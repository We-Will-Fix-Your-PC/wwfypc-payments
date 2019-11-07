#![feature(type_alias_impl_trait)]
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_derive_enum;
#[macro_use]
extern crate diesel_derives;
#[macro_use]
extern crate diesel_migrations;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate tera;

use actix::prelude::*;
use actix_cors::Cors;
use actix_redis::RedisSession;
use actix_web::{App, HttpRequest, HttpResponse, HttpServer, middleware, web};
use chrono::prelude::*;
use crypto::mac::Mac;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use futures::compat::Future01CompatExt;
use tera::Tera;

use dotenv::dotenv;
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex, RwLock};
use std::io::Read;

pub mod schema;
pub mod models;
pub mod oauth;
pub mod keycloak;
pub mod db;
pub mod util;
pub mod jobs;
pub mod apple_pay;

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

embed_migrations!("./migrations");

lazy_static! {
    pub static ref TERA: Tera = {
        let mut tera = compile_templates!("templates/**/*");
        tera.autoescape_on(vec!["html", ".sql"]);
        tera
    };
}

fn establish_connection() -> PgConnection {
    dotenv().ok();

    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    PgConnection::establish(&database_url)
        .expect(&format!("Error connecting to {}", database_url))
}

fn oauth_client() -> oauth::OAuthClient {
    dotenv().ok();

    let client_id = env::var("CLIENT_ID")
        .expect("CLIENT_ID must be set");
    let client_secret = env::var("CLIENT_SECRET")
        .expect("CLIENT_SECRET must be set");
    let well_known_url = env::var("OAUTH_WELL_KNOWN")
        .unwrap_or("https://account.cardifftec.uk/auth/realms/wwfypc-dev/.well-known/openid-configuration".to_string());

    let config = oauth::OAuthClientConfig::new(&client_id, &client_secret, &well_known_url).unwrap();

    oauth::OAuthClient::new(config)
}

fn keycloak_client() -> keycloak::KeycloakClient {
    dotenv().ok();

    let realm = env::var("KEYCLOAK_REALM")
        .unwrap_or("wwfypc-dev".to_string());
    let base_url = env::var("KEYCLOAK_BASE_URL")
        .unwrap_or("https://account.cardifftec.uk/auth/".to_string());

    let config = keycloak::KeycloakClientConfig::new(&base_url, &realm).unwrap();

    keycloak::KeycloakClient::new(config)
}

fn cookie_session() -> actix_redis::RedisSession {
    dotenv().ok();

    let key = match env::var("PRIVATE_KEY") {
        Ok(k) => k.into_bytes(),
        Err(_) => vec![0; 32]
    };
    let url = env::var("REDIS_URL")
        .unwrap_or("127.0.0.1:6379".to_string());

    RedisSession::new(url, &key)
        .cookie_name("wwfypc-payments-session")
        .cookie_secure(true)
}

fn worldpay_config() -> WorldpayConfig {
    dotenv().ok();

    WorldpayConfig {
        test_key: env::var("WORLDPAY_TEST_KEY").unwrap(),
        live_key: env::var("WORLDPAY_LIVE_KEY").unwrap(),
    }
}

fn mail_client() -> lettre::smtp::SmtpClient {
    dotenv().ok();

    let server = env::var("SMTP_SERVER")
        .unwrap_or("mail.misell.cymru".to_string());
    let username = env::var("SMTP_USERNAME")
        .expect("SMTP_USERNAME must be set");
    let password = env::var("SMTP_PASSWORD")
        .expect("SMTP_PASSWORD must be set");

    lettre::smtp::SmtpClient::new_simple(&server)
        .unwrap()
        .hello_name(lettre::smtp::extension::ClientId::Domain("payments.cardifftec.uk".to_string()))
        .connection_reuse(lettre::smtp::ConnectionReuseParameters::ReuseUnlimited)
        .credentials(lettre::smtp::authentication::Credentials::new(username, password))
        .authentication_mechanism(lettre::smtp::authentication::Mechanism::Plain)
        .smtp_utf8(true)
}

fn amqp_client() -> amqp::Channel {
    dotenv().ok();

    let server = env::var("AMPQ_SERVER")
        .unwrap_or("amqp://localhost//".to_string());
    let mut session = amqp::Session::open_url(&server).unwrap();
    let channel = session.open_channel(1).unwrap();
    channel
}

fn apple_pay_identity() -> reqwest::r#async::Client {
    dotenv().ok();
    let cert_path = env::var("APPLE_PAY_IDENTITY")
        .expect("APPLE_PAY_IDENTITY must be set");

    let mut buf = Vec::new();

    std::fs::File::open(cert_path).expect("Unable to open apple pay identity certificate")
        .read_to_end(&mut buf).expect("Unable to read apple pay identity certificate");

    let identity = reqwest::Identity::from_pkcs12_der(&buf, "").expect("Unable to decode apple pay identity certificate");

    reqwest::r#async::ClientBuilder::new()
        .identity(identity)
        .build()
        .expect("Unable to create apple pay client")
}

#[derive(Clone)]
struct WorldpayConfig {
    test_key: String,
    live_key: String,
}

#[derive(Clone)]
pub struct AppState {
    oauth: oauth::OAuthClient,
    keycloak: keycloak::KeycloakClient,
    worldpay: WorldpayConfig,
    apple_pay_client: reqwest::r#async::Client,
    db: Addr<db::DbExecutor>,
    jobs_state: jobs::JobsState,
}

#[derive(Clone, Debug, Deserialize)]
struct NewPaymentItemData {
    item_type: String,
    item_data: serde_json::Value,
    title: String,
    quantity: i32,
    price: rust_decimal::Decimal,
}

#[derive(Clone, Debug, Deserialize)]
struct NewPaymentData {
    environment: models::PaymentEnvironment,
    customer_id: uuid::Uuid,
    items: Vec<NewPaymentItemData>,
}

#[derive(Clone, Debug, Serialize)]
struct NewPaymentResponseData {
    id: uuid::Uuid,
}

async fn new_payment(token: oauth::BearerAuthToken, data: web::Data<AppState>, new_payment: web::Json<NewPaymentData>) -> actix_web::Result<impl actix_web::Responder> {
    let new_payment_items = new_payment.items.to_owned();

    data.oauth.verify_token(token.token(), "create-payments").await?;

    let payment_id = uuid::Uuid::new_v4();

    let items: Vec<db::CreatePaymentItem> = new_payment_items.into_iter()
        .map(|i| db::CreatePaymentItem::new(
            &uuid::Uuid::new_v4(),
            &i.item_type,
            &i.item_data,
            &i.title,
            i.quantity,
            &i.price,
        ))
        .collect();

    let res = data.db.send(db::CreatePayment::new(
        &payment_id,
        &Utc::now().naive_utc(),
        models::PaymentState::OPEN,
        new_payment.environment,
        &new_payment.customer_id,
        &items,
    )).compat().await?;

    match res {
        Ok(payment) => {
            let response = NewPaymentResponseData {
                id: payment.id
            };
            Ok(HttpResponse::Ok().json(response))
        }
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e)),
    }
}

#[derive(Clone, Debug, Serialize)]
struct PaymentItemResponseData {
    id: uuid::Uuid,
    #[serde(rename = "type")]
    item_type: String,
    #[serde(rename = "data")]
    item_data: serde_json::Value,
    title: String,
    price: f64,
}

#[derive(Clone, Debug, Serialize)]
struct PaymentCustomerResponseData {
    id: uuid::Uuid,
    name: String,
    email: Option<String>,
    phone: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct PaymentResponseData {
    id: uuid::Uuid,
    timestamp: DateTime<Utc>,
    state: models::PaymentState,
    environment: models::PaymentEnvironment,
    customer: PaymentCustomerResponseData,
    items: Vec<PaymentItemResponseData>,
}

async fn get_payment<'a>(data: web::Data<AppState>, info: web::Path<uuid::Uuid>, session: actix_session::Session) -> actix_web::Result<impl actix_web::Responder> {
    match match session.get::<uuid::Uuid>("sess_id") {
        Ok(s) => s,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Some(_) => {}
        None => match session.set("sess_id", uuid::Uuid::new_v4()) {
            Ok(_) => {}
            Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
        }
    }

    let payment = match match data.db.send(db::GetPayment::new(&info.into_inner())).compat().await {
        Ok(payment) => payment,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Ok(payment) => payment,
        Err(e) => return match e {
            diesel::result::Error::NotFound => Err(actix_web::error::ErrorNotFound(e)),
            _ => Err(actix_web::error::ErrorInternalServerError(e))
        }
    };
    let items = match match data.db.send(db::GetPaymentItems::new(&payment)).compat().await {
        Ok(items) => items,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Ok(items) => items,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    };

    let token = data.oauth.get_access_token().await?;
    let user = data.keycloak.clone().get_user(payment.customer_id, &token).await?;

    let response_data = PaymentResponseData {
        id: payment.id,
        timestamp: DateTime::<Utc>::from_utc(payment.time, Utc),
        state: payment.state,
        environment: payment.environment,
        customer: PaymentCustomerResponseData {
            id: user.id,
            name: format!("{} {}", user.first_name.unwrap_or("".to_string()), user.last_name.unwrap_or("".to_string())),
            email: user.email,
            phone: match user.attributes {
                Some(a) => match a.get("phone") {
                    Some(p) => match p.first() {
                        Some(s) => Some(s.to_owned()),
                        None => None
                    },
                    None => None
                },
                None => None
            },
        },
        items: items.into_iter()
            .map(|item| PaymentItemResponseData {
                id: item.id,
                item_type: item.item_type,
                item_data: item.item_data,
                title: item.title,
                price: (item.price.0 as f64) / 100.0,
            })
            .collect(),
    };

    Ok(HttpResponse::Ok().json(response_data))
}

#[derive(Clone, Debug, Deserialize)]
struct PaymentStateData {
    state: Option<String>
}

async fn render_payment(req: HttpRequest, data: web::Data<AppState>, info: web::Path<uuid::Uuid>, form: Option<web::Form<PaymentStateData>>, template_name: &str) -> actix_web::Result<impl actix_web::Responder> {
    let payment = match match data.db.send(db::GetPayment::new(&info.into_inner())).compat().await {
        Ok(r) => r,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Ok(r) => r,
        Err(e) => return match e {
            diesel::result::Error::NotFound => Err(actix_web::error::ErrorNotFound(e)),
            _ => Err(actix_web::error::ErrorInternalServerError(e))
        }
    };

    let is_open_payment = payment.state == models::PaymentState::OPEN;
    let is_test = payment.environment != models::PaymentEnvironment::LIVE;
    let accepts = match req.headers().get(actix_web::http::header::ACCEPT) {
        Some(a) => match a.to_str() {
            Ok(a) => a,
            Err(e) => return Err(actix_web::error::ErrorBadRequest(e))
        },
        None => "*/*"
    };
    let state = match form {
        Some(f) => f.state.to_owned(),
        None => None
    };

    let mut context = tera::Context::new();
    context.insert("payment_id", &payment.id);
    context.insert("is_open_payment", &is_open_payment);
    context.insert("test", &is_test);
    context.insert("accepts_header", &accepts);
    context.insert("state", &state);

    match TERA.render(template_name, &context) {
        Ok(r) => Ok(HttpResponse::Ok().body(r)),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
    }
}

async fn render_fb_payment_get(req: HttpRequest, data: web::Data<AppState>, info: web::Path<uuid::Uuid>) -> actix_web::Result<impl actix_web::Responder> {
    render_payment(req, data, info, None, "fb_payment.html").await
}

async fn render_fb_payment_post(req: HttpRequest, data: web::Data<AppState>, info: web::Path<uuid::Uuid>, form: web::Form<PaymentStateData>) -> actix_web::Result<impl actix_web::Responder> {
    render_payment(req, data, info, Some(form), "fb_payment.html").await
}


#[derive(Clone, Debug, Deserialize)]
struct BillingAddressData {
    #[serde(rename = "addressLine")]
    address_line: Vec<String>,
    country: String,
    city: String,
    #[serde(rename = "postalCode")]
    postal_code: String,
    region: String,
    phone: String,
}

#[derive(Clone, Deserialize)]
struct WorldpayPaymentData {
    accepts: String,
    email: String,
    phone: String,
    #[serde(rename = "payerName")]
    payer_name: String,
    token: String,
    payment: Option<WorldpayNewPaymentData>,
    #[serde(rename = "billingAddress")]
    billing_address: BillingAddressData,
}

#[derive(Clone, Deserialize)]
struct WorldpayNewPaymentItemData {
    #[serde(rename = "type")]
    item_type: String,
    #[serde(rename = "data")]
    item_data: serde_json::Value,
    title: String,
    quantity: i32,
    price: rust_decimal::Decimal,
    #[serde(with = "serde_hex::SerHex::<serde_hex::Strict>")]
    sig: [u8; 64],
}

#[derive(Clone, Debug, Deserialize)]
struct WorldpayNewCustomerData {
    email: String,
    phone: String,
    name: String,
}

#[derive(Clone, Deserialize)]
struct WorldpayNewPaymentData {
    environment: models::PaymentEnvironment,
    customer: WorldpayNewCustomerData,
    items: Vec<WorldpayNewPaymentItemData>,
}

#[derive(Clone, Debug, Serialize)]
enum WorldpayPaymentStatus {
    SUCCESS,
    FAILED,
    #[serde(rename = "3DS")]
    THREEDS,
    #[serde(rename = "EXISTING_ACCOUNT")]
    ExistingAccount,
    UNKNOWN,
}

#[derive(Clone, Debug, Serialize)]
struct WorldpayPaymentDataResp {
    state: WorldpayPaymentStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct WorldpayBillingAddress {
    address1: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    address2: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    address3: Option<String>,
    #[serde(rename = "postalCode")]
    postal_code: String,
    city: String,
    #[serde(rename = "countryCode")]
    country_code: String,
    state: String,
    #[serde(rename = "telephoneNumber")]
    telephone_number: String,
}

impl From<&BillingAddressData> for WorldpayBillingAddress {
    fn from(data: &BillingAddressData) -> Self {
        let address1 = match data.address_line.get(0) {
            Some(l) => l.to_string(),
            None => "".to_string()
        };
        let address2 = match data.address_line.get(1) {
            Some(l) => Some(l.to_string()),
            None => None
        };
        let address3 = match data.address_line.get(2) {
            Some(l) => Some(l.to_string()),
            None => None
        };

        Self {
            address1,
            address2,
            address3,
            city: data.city.clone(),
            state: data.region.clone(),
            postal_code: data.postal_code.clone(),
            country_code: data.country.clone(),
            telephone_number: data.phone.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct WorldpayOrder {
    #[serde(rename = "orderType")]
    order_type: String,
    #[serde(rename = "orderDescription")]
    order_description: String,
    #[serde(rename = "customerOrderCode")]
    customer_order_code: String,
    amount: i64,
    #[serde(rename = "currencyCode")]
    currency_code: String,
    name: String,
    #[serde(rename = "shopperEmailAddress")]
    shopper_email_address: String,
    #[serde(rename = "billingAddress")]
    billing_address: WorldpayBillingAddress,
    #[serde(rename = "shopperIpAddress")]
    shopper_ip_address: String,
    #[serde(rename = "shopperUserAgent")]
    shopper_user_agent: String,
    #[serde(rename = "shopperAcceptHeader")]
    shopper_accept_header: String,
    #[serde(rename = "shopperSessionId")]
    shopper_session_id: String,
    #[serde(rename = "is3DSOrder")]
    is_3ds_order: bool,
    #[serde(rename = "authorizeOnly")]
    authorize_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct WorldpayThreedsOrder {
    #[serde(rename = "threeDSResponseCode")]
    threeds_response_code: String,
    #[serde(rename = "shopperIpAddress")]
    shopper_ip_address: String,
    #[serde(rename = "shopperUserAgent")]
    shopper_user_agent: String,
    #[serde(rename = "shopperAcceptHeader")]
    shopper_accept_header: String,
    #[serde(rename = "shopperSessionId")]
    shopper_session_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
enum WorldpayOrderStatus {
    #[serde(rename = "SUCCESS")]
    Success,
    #[serde(rename = "FAILED")]
    Failed,
    #[serde(rename = "SENT_FOR_REFUND")]
    SentForRefund,
    #[serde(rename = "REFUNDED")]
    Refunded,
    #[serde(rename = "PARTIALLY_REFUNDED")]
    PartialyRefunded,
    #[serde(rename = "AUTHORIZED")]
    Authorized,
    #[serde(rename = "PRE_AUTHORIZED")]
    PreAuthorized,
    #[serde(rename = "CANCELLED")]
    Cancelled,
    #[serde(rename = "EXPIRED")]
    Expride,
    #[serde(rename = "SETTLED")]
    Settled,
    #[serde(rename = "CHARGED_BACK")]
    ChargedBack,
    #[serde(rename = "INFORMATION_REQUESTED")]
    InformationRequested,
    #[serde(rename = "INFORMATION_SUPPLIED")]
    InformationSupplied,
}

#[derive(Clone, Debug, Deserialize)]
struct WorldpayOrderPaymentResponse {
    #[serde(rename = "cardIssuer")]
    card_issuer: String,
    #[serde(rename = "maskedCardNumber")]
    masked_card_number: String,
}

#[derive(Clone, Debug, Deserialize)]
struct WorldpayOrderResp {
    #[serde(rename = "orderCode")]
    order_code: String,
    #[serde(rename = "paymentStatus")]
    payment_status: WorldpayOrderStatus,
    #[serde(rename = "paymentResponse")]
    payment_response: WorldpayOrderPaymentResponse,
    #[serde(rename = "redirectURL")]
    redirect_url: Option<String>,
    #[serde(rename = "oneTime3DsToken")]
    one_time_3ds_token: Option<String>,
}

async fn process_worldpay_payment(req: HttpRequest, data: web::Data<AppState>, info: web::Path<uuid::Uuid>, session: actix_session::Session, payment_data: web::Json<WorldpayPaymentData>) -> actix_web::Result<impl actix_web::Responder> {
    let sess_id = match match session.get::<uuid::Uuid>("sess_id") {
        Ok(s) => s,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Some(s) => s,
        None => {
            let s = uuid::Uuid::new_v4();
            match session.set("sess_id", s) {
                Ok(_) => s,
                Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
            }
        }
    };

    let token = data.oauth.clone().get_access_token().await?;

    let payment = match match data.db.send(db::GetPayment::new(&info)).compat().await {
        Ok(r) => r,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Ok(r) => r,
        Err(e) => match (e, payment_data.payment.as_ref()) {
            (diesel::result::Error::NotFound, Some(payment)) => {
                let tokens = match match data.db.send(db::GetPaymentTokens::new()).compat().await {
                    Ok(r) => r,
                    Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
                } {
                    Ok(r) => r,
                    Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
                };
                let mut items: Vec<db::CreatePaymentItem> = vec![];
                for i in payment.items.iter() {
                    let digest = crypto::sha2::Sha512::new();
                    let mut price = i.price * rust_decimal::Decimal::new(100, 0);
                    price.set_scale(0).unwrap();
                    let hmac_data = format!("{}{}{}{}{}", i.item_type, i.item_data, i.title, i.quantity, price.to_string()).into_bytes();
                    let sig = crypto::mac::MacResult::new(&i.sig);

                    let mut validated = false;
                    for token in &tokens {
                        let mut hmac = crypto::hmac::Hmac::new(digest, &token.token);
                        hmac.input(&hmac_data);
                        let res = hmac.result();

                        if res == sig {
                            validated = true;
                        }
                    }
                    if !validated {
                        return Err(actix_web::error::ErrorBadRequest("invalid signature"));
                    }

                    items.push(db::CreatePaymentItem::new(
                        &uuid::Uuid::new_v4(),
                        &i.item_type,
                        &i.item_data,
                        &i.title,
                        i.quantity,
                        &i.price,
                    ));
                }

                let user_id = match session.get::<oauth::OAuthToken>("oauth_token") {
                    Ok(s) => match s {
                        Some(oauth_token) => {
                            let (introspect, oauth_token) = data.oauth.update_and_verify_token(oauth_token, None).await?;
                            match session.set("oauth_token", oauth_token) {
                                Ok(_) => {}
                                Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
                            }

                            match match introspect.sub {
                                Some(u) => uuid::Uuid::parse_str(&u),
                                None => return Err(actix_web::error::ErrorInternalServerError(""))
                            } {
                                Ok(u) => u,
                                Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
                            }
                        }
                        None => match data.keycloak.get_user_by_email(&payment.customer.email, &token).await? {
                            Some(_) => {
                                return Ok(HttpResponse::Ok().json(WorldpayPaymentDataResp {
                                    state: WorldpayPaymentStatus::ExistingAccount,
                                    frame: Some(format!("https://{}/login/auth/?{}", req.connection_info().host(), serde_urlencoded::to_string(&[
                                        ("next", format!("https://{}/payment/login-complete/", req.connection_info().host())),
                                    ]).unwrap())),
                                }));
                            }
                            None => {
                                let mut u = data.keycloak.create_user(&payment.customer.email, &token).await?;
                                u.first_name = Some(payment.customer.name.clone());
                                u.set_attribute("phone", &payment.customer.phone);
                                u.update(&token).await?;
                                u.required_actions(&[
                                    "UPDATE_PASSWORD",
                                    "UPDATE_PROFILE",
                                    "VERIFY_EMAIL"
                                ], &token).await?;

                                u.id
                            }
                        }
                    },
                    Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
                };

                let payment = db::CreatePayment::new(
                    &info.into_inner(),
                    &Utc::now().naive_utc(),
                    models::PaymentState::OPEN,
                    payment.environment,
                    &user_id,
                    &items,
                );

                match match data.db.send(payment).compat().await {
                    Ok(r) => r,
                    Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
                } {
                    Ok(r) => r,
                    Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
                }
            }
            (diesel::result::Error::NotFound, None) => return Err(actix_web::error::ErrorNotFound(diesel::result::Error::NotFound)),
            (e, _) => return Err(actix_web::error::ErrorInternalServerError(e))
        }
    };

    let items = match match data.db.send(db::GetPaymentItems::new(&payment)).compat().await {
        Ok(r) => r,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Ok(r) => r,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    };

    let mut user = data.keycloak.clone().get_user(payment.customer_id, &token).await?;

    user.add_role(&["customer"], &token).await?;
    if let None = user.email {
        user.email = Some(payment_data.email.clone());
    }
    if let None = user.first_name {
        user.first_name = Some(payment_data.payer_name.clone());
    }
    if !user.has_attribute("phone") {
        user.set_attribute("phone", &payment_data.billing_address.phone);
    }
    user.update(&token).await?;

    let billing_address = WorldpayBillingAddress::from(&payment_data.billing_address);

    let description: String = items.iter().map(|i| i.title.clone()).collect::<Vec<String>>().join(", ");
    let total = items.iter().map(|i| i.price.0 * i.quantity as i64).fold(0, |acc, i| acc + i);

    let mut order_data = WorldpayOrder {
        order_type: "ECOM".to_string(),
        order_description: description,
        customer_order_code: payment.id.to_string(),
        amount: total,
        currency_code: "GBP".to_string(),
        name: payment_data.payer_name.clone(),
        shopper_email_address: payment_data.email.clone(),
        billing_address,
        shopper_ip_address: req.connection_info().remote().unwrap_or("").to_string(),
        shopper_user_agent: req.headers().get(actix_web::http::header::USER_AGENT)
            .unwrap_or(&actix_web::http::header::HeaderValue::from_static("")).to_str()
            .unwrap_or("").to_string(),
        shopper_accept_header: payment_data.accepts.clone(),
        shopper_session_id: sess_id.to_string(),
        is_3ds_order: true,
        authorize_only: total == 0,
        token: None,
    };

    order_data.token = Some(payment_data.token.to_string());
    match match data.db.send(db::CreateCardToken::new(
        &payment.customer_id,
        &payment_data.token,
    )).compat().await {
        Ok(r) => r,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Ok(_) => {}
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    };

    let worldpay_token = match payment.environment {
        models::PaymentEnvironment::LIVE => &data.worldpay.live_key,
        models::PaymentEnvironment::TEST => &data.worldpay.test_key,
    };

    let mut c = util::async_reqwest_to_error(
        reqwest::r#async::Client::new().post("https://api.worldpay.com/v1/orders")
            .header(reqwest::header::AUTHORIZATION, worldpay_token)
            .json(&order_data)
    ).await?;
    let r = match c.json::<WorldpayOrderResp>().compat().await {
        Ok(c) => c,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    };

    match r.payment_status {
        WorldpayOrderStatus::Success | WorldpayOrderStatus::Authorized => {
            match match data.db.send(db::UpdatePaymentState::new(
                &payment.id,
                models::PaymentState::PAID,
                Some(&format!("{} {}", r.payment_response.card_issuer, r.payment_response.masked_card_number)),
            )).compat().await {
                Ok(r) => r,
                Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
            } {
                Ok(_) => {
                    let job = jobs::CompletePayment::new(&payment.id);
                    let job_state = data.jobs_state.clone();
                    std::thread::spawn(move || {
                        jobs::send_payment_notification(job, job_state)
                    });
                }
                Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
            };

            Ok(HttpResponse::Ok().json(WorldpayPaymentDataResp {
                state: WorldpayPaymentStatus::SUCCESS,
                frame: None,
            }))
        }
        WorldpayOrderStatus::PreAuthorized => {
            match match data.db.send(db::CreateThreedsData::new(
                &payment.id,
                &r.one_time_3ds_token.unwrap(),
                &r.redirect_url.unwrap(),
                &r.order_code,
            )).compat().await {
                Ok(r) => r,
                Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
            } {
                Ok(_) => {}
                Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
            };

            Ok(HttpResponse::Ok().json(WorldpayPaymentDataResp {
                state: WorldpayPaymentStatus::THREEDS,
                frame: Some(format!("https://{}/payment/3ds/{}/", req.connection_info().host(), payment.id)),
            }))
        }
        WorldpayOrderStatus::Failed => Ok(HttpResponse::Ok().json(WorldpayPaymentDataResp {
            state: WorldpayPaymentStatus::FAILED,
            frame: None,
        })),
        _ => Ok(HttpResponse::Ok().json(WorldpayPaymentDataResp {
            state: WorldpayPaymentStatus::UNKNOWN,
            frame: None,
        }))
    }
}

async fn render_3ds_form<'a>(req: HttpRequest, data: web::Data<AppState>, info: web::Path<uuid::Uuid>) -> actix_web::Result<impl actix_web::Responder> {
    let payment = match match data.db.send(db::GetPayment::new(&info.into_inner())).compat().await {
        Ok(r) => r,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Ok(r) => r,
        Err(e) => return match e {
            diesel::result::Error::NotFound => Err(actix_web::error::ErrorNotFound(e)),
            _ => Err(actix_web::error::ErrorInternalServerError(e))
        }
    };

    let threeds_data = match match data.db.send(db::GetThreedsData::new(&payment)).compat().await {
        Ok(r) => r,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Ok(r) => r,
        Err(e) => return match e {
            diesel::result::Error::NotFound => Err(actix_web::error::ErrorNotFound(e)),
            _ => Err(actix_web::error::ErrorInternalServerError(e))
        }
    };

    let connection_info = req.connection_info();
    let mut context = tera::Context::new();
    context.insert("redirect", &format!("https://{}/payment/3ds-complete/{}/", connection_info.host(), payment.id));
    context.insert("redirect_url", &threeds_data.redirect_url);
    context.insert("one_time_3ds_token", &threeds_data.one_time_3ds_token);
    context.insert("order_id", &threeds_data.order_id);

    match TERA.render("3ds_form.html", &context) {
        Ok(r) => Ok(HttpResponse::Ok().body(r)),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
    }
}

#[derive(Clone, Debug, Deserialize)]
struct ThreedsData {
    #[serde(rename = "MD")]
    order_id: String,
    #[serde(rename = "PaRes")]
    response_code: String,
}

async fn render_3ds_complete<'a>(req: HttpRequest, data: web::Data<AppState>, info: web::Path<uuid::Uuid>, session: actix_session::Session, form: web::Form<ThreedsData>) -> actix_web::Result<impl actix_web::Responder> {
    let sess_id = match match session.get::<uuid::Uuid>("sess_id") {
        Ok(s) => s,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Some(s) => s.to_string(),
        None => "".to_string()
    };

    let payment = match match data.db.send(db::GetPayment::new(&info.into_inner())).compat().await {
        Ok(r) => r,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Ok(r) => r,
        Err(e) => return match e {
            diesel::result::Error::NotFound => Err(actix_web::error::ErrorNotFound(e)),
            _ => Err(actix_web::error::ErrorInternalServerError(e))
        }
    };
    match match data.db.send(db::DeleteThreedsData::new(&payment)).compat().await {
        Ok(r) => r,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Ok(_) => {}
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    };

    let worldpay_token = match payment.environment {
        models::PaymentEnvironment::LIVE => &data.worldpay.live_key,
        models::PaymentEnvironment::TEST => &data.worldpay.test_key,
    };

    let order_data = WorldpayThreedsOrder {
        threeds_response_code: form.response_code.clone(),
        shopper_ip_address: req.connection_info().remote().unwrap_or("").to_string(),
        shopper_user_agent: req.headers().get(actix_web::http::header::USER_AGENT)
            .unwrap_or(&actix_web::http::header::HeaderValue::from_static("")).to_str()
            .unwrap_or("").to_string(),
        shopper_accept_header: req.headers().get(actix_web::http::header::ACCEPT)
            .unwrap_or(&actix_web::http::header::HeaderValue::from_static("*/*")).to_str()
            .unwrap_or("*/*").to_string(),
        shopper_session_id: sess_id.to_string(),
    };

    let mut context = tera::Context::new();
    context.insert("payment_id", &payment.id);

    match {
        match util::async_reqwest_to_error(
            reqwest::r#async::Client::new().put(reqwest::Url::parse(&format!("https://api.worldpay.com/v1/orders/{}", form.order_id)).unwrap())
                .header(reqwest::header::AUTHORIZATION, worldpay_token)
                .json(&order_data)
        ).await {
            Ok(mut c) => match c.json::<WorldpayOrderResp>().compat().await {
                Ok(r) => match r.payment_status {
                    WorldpayOrderStatus::Success | WorldpayOrderStatus::Authorized => {
                        match data.db.send(db::UpdatePaymentState::new(
                            &payment.id,
                            models::PaymentState::PAID,
                            Some(&format!("{} {}", r.payment_response.card_issuer, r.payment_response.masked_card_number)),
                        )).compat().await {
                            Ok(r) => match r {
                                Ok(_) => {
                                    let job = jobs::CompletePayment::new(&payment.id);
                                    let job_state = data.jobs_state.clone();
                                    std::thread::spawn(move || {
                                        jobs::send_payment_notification(job, job_state)
                                    });
                                    context.insert("threeds_approved", &true);
                                    Ok(())
                                }
                                Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
                            },
                            Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
                        }
                    }
                    _ => Err(actix_web::error::ErrorBadRequest(""))
                },
                Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
            },
            Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
        }
    } {
        Ok(_) => {}
        Err(_) => {
            context.insert("threeds_approved", &false);
        }
    }

    match TERA.render("3ds_complete.html", &context) {
        Ok(r) => Ok(HttpResponse::Ok().body(r)),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e).into())
    }
}

async fn render_login_complete<'a>(data: web::Data<AppState>, session: actix_session::Session) -> actix_web::Result<impl actix_web::Responder> {
    let mut context = tera::Context::new();

    match {
        match session.get::<oauth::OAuthToken>("oauth_token") {
            Ok(s) => match s {
                Some(oauth_token) => {
                    match data.oauth.update_and_verify_token(oauth_token, None).await {
                        Ok((_, oauth_token)) => {
                            match session.set("oauth_token", oauth_token) {
                                Ok(_) => Ok(()),
                                Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
                            }
                        }
                        Err(e) => Err(e.into())
                    }
                }
                None => Err(actix_web::error::ErrorInternalServerError(""))
            },
            Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
        }
    } {
        Ok(_) => {
            context.insert("login_successful", &true);
        }
        Err(_) => {
            context.insert("login_successful", &false);
        }
    }

    match TERA.render("login_complete.html", &context) {
        Ok(r) => Ok(HttpResponse::Ok().body(r)),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
    }
}

#[derive(Serialize, Deserialize)]
struct OauthState {
    id: uuid::Uuid,
    redirect_uri: String,
    next_uri: Option<String>,
}

#[derive(Deserialize)]
struct OauthLoginInfo {
    next: Option<String>,
}


#[derive(Deserialize)]
struct OauthCallbackInfo {
    state: uuid::Uuid,
    code: String,
    error: Option<String>,
}

async fn start_login(req: HttpRequest, data: web::Data<AppState>, info: web::Query<OauthLoginInfo>, session: actix_session::Session) -> actix_web::Result<impl actix_web::Responder> {
    let redirect_uri = format!("https://{}/login/redirect/", req.connection_info().host());

    let state = OauthState {
        id: uuid::Uuid::new_v4(),
        redirect_uri: redirect_uri.clone(),
        next_uri: info.next.clone(),
    };

    let id_str = state.id.to_string();

    let url = data.oauth.authorization_url(&[
        "openid",
        "email",
        "profile"
    ], "code", Some(&id_str), Some(&redirect_uri)).await?;

    match session.set("login_state", state) {
        Ok(s) => s,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    };

    Ok(
        HttpResponse::Found()
            .header(actix_web::http::header::LOCATION, url)
            .finish()
    )
}

async fn login_callback(data: web::Data<AppState>, info: web::Query<OauthCallbackInfo>, session: actix_session::Session) -> actix_web::Result<impl actix_web::Responder> {
    let state = match match session.get::<OauthState>("login_state") {
        Ok(s) => s,
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    } {
        Some(s) => s,
        None => return Err(actix_web::error::ErrorInternalServerError(""))
    };

    if Option::is_none(&info.error) && state.id == info.state {
        let oauth_token = data.oauth.token_exchange(&info.code, Some(&state.redirect_uri)).await?;

        match session.set("oauth_token", oauth_token) {
            Ok(s) => s,
            Err(_) => {}
        };
    }

    let mut resp = HttpResponse::Found();
    if let Some(next) = state.next_uri {
        resp.header(actix_web::http::header::LOCATION, next);
    } else {
        resp.header(actix_web::http::header::LOCATION, "/");
    }
    Ok(resp.finish())
}

fn main() {
    pretty_env_logger::init();

    info!("Migrating database...");
    let connection = establish_connection();
    embedded_migrations::run_with_output(&connection, &mut std::io::stdout())
        .expect("Unable to run migrations");
    info!("Migrations complete!");

    let sys = actix::System::new("wwfypc-payments");

    let db_addr = SyncArbiter::start(3, || {
        db::DbExecutor::new(establish_connection())
    });

    let oauth_client = oauth_client();
    let keycloak_client = keycloak_client();
    let mail_client = mail_client();
    let worldpay_config = worldpay_config();
    let amqp_client = amqp_client();

    let jobs_data = jobs::JobsState {
        db: db_addr.clone(),
        keycloak: keycloak_client.clone(),
        oauth: oauth_client.clone(),
        mail_client,
        amqp: Arc::new(Mutex::new(amqp_client)),
    };

    let data = AppState {
        oauth: oauth_client,
        keycloak: keycloak_client,
        worldpay: worldpay_config,
        apple_pay_client: apple_pay_identity(),
        db: db_addr,
        jobs_state: jobs_data,
    };

    let mut server = HttpServer::new(move || {
        let generated = generate();

        App::new()
            .data(data.clone())
            .wrap(middleware::Logger::default())
            .wrap(middleware::Compress::default())
            .wrap(cookie_session())
            .service(actix_web_static_files::ResourceFiles::new(
                "/static",
                generated,
            ))
            .route(".well-known/apple-developer-merchantid-domain-association.txt", web::get().to(|| HttpResponse::Ok().body(
                actix_web::dev::Body::from_slice(include_bytes!("../apple-developer-merchantid-domain-association.txt")))))
            .route("/login/auth/", web::get().to_async(actix_web_async_await::compat4(start_login)))
            .route("/login/redirect/", web::get().to_async(actix_web_async_await::compat3(login_callback)))
            .route("/apple-merchant-verification/", web::post().to_async(actix_web_async_await::compat3(apple_pay::merchant_verification)))
            .route("/payment/new/", web::post().to_async(actix_web_async_await::compat3(new_payment)))
            .route("/payment/login-complete/", web::get().to_async(actix_web_async_await::compat2(render_login_complete)))
            .service(
                web::resource("/payment/{payment_id}/")
                    .wrap(Cors::new()
                        .supports_credentials())
                    .route(web::get().to_async(actix_web_async_await::compat3(get_payment)))
            )
            .service(
                web::resource("/payment/worldpay/{payment_id}/")
                    .wrap(Cors::new()
                        .supports_credentials())
                    .route(web::post().to_async(actix_web_async_await::compat5(process_worldpay_payment)))
            )
            .route("/payment/3ds/{payment_id}/", web::get().to_async(actix_web_async_await::compat3(render_3ds_form)))
            .route("/payment/3ds-complete/{payment_id}/", web::post().to_async(actix_web_async_await::compat5(render_3ds_complete)))
            .service(web::resource("/payment/fb/{payment_id}/")
                .route(web::get().to_async(actix_web_async_await::compat3(render_fb_payment_get)))
                .route(web::post().to_async(actix_web_async_await::compat4(render_fb_payment_post))))
    });

    let mut listenfd = listenfd::ListenFd::from_env();

    info!("Start listening...");
    server = if let Some(l) = listenfd.take_tcp_listener(0).unwrap() {
        server.listen(l).unwrap()
    } else {
        server.bind("127.0.0.1:3000").unwrap()
    };

    server.start();
    let _ = sys.run();
}

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;
#[macro_use]
extern crate diesel_derives;
extern crate dotenv;
#[macro_use]
extern crate log;
extern crate pretty_env_logger;
extern crate actix_web;
extern crate actix_cors;
extern crate actix;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate tera;
extern crate reqwest;
#[macro_use]
extern crate serde;
extern crate serde_hex;
extern crate serde_json;
extern crate futures;
extern crate http;
extern crate rust_decimal;
#[macro_use]
extern crate diesel_derive_enum;
extern crate uuid;
extern crate chrono;
extern crate crypto;

pub mod schema;
pub mod models;
pub mod oauth;
pub mod keycloak;
pub mod db;

use actix::prelude::*;
use actix_cors::Cors;
use diesel::prelude::*;
use diesel::pg::PgConnection;
use dotenv::dotenv;
use std::env;
use actix_web::{web, middleware, App, HttpServer, HttpRequest, HttpResponse};
use tera::Tera;
use futures::future::{err, ok, Either, join_all};
use chrono::prelude::*;
use std::collections::HashMap;
use crypto::mac::Mac;

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

fn cookie_session() -> actix_session::CookieSession {
    dotenv().ok();

    let key = match env::var("PRIVATE_KEY") {
        Ok(k) => k.into_bytes(),
        Err(_) => vec![0; 32]
    };

    actix_session::CookieSession::private(&key)
        .name("wwfypc_payment_session")
        .path("/")
        .http_only(true)
        .secure(true)
}

fn worldpay_config() -> WorldpayConfig {
    dotenv().ok();

    WorldpayConfig {
        test_key: env::var("WORLDPAY_TEST_KEY").unwrap(),
        live_key: env::var("WORLDPAY_LIVE_KEY").unwrap(),
    }
}

#[derive(Clone)]
struct WorldpayConfig {
    test_key: String,
    live_key: String,
}

#[derive(Clone)]
struct AppState {
    oauth: oauth::OAuthClient,
    keycloak: keycloak::KeycloakClient,
    worldpay: WorldpayConfig,
    db: Addr<db::DbExecutor>,
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

async fn new_payment(token: oauth::BearerAuthToken, data: web::Data<AppState>, new_payment: web::Json<NewPaymentData>) {
    let new_payment_items = new_payment.items.to_owned();

//    data.oauth.clone().verify_token(token.token().to_owned(), "create-payments")
//        .and_then(move |_| {
//            let payment_id = uuid::Uuid::new_v4();
//
//            let items: Vec<db::CreatePaymentItem> = new_payment_items.into_iter()
//                .map(|i| db::CreatePaymentItem::new(
//                    &uuid::Uuid::new_v4(),
//                    &i.item_type,
//                    &i.item_data,
//                    &i.title,
//                    i.quantity,
//                    &i.price,
//                ))
//                .collect();
//
//            data_1.db.send(db::CreatePayment::new(
//                &payment_id,
//                &Utc::now().naive_utc(),
//                models::PaymentState::OPEN,
//                new_payment.environment,
//                &new_payment.customer_id,
//                &items,
//            ))
//                .from_err()
//        })
//        .and_then(|res| match res {
//            Ok(payment) => Ok(payment),
//            Err(e) => Err(actix_web::error::ErrorInternalServerError(e)),
//        })
//        .map(move |payment| {
//            let response = NewPaymentResponseData {
//                id: payment.id
//            };
//            HttpResponse::Ok().json(response)
//        })

    data.oauth.clone().verify_token(token.token().to_owned(), "create-payments").await?;

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
    )).await?;

    match res {
        Ok(payment) => {
             let response = NewPaymentResponseData {
                id: payment.id
            };
            Ok(HttpResponse::Ok().json(response))
        },
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

fn get_payment<'a>(data: web::Data<AppState>, info: web::Path<(uuid::Uuid)>, session: actix_session::Session) -> impl Future<Item=impl actix_web::Responder, Error=actix_web::error::Error> + 'a {
    match match session.get::<uuid::Uuid>("sess_id") {
        Ok(s) => s,
        Err(e) => return Either::A(err(actix_web::error::ErrorInternalServerError(e)))
    } {
        Some(_) => {}
        None => match session.set("sess_id", uuid::Uuid::new_v4()) {
            Ok(_) => {}
            Err(e) => return Either::A(err(actix_web::error::ErrorInternalServerError(e)))
        }
    }

    Either::B(data.db.send(db::GetPayment::new(&info.into_inner()))
        .from_err()
        .and_then(|res| match res {
            Ok(payment) => Ok(payment),
            Err(e) => Err(actix_web::error::ErrorNotFound(e))
        })
        .and_then(move |payment| {
            data.db.send(db::GetPaymentItems::new(&payment))
                .from_err()
                .and_then(|res| match res {
                    Ok(items) => Ok(items),
                    Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
                })
                .and_then(move |items| {
                    data.oauth.clone().get_access_token()
                        .and_then(move |token| {
                            data.keycloak.clone().get_user(payment.customer_id, &token)
                                .and_then(move |user| {
                                    HttpResponse::Ok().json(PaymentResponseData {
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
                                    })
                                })
                        })
                })
        }))
}

#[derive(Clone, Debug, Deserialize)]
struct PaymentStateData {
    state: Option<String>
}

fn render_payment<'a>(req: HttpRequest, data: web::Data<AppState>, info: web::Path<(uuid::Uuid)>, form: Option<web::Form<PaymentStateData>>, template_name: &'a str) -> impl Future<Item=impl actix_web::Responder, Error=actix_web::error::Error> + 'a {
    data.db.send(db::GetPayment::new(&info.into_inner()))
        .from_err()
        .and_then(|res| match res {
            Ok(payment) => Ok(payment),
            Err(e) => Err(actix_web::error::ErrorNotFound(e))
        })
        .and_then(move |payment| {
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
        })
}

fn render_fb_payment_get<'a>(req: HttpRequest, data: web::Data<AppState>, info: web::Path<(uuid::Uuid)>) -> impl Future<Item=impl actix_web::Responder, Error=actix_web::error::Error> + 'a {
    render_payment(req, data, info, None, "fb_payment.html")
}

fn render_fb_payment_post<'a>(req: HttpRequest, data: web::Data<AppState>, info: web::Path<(uuid::Uuid)>, form: web::Form<PaymentStateData>) -> impl Future<Item=impl actix_web::Responder, Error=actix_web::error::Error> + 'a {
    render_payment(req, data, info, Some(form), "fb_payment.html")
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

fn process_worldpay_payment<'a>(req: HttpRequest, data: web::Data<AppState>, info: web::Path<(uuid::Uuid)>, session: actix_session::Session, payment_data: web::Json<WorldpayPaymentData>) -> impl Future<Item=impl actix_web::Responder, Error=actix_web::error::Error> + 'a {
    let sess_id = match match session.get::<uuid::Uuid>("sess_id") {
        Ok(s) => s,
        Err(e) => return Either::A(err(actix_web::error::ErrorInternalServerError(e)))
    } {
        Some(s) => s,
        None => {
            let s = uuid::Uuid::new_v4();
            match session.set("sess_id", s) {
                Ok(_) => s,
                Err(e) => return Either::A(err(actix_web::error::ErrorInternalServerError(e)))
            }
        }
    };

    Either::B(data.db.send(db::GetPayment::new(&info.into_inner()))
        .from_err()
        .and_then(|res| match res {
            Ok(payment) => Ok((payment, data)),
            Err(e) => match (e, payment_data.payment) {
                (diesel::result::Error::NotFound, Some(payment)) => {
                    data.db.send(db::GetPaymentTokens::new())
                        .from_err()
                        .and_then(|res| match res {
                            Ok(tokens) => {
                                let items: Vec<db::CreatePaymentItem> = vec![];
                                for i in payment.items.into_iter() {
                                    let digest = crypto::sha2::Sha512::new();
                                    let hmac_data = format!("{}{}{}{}{}", i.item_type, i.item_data, i.title, i.quantity, i.price);
                                    let sig = crypto::mac::MacResult::new(&i.sig);

                                    let mut validated = false;
                                    for token in tokens {
                                        let hmac = crypto::hmac::Hmac::new(digest, &token).result();

                                        if hmac == sig {
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

                                let payment = db::CreatePayment::new(
                                    &info.into_inner(),
                                    &Utc::now().naive_utc(),
                                    models::PaymentState::OPEN,
                                    payment.environment,
                                    &payment.customer_id,
                                    &items,
                                );

//                    data.db.send(payment)
//                        .from_err()
//                        .and_then(|res| match res {
//                            Ok(payment) => Ok((payment, data)),
//                            Err(e) => Err(actix_web::error::ErrorInternalServerError(e)),
//                        })
                            }
                            Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
                        })
                }
                (_, _) => Err(actix_web::error::ErrorNotFound(e))
            }
        })
        .and_then(|(payment, data)| {
            data.db.send(db::GetPaymentItems::new(&payment))
                .from_err()
                .and_then(|res| match res {
                    Ok(items) => Ok(items),
                    Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
                })
                .map(|items| (payment, items, data))
        })
        .and_then(|(payment, items, data)| {
            data.oauth.clone().get_access_token()
                .map(|token| (payment, items, token, data))
        })
        .and_then(|(payment, items, token, data)| {
            data.keycloak.clone().get_user(payment.customer_id, &token)
                .map(|user| (payment, items, token, user, data))
        })
        .and_then(|(payment, items, token, user, data)| {
            user.add_role(&["customer"], token.clone())
                .map(|user| (payment, items, token, user, data))
        })
        .and_then(move |(payment, items, token, mut user, data)| {
            if let None = user.email {
                user.email = Some(payment_data.email.clone());
            }
            if let None = user.first_name {
                user.first_name = Some(payment_data.payer_name.clone());
            }
            if !user.has_attribute("phone") {
                user.set_attribute("phone", &payment_data.billing_address.phone);
            }

            user.update(&token)
                .and_then(move |_| {
                    let billing_address = WorldpayBillingAddress::from(&payment_data.billing_address);

                    let description: String = items.iter().map(|i| i.title.clone()).collect::<Vec<String>>().join(", ");
                    let total = items.iter().map(|i| i.price.0 * i.quantity as i64).fold(0, |acc, i| acc + i);

                    println!("{}", payment_data.accepts);

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
                    data.db.send(db::CreateCardToken::new(
                        &payment.customer_id,
                        &payment_data.token,
                    ))
                        .from_err()
                        .and_then(move |_| {
                            let worldpay_token = match payment.environment {
                                models::PaymentEnvironment::LIVE => &data.worldpay.live_key,
                                models::PaymentEnvironment::TEST => &data.worldpay.test_key,
                            };

                            reqwest::r#async::Client::new().post("https://api.worldpay.com/v1/orders")
                                .header(reqwest::header::AUTHORIZATION, worldpay_token)
                                .json(&order_data)
                                .send()
                                .and_then(|c| c.error_for_status())
                                .and_then(|mut c| c.json::<WorldpayOrderResp>())
                                .map_err(|e| actix_web::error::ErrorInternalServerError(e))
                                .and_then(move |r| {
                                    match r.payment_status {
                                        WorldpayOrderStatus::Success | WorldpayOrderStatus::Authorized => {
                                            Either::A(Either::A(
                                                data.db.send(db::UpdatePaymentState::new(
                                                    &payment.id,
                                                    models::PaymentState::PAID,
                                                    Some(&format!("{} {}", r.payment_response.card_issuer, r.payment_response.masked_card_number)),
                                                ))
                                                    .from_err()
                                                    .map(|_| {
                                                        HttpResponse::Ok().json(WorldpayPaymentDataResp {
                                                            state: WorldpayPaymentStatus::SUCCESS,
                                                            frame: None,
                                                        })
                                                    })
                                            ))
                                        }
                                        WorldpayOrderStatus::PreAuthorized => {
                                            Either::A(Either::B(
                                                data.db.send(db::CreateThreedsData::new(
                                                    &payment.id,
                                                    &r.one_time_3ds_token.unwrap(),
                                                    &r.redirect_url.unwrap(),
                                                    &r.order_code,
                                                ))
                                                    .from_err()
                                                    .map(move |_| {
                                                        HttpResponse::Ok().json(WorldpayPaymentDataResp {
                                                            state: WorldpayPaymentStatus::THREEDS,
                                                            frame: Some(format!("https://{}/payment/3ds/{}/", req.connection_info().host(), payment.id)),
                                                        })
                                                    }))
                                            )
                                        }
                                        WorldpayOrderStatus::Failed => {
                                            Either::B(ok(HttpResponse::Ok().json(WorldpayPaymentDataResp {
                                                state: WorldpayPaymentStatus::FAILED,
                                                frame: None,
                                            })))
                                        }
                                        _ => {
                                            Either::B(ok(HttpResponse::Ok().json(WorldpayPaymentDataResp {
                                                state: WorldpayPaymentStatus::UNKNOWN,
                                                frame: None,
                                            })))
                                        }
                                    }
                                })
                        })
                })
        }))
}

fn render_3ds_form<'a>(req: HttpRequest, data: web::Data<AppState>, info: web::Path<(uuid::Uuid)>) -> impl Future<Item=impl actix_web::Responder, Error=actix_web::error::Error> + 'a {
    data.db.send(db::GetPayment::new(&info.into_inner()))
        .from_err()
        .and_then(|res| match res {
            Ok(payment) => Ok(payment),
            Err(e) => Err(actix_web::error::ErrorNotFound(e))
        })
        .and_then(move |payment|
            data.db.send(db::GetThreedsData::new(&payment))
                .from_err()
                .and_then(|res| match res {
                    Ok(threeds_data) => Ok((threeds_data, payment)),
                    Err(e) => Err(actix_web::error::ErrorNotFound(e))
                })
        )
        .and_then(move |(threeds_data, payment)| {
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
        })
}

#[derive(Clone, Debug, Deserialize)]
struct ThreedsData {
    #[serde(rename = "MD")]
    order_id: String,
    #[serde(rename = "PaRes")]
    response_code: String,
}

fn render_3ds_complete<'a>(req: HttpRequest, data: web::Data<AppState>, info: web::Path<(uuid::Uuid)>, session: actix_session::Session, form: web::Form<ThreedsData>) -> impl Future<Item=impl actix_web::Responder, Error=actix_web::error::Error> + 'a {
    let sess_id = match match session.get::<uuid::Uuid>("sess_id") {
        Ok(s) => s,
        Err(e) => return Either::A(err(actix_web::error::ErrorInternalServerError(e)))
    } {
        Some(s) => s.to_string(),
        None => "".to_string()
    };

    Either::B(data.db.send(db::GetPayment::new(&info.into_inner()))
        .from_err()
        .and_then(|res| match res {
            Ok(payment) => Ok(payment),
            Err(e) => Err(actix_web::error::ErrorNotFound(e))
        })
        .and_then(move |payment|
            data.db.send(db::DeleteThreedsData::new(&payment))
                .from_err()
                .and_then(|res| match res {
                    Ok(_) => Ok((payment, data)),
                    Err(e) => Err(actix_web::error::ErrorNotFound(e))
                })
        )
        .and_then(move |(payment, data)| {
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
            let mut context_2 = context.clone();

            reqwest::r#async::Client::new().put(reqwest::Url::parse(&format!("https://api.worldpay.com/v1/orders/{}", form.order_id)).unwrap())
                .header(reqwest::header::AUTHORIZATION, worldpay_token)
                .json(&order_data)
                .send()
                .and_then(|c| c.error_for_status())
                .and_then(|mut c| c.json::<WorldpayOrderResp>())
                .map_err(|e| actix_web::error::ErrorInternalServerError(e))
                .and_then(move |r| {
                    match r.payment_status {
                        WorldpayOrderStatus::Success | WorldpayOrderStatus::Authorized => {
                            Either::B(data.db.send(db::UpdatePaymentState::new(
                                &payment.id,
                                models::PaymentState::PAID,
                                Some(&format!("{} {}", r.payment_response.card_issuer, r.payment_response.masked_card_number)),
                            ))
                                .from_err()
                                .map(move |_| {
                                    context.insert("threeds_approved", &true);
                                    match TERA.render("3ds_complete.html", &context) {
                                        Ok(r) => Ok(HttpResponse::Ok().body(r)),
                                        Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
                                    }
                                }))
                        }
                        _ => Either::A(err(actix_web::error::ErrorInternalServerError("")))
                    }
                })
                .then(move |r| {
                    match r {
                        Ok(r) => r,
                        Err(_) => {
                            context_2.insert("threeds_approved", &false);
                            match TERA.render("3ds_complete.html", &context_2) {
                                Ok(r) => Ok(HttpResponse::Ok().body(r)),
                                Err(e) => Err(actix_web::error::ErrorInternalServerError(e).into())
                            }
                        }
                    }
                })
        }))
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
    let worldpay_config = worldpay_config();

    let data = AppState {
        oauth: oauth_client,
        keycloak: keycloak_client,
        worldpay: worldpay_config,
        db: db_addr,
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
            .route("/payment/new/", web::post().to_async(new_payment))
            .service(
                web::resource("/payment/{payment_id}/")
                    .wrap(Cors::new()
                        .supports_credentials())
                    .route(web::get().to_async(get_payment))
            )
            .service(
                web::resource("/payment/worldpay/{payment_id}/")
                    .wrap(Cors::new()
                        .supports_credentials())
                    .route(web::post().to_async(process_worldpay_payment))
            )
            .route("/payment/3ds/{payment_id}/", web::get().to_async(render_3ds_form))
            .route("/payment/3ds-complete/{payment_id}/", web::post().to_async(render_3ds_complete))
            .service(web::resource("/payment/fb/{payment_id}/")
                .route(web::get().to_async(render_fb_payment_get))
                .route(web::post().to_async(render_fb_payment_post)))
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

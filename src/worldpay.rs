use actix_web::{HttpRequest, HttpResponse, web};
use chrono::prelude::*;
use crypto::mac::Mac;
use futures::compat::Future01CompatExt;
use encoding::types::Encoding;

use crate::db;
use crate::jobs;
use crate::models;
use crate::util;

#[derive(Clone, Debug, Deserialize)]
pub struct BillingAddressData {
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
pub struct CardData {
    name: String,
    exp_month: u32,
    exp_year: u32,
    card_number: String,
    cvc: String,
}

#[derive(Clone, Deserialize)]
pub struct WorldpayPaymentData {
    accepts: String,
    email: Option<String>,
    phone: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    card: CardData,
    payment: Option<WorldpayNewPaymentData>,
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
    #[serde(rename = "paymentMethod")]
    payment_method: WorldpayCard,
}

#[derive(Clone, Debug, Serialize)]
struct WorldpayCard {
    name: String,
    #[serde(rename = "expiryMonth")]
    exp_month: u32,
    #[serde(rename = "expiryYear")]
    exp_year: u32,
    #[serde(rename = "cardNumber")]
    pan: String,
    #[serde(rename = "type")]
    _type: String,
    cvc: Option<String>
}

impl WorldpayCard {
    fn new(name: &str, pan: &str, exp_month: u32, exp_year: u32, cvc: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            exp_month,
            exp_year,
            pan: pan.to_string(),
            _type: "Card".to_string(),
            cvc: match cvc {
                Some(s) => Some(s.to_string()),
                None => None
            }
        }
    }
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

pub async fn process_worldpay_payment(req: HttpRequest, data: web::Data<crate::config::AppState>, info: web::Path<uuid::Uuid>, session: actix_session::Session, payment_data: web::Json<WorldpayPaymentData>) -> actix_web::Result<impl actix_web::Responder> {
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

                let user_id = match util::user_id_from_session(&session, &data.oauth).await? {
                    Some(u) => u,
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
        user.email = payment_data.email.clone();
    }
    if let None = user.first_name {
        user.first_name = payment_data.first_name.clone()
    }
    if let None = user.last_name {
        user.first_name = payment_data.first_name.clone()
    }
    if !user.has_attribute("phone") {
        user.set_attribute("phone", &payment_data.billing_address.phone);
    }
    user.update(&token).await?;

    let billing_address = WorldpayBillingAddress::from(&payment_data.billing_address);

    let description: String = items.iter().map(|i| i.title.clone()).collect::<Vec<String>>().join(", ");
    let total = items.iter().map(|i| i.price.0 * i.quantity as i64).fold(0, |acc, i| acc + i);
    let name = format!("{} {}", user.first_name.unwrap_or("".to_string()), user.last_name.unwrap_or("".to_string()));

    let order_data = WorldpayOrder {
        order_type: "ECOM".to_string(),
        order_description: description,
        customer_order_code: payment.id.to_string(),
        amount: total,
        currency_code: "GBP".to_string(),
        name: match encoding::all::ISO_8859_1.encode(&name, encoding::EncoderTrap::Ignore) {
            Ok(s) => match String::from_utf8(s) {
                Ok(s) => s,
                Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
            },
            Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
        },
        shopper_email_address: user.email.unwrap_or("".to_string()),
        billing_address,
        shopper_ip_address: req.connection_info().remote().unwrap_or("").to_string(),
        shopper_user_agent: req.headers().get(actix_web::http::header::USER_AGENT)
            .unwrap_or(&actix_web::http::header::HeaderValue::from_static("")).to_str()
            .unwrap_or("").to_string(),
        shopper_accept_header: payment_data.accepts.clone(),
        shopper_session_id: sess_id.to_string(),
        is_3ds_order: true,
        authorize_only: total == 0,
        payment_method: WorldpayCard::new(
            &payment_data.card.name,
            &payment_data.card.card_number,
            payment_data.card.exp_month,
            payment_data.card.exp_year,
            Some(&payment_data.card.cvc),
        )
    };

    match match data.db.send(db::CreateCard::new(
        &uuid::Uuid::new_v4(),
        &payment.customer_id,
        &payment_data.card.card_number,
        payment_data.card.exp_month,
        payment_data.card.exp_year,
        &payment_data.card.name,
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

pub async fn render_3ds_form<'a>(req: HttpRequest, data: web::Data<crate::config::AppState>, info: web::Path<uuid::Uuid>) -> actix_web::Result<impl actix_web::Responder> {
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

    match crate::TERA.render("3ds_form.html", &context) {
        Ok(r) => Ok(HttpResponse::Ok().body(r)),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ThreedsData {
    #[serde(rename = "MD")]
    order_id: String,
    #[serde(rename = "PaRes")]
    response_code: String,
}

pub async fn render_3ds_complete<'a>(req: HttpRequest, data: web::Data<crate::config::AppState>, info: web::Path<uuid::Uuid>, session: actix_session::Session, form: web::Form<ThreedsData>) -> actix_web::Result<impl actix_web::Responder> {
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

    match crate::TERA.render("3ds_complete.html", &context) {
        Ok(r) => Ok(HttpResponse::Ok().body(r)),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e).into())
    }
}
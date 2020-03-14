use actix_web::{HttpRequest, HttpResponse, web};
use futures::compat::Future01CompatExt;
use chrono::prelude::*;
use crate::db;

#[derive(Clone, Debug, Deserialize)]
struct NewPaymentItemData {
    item_type: String,
    item_data: serde_json::Value,
    title: String,
    quantity: i32,
    price: rust_decimal::Decimal,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewPaymentData {
    environment: crate::models::PaymentEnvironment,
    customer_id: uuid::Uuid,
    items: Vec<NewPaymentItemData>,
}

#[derive(Clone, Debug, Serialize)]
struct NewPaymentResponseData {
    id: uuid::Uuid,
}

pub async fn new_payment(token: crate::oauth::BearerAuthToken, data: web::Data<crate::config::AppState>, new_payment: web::Json<NewPaymentData>) -> actix_web::Result<impl actix_web::Responder> {
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
        crate::models::PaymentState::OPEN,
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
    quantity: i32,
}

#[derive(Clone, Debug, Serialize)]
struct PaymentCustomerResponseData {
    id: uuid::Uuid,
    email: Option<String>,
    request_name: bool,
    request_email: bool,
    request_phone: bool,
}

#[derive(Clone, Debug, Serialize)]
struct PaymentResponseData {
    id: uuid::Uuid,
    timestamp: DateTime<Utc>,
    state: crate::models::PaymentState,
    environment: crate::models::PaymentEnvironment,
    customer: PaymentCustomerResponseData,
    items: Vec<PaymentItemResponseData>,
    payment_method: Option<String>,
}

pub async fn get_payment<'a>(token: crate::oauth::OptionalBearerAuthToken, data: web::Data<crate::config::AppState>, info: web::Path<uuid::Uuid>, session: actix_session::Session) -> actix_web::Result<impl actix_web::Responder> {
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

    if let Some(t) = token.token() {
        data.oauth.verify_token(t, "view-payments").await?;
    } else {
        let user_id = match crate::util::user_id_from_session(&session, &data.oauth).await? {
            Some(u) => u,
            None => return Err(actix_web::error::ErrorUnauthorized(""))
        };

        if payment.customer_id != user_id {
            return Err(actix_web::error::ErrorForbidden(""))
        }
    }
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
        payment_method: payment.payment_method,
        customer: PaymentCustomerResponseData {
            id: user.id,
            request_name: user.first_name.is_none() || user.last_name.is_none(),
            email: user.email.clone(),
            request_email: user.email.is_none(),
            request_phone: !user.has_attribute("phone"),
        },
        items: items.into_iter()
            .map(|item| PaymentItemResponseData {
                id: item.id,
                item_type: item.item_type,
                item_data: item.item_data,
                title: item.title,
                price: (item.price.0 as f64) / 100.0,
                quantity: item.quantity,
            })
            .collect(),
    };

    Ok(HttpResponse::Ok().json(response_data))
}

async fn render_payment(req: HttpRequest, data: web::Data<crate::config::AppState>, info: web::Path<uuid::Uuid>, query: web::Query<crate::login_views::LoginKey>, session: actix_session::Session, template_name: &str) -> actix_web::Result<impl actix_web::Responder> {
    let user_id = match crate::util::user_id_from_session(&session, &data.oauth).await? {
        Some(u) => u,
        None => {
            let mut params: Vec<(&str, String)> = vec![
                ("next", req.uri().to_string()),
            ];
            if let Some(key) = &query.key {
                params.push(("key", key.to_string()))
            }
            let url = format!("https://{}/login/auth/?{}", req.connection_info().host(), serde_urlencoded::to_string(&params).unwrap());

            return Ok(
                HttpResponse::Found()
                    .header(actix_web::http::header::LOCATION, url)
                    .finish()
            )
        }
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

    let is_users_payment = payment.customer_id == user_id;
    let is_open_payment = payment.state == crate::models::PaymentState::OPEN;
    let is_test = payment.environment != crate::models::PaymentEnvironment::LIVE;
    let accepts = match req.headers().get(actix_web::http::header::ACCEPT) {
        Some(a) => match a.to_str() {
            Ok(a) => a,
            Err(e) => return Err(actix_web::error::ErrorBadRequest(e))
        },
        None => "*/*"
    };

    let mut context = tera::Context::new();
    context.insert("payment_id", &payment.id);
    context.insert("logout_url", &format!("/login/logout/?{}", serde_urlencoded::to_string(&[("next", req.uri().to_string())]).unwrap()));
    context.insert("is_users_payment", &is_users_payment);
    context.insert("is_open_payment", &is_open_payment);
    context.insert("test", &is_test);
    context.insert("accepts_header", &accepts);

    match crate::TERA.render(template_name, &context) {
        Ok(r) => Ok(HttpResponse::Ok().body(r)),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
    }
}

pub async fn render_fb_payment(req: HttpRequest, data: web::Data<crate::config::AppState>, info: web::Path<uuid::Uuid>, query: web::Query<crate::login_views::LoginKey>, session: actix_session::Session) -> actix_web::Result<impl actix_web::Responder> {
    render_payment(req, data, info, query, session,"fb_payment.html").await
}

pub async fn render_login_complete<'a>(data: web::Data<crate::config::AppState>, session: actix_session::Session) -> actix_web::Result<impl actix_web::Responder> {
    let mut context = tera::Context::new();

    match {
        match session.get::<crate::oauth::OAuthToken>("oauth_token") {
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

    match crate::TERA.render("login_complete.html", &context) {
        Ok(r) => Ok(HttpResponse::Ok().body(r)),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
    }
}
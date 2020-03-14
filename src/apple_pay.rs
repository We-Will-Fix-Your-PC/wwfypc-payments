use actix_web::{HttpRequest, HttpResponse, web};
use futures::compat::Future01CompatExt;

#[derive(Clone, Debug, Deserialize)]
pub struct MerchantVerificationData {
    url: String,
}

#[derive(Clone, Debug, Serialize)]
struct MerchantVerificationPostData {
    #[serde(rename = "merchantIdentifier")]
    merchant_identifier: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "domainName")]
    domain_name: String,
}

#[derive(Clone, Debug, Serialize)]
struct MerchantVerificationResponseData {
    verification: serde_json::Value
}

pub async fn merchant_verification(req: HttpRequest, state: web::Data<crate::config::AppState>, data: web::Json<MerchantVerificationData>) -> failure::Fallible<impl actix_web::Responder> {
    let url = reqwest::Url::parse(&data.url)?;
    let host_str = url.host_str().unwrap_or("");
    if !host_str.ends_with(".apple.com") {
        return Ok(
            HttpResponse::BadRequest().finish()
        )
    }
    let mut resp = crate::util::async_reqwest_to_error(state.apple_pay_client.post(&data.url)
        .json(&MerchantVerificationPostData {
            merchant_identifier: "merchant.uk.cardifftec".to_string(),
            display_name: "We Will Fix Your PC".to_string(),
            domain_name: match req.headers().get(actix_web::http::header::ORIGIN) {
                Some(h) => h.to_str().unwrap_or("payments.cardifftec.uk"),
                None => "payments.cardifftec.uk"
            }.to_string().replace("https://", "").replace("http://", ""),
        })).await?;

    let data = resp.json().compat().await?;

    Ok(
        HttpResponse::Ok().json(&MerchantVerificationResponseData {
            verification: data
        })
    )
}
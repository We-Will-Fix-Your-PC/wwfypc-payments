use actix_web::{HttpRequest, HttpResponse, web};

#[derive(Serialize, Deserialize)]
struct OauthState {
    id: uuid::Uuid,
    redirect_uri: String,
    next_uri: Option<String>,
}

#[derive(Deserialize)]
pub struct OauthLoginInfo {
    next: Option<String>,
}


#[derive(Deserialize)]
pub struct OauthCallbackInfo {
    state: uuid::Uuid,
    code: String,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginKey {
    pub key: Option<String>
}

pub async fn start_login(req: HttpRequest, data: web::Data<crate::config::AppState>, info: web::Query<OauthLoginInfo>, query: web::Query<LoginKey>, session: actix_session::Session) -> actix_web::Result<impl actix_web::Responder> {
    let redirect_uri = format!("https://{}/login/redirect/", req.connection_info().host());

    let state = OauthState {
        id: uuid::Uuid::new_v4(),
        redirect_uri: redirect_uri.clone(),
        next_uri: info.next.clone(),
    };

    let id_str = state.id.to_string();

    let mut additional = vec![];
    if let Some(key) = &query.key {
        additional.push(("key", key.as_str()))
    }

    let url = data.oauth.authorization_url(
        &[
            "openid",
            "email",
            "profile"
        ],
        "code",
        Some(&id_str),
        Some(&redirect_uri),
        Some(additional.as_slice())
    ).await?;

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

pub async fn start_logout(req: HttpRequest, data: web::Data<crate::config::AppState>, info: web::Query<OauthLoginInfo>, session: actix_session::Session) -> actix_web::Result<impl actix_web::Responder> {
    let next_uri = info.next.as_ref().map(|x| &**x).unwrap_or("/");

    match session.get::<crate::oauth::OAuthToken>("oauth_token") {
        Ok(s) => match s {
            Some(oauth_token) => {
                session.remove("oauth_token");

                let redirect_uri = format!("https://{}{}", req.connection_info().host(), next_uri);
                let url = data.oauth.logout_url(oauth_token.id_token.as_ref().map(|x| &**x), Some(&redirect_uri)).await?;

                return Ok(
                    HttpResponse::Found()
                        .header(actix_web::http::header::LOCATION, url)
                        .finish()
                );
            }
            None => {}
        },
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    };

    Ok(HttpResponse::Found().header(actix_web::http::header::LOCATION, next_uri).finish())
}

pub async fn login_callback(data: web::Data<crate::config::AppState>, info: web::Query<OauthCallbackInfo>, session: actix_session::Session) -> actix_web::Result<impl actix_web::Responder> {
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
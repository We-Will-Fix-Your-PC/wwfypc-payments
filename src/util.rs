pub async fn async_reqwest_to_error(request: reqwest::RequestBuilder) -> failure::Fallible<reqwest::Response> {
    let c = request.send().await?;
    if c.status().is_client_error() || c.status().is_server_error() {
        let err = Err(c.error_for_status_ref().unwrap_err().into());
        debug!("Got response with status code {} with body {:?}", c.status(), c.text().await);
        err
    } else {
        Ok(c)
    }
}

pub async fn user_token_from_session(session: &actix_session::Session, oauth_client: &crate::oauth::OAuthClient) -> actix_web::Result<Option<(crate::oauth::OAuthTokenIntrospect, crate::oauth::OAuthToken)>> {
    match session.get::<crate::oauth::OAuthToken>("oauth_token") {
        Ok(s) => match s {
            Some(oauth_token) => {
                let (introspect, oauth_token) = oauth_client.update_and_verify_token(oauth_token, None).await?;
                match session.set("oauth_token", oauth_token.clone()) {
                    Ok(_) => {}
                    Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
                }
                return Ok(Some((introspect, oauth_token)))
            }
            None => Ok(None)
        },
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
    }
}

pub async fn user_id_from_session(session: &actix_session::Session, oauth_client: &crate::oauth::OAuthClient) -> actix_web::Result<Option<uuid::Uuid>> {
    let (introspect, _oauth_token) = match user_token_from_session(session, oauth_client).await? {
        Some(d) => d,
        None => return Ok(None)
    };

    match match introspect.sub {
        Some(u) => uuid::Uuid::parse_str(&u),
        None => return Err(actix_web::error::ErrorInternalServerError(""))
    } {
        Ok(u) => Ok(Some(u)),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
    }
}
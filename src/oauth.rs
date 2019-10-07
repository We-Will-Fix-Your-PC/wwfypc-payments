use std::sync::Arc;
use futures::lock::RwLock;
use futures::prelude::*;
use chrono::prelude::*;
use futures::future::{ok, err, Either};
use std::collections::HashMap;

#[derive(Clone)]
pub struct OAuthClientConfig {
    client_id: String,
    client_secret: String,
    well_known_url: reqwest::Url,
}

impl OAuthClientConfig {
    pub fn new(client_id: &str, client_secret: &str, well_known_url: &str) -> Result<Self, reqwest::UrlError> {
        Ok(Self {
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
            well_known_url: reqwest::Url::parse(well_known_url)?,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct OAuthWellKnown {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: Option<String>,
    introspection_endpoint: Option<String>,
    jwks_uri: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthTokenIntrospectAccess {
    roles: Vec<String>
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthTokenIntrospect {
    active: bool,
    scope: Option<String>,
    client_id: Option<String>,
    username: Option<String>,
    token_type: Option<String>,
    exp: Option<i64>,
    iat: Option<i64>,
    nbf: Option<i64>,
    sub: Option<String>,
    aud: Option<Vec<String>>,
    iss: Option<String>,
    jti: Option<String>,
    realm_access: Option<OAuthTokenIntrospectAccess>,
    resource_access: Option<HashMap<String, OAuthTokenIntrospectAccess>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i64,
    refresh_token: Option<String>,
    refresh_expires_in: Option<i64>,
    scopes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthToken {
    access_token: String,
    expires_at: DateTime<Utc>,
    refresh_token: Option<String>,
    refresh_expires_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct OAuthTokenGrantForm {
    client_id: String,
    client_secret: String,
    grant_type: String,
}

#[derive(Serialize)]
struct OAuthTokenRefreshGrantForm {
    #[serde(flatten)]
    grant: OAuthTokenGrantForm,
    refresh_token: String,
}

#[derive(Serialize)]
struct OAuthTokenIntrospectForm {
    client_id: String,
    client_secret: String,
    token: String,
}

#[derive(Clone)]
pub struct OAuthClient {
    config: OAuthClientConfig,
    client: reqwest::r#async::Client,
    _well_known: Arc<RwLock<Option<OAuthWellKnown>>>,
    _access_token: Arc<RwLock<Option<OAuthToken>>>,
}

impl OAuthClient {
    pub fn new(config: OAuthClientConfig) -> Self {
        Self {
            config,
            client: reqwest::r#async::Client::new(),
            _well_known: Arc::new(RwLock::new(None)),
            _access_token: Arc::new(RwLock::new(None)),
        }
    }

    fn well_known<'a>(self) -> impl Future<Item=OAuthWellKnown, Error=actix_web::error::Error> + 'a {
        {
            if let Some(well_known) = self._well_known.read().unwrap().clone() {
                return Either::A(ok(well_known));
            }
        }

        Either::B(
            self.client.get(self.config.well_known_url.clone())
                .send()
                .and_then(|mut c| {
                    c.json::<OAuthWellKnown>()
                })
                .map(move |d| {
                    *self._well_known.write().unwrap() = Some(d.clone());
                    d
                })
                .map_err(|e| actix_web::error::ErrorInternalServerError(e))
        )
    }

    pub fn get_access_token<'a>(self) -> impl Future<Item=String, Error=actix_web::Error> + 'a {
        let client = self.client.clone();
        let config = self.config.clone();
        let now = Utc::now();

        {
            if let Some(access_token) = self._access_token.clone().read().unwrap().clone() {
                if access_token.expires_at > now {
                    return Either::A(Either::A(ok(access_token.access_token)));
                } else if let Some(refresh_expires_at) = access_token.refresh_expires_at {
                    if let Some(refresh_token) = access_token.refresh_token {
                        if refresh_expires_at > now {
                            return Either::A(Either::B(self.clone().well_known()
                                .and_then(move |w| {
                                    match w.token_endpoint {
                                        Some(e) => Either::A(match reqwest::Url::parse(&e) {
                                            Ok(u) => {
                                                let grant = OAuthTokenGrantForm {
                                                    client_id: config.client_id.clone(),
                                                    client_secret: config.client_secret.clone(),
                                                    grant_type: "refresh_token".to_string(),
                                                };

                                                let form = OAuthTokenRefreshGrantForm {
                                                    grant,
                                                    refresh_token,
                                                };


                                                Either::A(
                                                    client.post(u)
                                                        .form(&form)
                                                        .send()
                                                        .and_then(|mut c| c.json::<OAuthTokenResponse>())
                                                        .map_err(|e| actix_web::error::ErrorInternalServerError(e))
                                                        .map(move |t| {
                                                            *self._access_token.write().unwrap() = Some(OAuthToken {
                                                                access_token: t.access_token.clone(),
                                                                expires_at: now + chrono::Duration::seconds(t.expires_in),
                                                                refresh_token: t.refresh_token.clone(),
                                                                refresh_expires_at: match t.refresh_expires_in {
                                                                    Some(e) => Some(now + chrono::Duration::seconds(e)),
                                                                    None => None
                                                                },
                                                            });
                                                            t.access_token
                                                        })
                                                )
                                            }
                                            Err(e) => Either::B(err(actix_web::error::ErrorInternalServerError(e)))
                                        }),
                                        None => Either::B(err(actix_web::error::ErrorInternalServerError("no token endpoint")))
                                    }
                                })));
                        }
                    }
                }
            }
        }

        Either::B(self.clone().well_known()
            .and_then(move |w| {
                match w.token_endpoint {
                    Some(e) => Either::A(match reqwest::Url::parse(&e) {
                        Ok(u) => {
                            let form = OAuthTokenGrantForm {
                                client_id: config.client_id.clone(),
                                client_secret: config.client_secret.clone(),
                                grant_type: "client_credentials".to_string(),
                            };

                            Either::A(
                                client.post(u)
                                    .form(&form)
                                    .send()
                                    .and_then(|mut c| c.json::<OAuthTokenResponse>())
                                    .map_err(|e| actix_web::error::ErrorInternalServerError(e))
                                    .map(move |t| {
                                        *self._access_token.write().unwrap() = Some(OAuthToken {
                                            access_token: t.access_token.clone(),
                                            expires_at: now + chrono::Duration::seconds(t.expires_in),
                                            refresh_token: t.refresh_token.clone(),
                                            refresh_expires_at: match t.refresh_expires_in {
                                                Some(e) => Some(now + chrono::Duration::seconds(e)),
                                                None => None
                                            },
                                        });
                                        t.access_token
                                    })
                            )
                        }
                        Err(e) => Either::B(err(actix_web::error::ErrorInternalServerError(e)))
                    }),
                    None => Either::B(err(actix_web::error::ErrorInternalServerError("no token endpoint")))
                }
            }))
    }

    pub async fn introspect_token(self, token: &str) {
        let w = self.well_known().await?;
        match w.introspection_endpoint {
            Some(e) => match reqwest::Url::parse(&e) {
                Ok(u) => {
                    let form = OAuthTokenIntrospectForm {
                        client_id: self.config.client_id,
                        client_secret: self.config.client_secret,
                        token: self.token,
                    };

                    let mut c = client.post(u).form(&form).send().await?;
                    c.json::<OAuthTokenIntrospect>().await
                }
                Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
            },
            None => Err(actix_web::error::ErrorInternalServerError("no introspection endpoint"))
        }
    }

    pub async fn verify_token(self, token: &str, role: &str) {
        let i = self.clone().introspect_token(token).await?;
        match (&i.aud, &i.resource_access) {
            (Some(aud), Some(resource_access)) => {
                if !aud.contains(&self.config.client_id) ||
                    !resource_access.contains_key(&self.config.client_id) ||
                    !resource_access.get(&self.config.client_id).unwrap().roles.contains(&role.to_owned()) {
                    return Err(actix_web::error::ErrorForbidden(""));
                }

                Ok(i)
            }
            _ => Err(actix_web::error::ErrorForbidden(""))
        }
    }
}

#[derive(Debug)]
pub struct BearerAuthToken {
    token: String
}

impl BearerAuthToken {
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl actix_web::FromRequest for BearerAuthToken {
    type Error = actix_web::HttpResponse;
    type Future = Result<Self, Self::Error>;
    type Config = ();

    fn from_request(req: &actix_web::HttpRequest, _payload: &mut actix_web::dev::Payload) -> Self::Future {
        let auth_header = req.headers().get(actix_web::http::header::AUTHORIZATION);
        if let Some(auth_token) = auth_header {
            if let Ok(auth_token_str) = auth_token.to_str() {
                let auth_token_str = auth_token_str.trim();
                if auth_token_str.starts_with("Bearer ") {
                    Ok(Self {
                        token: auth_token_str[7..].to_owned()
                    })
                } else {
                    Err(actix_web::HttpResponse::new(http::StatusCode::UNAUTHORIZED))
                }
            } else {
                Err(actix_web::HttpResponse::new(http::StatusCode::UNAUTHORIZED))
            }
        } else {
            Err(actix_web::HttpResponse::new(http::StatusCode::UNAUTHORIZED))
        }
    }
}
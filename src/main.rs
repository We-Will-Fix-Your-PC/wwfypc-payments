#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;
extern crate dotenv;
#[macro_use]
extern crate log;
extern crate pretty_env_logger;
extern crate actix_web;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate tera;
extern crate reqwest;
#[macro_use]
extern crate serde;
extern crate serde_json;
extern crate futures;
extern crate http;
extern crate rust_decimal;
#[macro_use]
extern crate diesel_derive_enum;

pub mod schema;
pub mod models;

use diesel::prelude::*;
use diesel::pg::PgConnection;
use dotenv::dotenv;
use std::env;
use std::sync::{Arc, RwLock};
use actix_web::{web, middleware, App, HttpServer};
use tera::Tera;
use futures::prelude::*;
use futures::future::{ok, err, Either};
use std::collections::HashMap;


embed_migrations!("./migrations");

lazy_static! {
    pub static ref TERA: Tera = {
        let mut tera = compile_templates!("templates/**/*");
        // and we can add more things to our instance if we want to
        tera.autoescape_on(vec!["html", ".sql"]);
        tera
    };
}

pub fn establish_connection() -> PgConnection {
    dotenv().ok();

    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    PgConnection::establish(&database_url)
        .expect(&format!("Error connecting to {}", database_url))
}

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
}

impl OAuthClient {
    pub fn new(config: OAuthClientConfig) -> Self {
        Self {
            config,
            client: reqwest::r#async::Client::new(),
            _well_known: Arc::new(RwLock::new(None)),
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

    pub fn introspect_token<'a>(self, token: String) -> impl Future<Item=OAuthTokenIntrospect, Error=actix_web::Error> + 'a {
        let client = self.client.clone();
        let config = self.config.clone();
        self.well_known()
            .and_then(move |w| {
                match w.introspection_endpoint {
                    Some(e) => Either::A(match reqwest::Url::parse(&e) {
                        Ok(u) => {
                            let form = OAuthTokenIntrospectForm {
                                client_id: config.client_id.clone(),
                                client_secret: config.client_secret.clone(),
                                token: token.to_owned(),
                            };

                            Either::A(
                                client.post(u)
                                    .form(&form)
                                    .send()
                                    .and_then(|mut c| c.json::<OAuthTokenIntrospect>())
                                    .map_err(|e| actix_web::error::ErrorInternalServerError(e))
                            )
                        }
                        Err(e) => Either::B(err(actix_web::error::ErrorInternalServerError(e)))
                    }),
                    None => Either::B(err(actix_web::error::ErrorInternalServerError("no introspection endpoint")))
                }
            })
    }

    pub fn verify_token<'a>(self, token: String, role: &'a str) -> impl Future<Item=OAuthTokenIntrospect, Error=actix_web::Error> + 'a {
        self.clone().introspect_token(token)
            .and_then(move |i| {
                match (&i.aud, &i.resource_access) {
                    (Some(aud), Some(resource_access)) => {
                        if !aud.contains(&self.config.client_id) ||
                            !resource_access.contains_key(&self.config.client_id) ||
                            !resource_access.get(&self.config.client_id).unwrap().roles.contains(&role.to_owned()) {
                            return err(actix_web::error::ErrorForbidden(""));
                        }

                        ok(i)
                    }
                    _ => err(actix_web::error::ErrorForbidden(""))
                }
            })
    }
}

pub fn oauth_client() -> OAuthClient {
    dotenv().ok();

    let client_id = env::var("CLIENT_ID")
        .expect("CLIENT_ID must be set");
    let client_secret = env::var("CLIENT_SECRET")
        .expect("CLIENT_SECRET must be set");
    let well_known_url = env::var("OAUTH_WELL_KNOWN")
        .unwrap_or("https://account.cardifftec.uk/auth/realms/wwfypc-dev/.well-known/openid-configuration".to_string());

    let config = OAuthClientConfig::new(&client_id, &client_secret, &well_known_url).unwrap();

    OAuthClient::new(config)
}

#[derive(Debug)]
struct BearerAuthToken {
    token: String
}

impl actix_web::FromRequest for BearerAuthToken {
    type Error = actix_web::HttpResponse;
    type Future = Result<Self, Self::Error>;
    type Config = ();

    fn from_request(req: &actix_web::HttpRequest, _payload: &mut actix_web::dev::Payload) -> Self::Future {
        let auth_header = req.headers().get("Authorization");
        if let Some(auth_token) = auth_header {
            if let Ok(auth_token_str) = auth_token.to_str() {
                let auth_token_str = auth_token_str.trim();
                if auth_token_str.starts_with("Bearer ") {
                    Ok(Self {
                        token: auth_token_str[7..].to_owned()
                    })
                } else {
                    Err(actix_web::HttpResponse::new(http::StatusCode::FORBIDDEN))
                }
            } else {
                Err(actix_web::HttpResponse::new(http::StatusCode::FORBIDDEN))
            }
        } else {
            Err(actix_web::HttpResponse::new(http::StatusCode::FORBIDDEN))
        }
    }
}

#[derive(Clone)]
struct AppState {
    oauth: OAuthClient
}


#[derive(Clone, Debug, Deserialize)]
struct NewPaymentItemData {
    item_type: String,
    item_data: String,
    title: String,
    price: rust_decimal::Decimal
}

#[derive(Clone, Debug, Deserialize)]
struct NewPaymentData {
    environment: schema::PaymentEnvironment,
    customer_id: String,
    items: Vec<NewPaymentItemData>
}

fn new_payment<'a>(token: BearerAuthToken, data: web::Data<AppState>, new_payment: web::Json<NewPaymentData>) -> impl Future<Item=impl actix_web::Responder, Error=actix_web::error::Error> + 'a {
    data.oauth.clone().verify_token(token.token, "create-payments")
        .and_then(move |i| {
            println!("{:?}", new_payment);
            actix_web::HttpResponse::Ok().body(format!("{:?}", i))
        })
}

fn main() {
    pretty_env_logger::init();

    info!("Migrating database...");
    let connection = establish_connection();
    embedded_migrations::run_with_output(&connection, &mut std::io::stdout())
        .expect("Unable to run migrations");
    info!("Migrations complete!");

    let oauth_client = oauth_client();

    let data = AppState {
        oauth: oauth_client
    };

    let mut server = HttpServer::new(move || {
        App::new()
            .data(data.clone())
            .wrap(middleware::Logger::default())
            .wrap(middleware::Compress::default())
            .route("/payments/new", web::post().to_async(new_payment))
//            .route("/again", web::get().to(index2))
    });

    let mut listenfd = listenfd::ListenFd::from_env();

    info!("Start listening...");
    server = if let Some(l) = listenfd.take_tcp_listener(0).unwrap() {
        server.listen(l).unwrap()
    } else {
        server.bind("127.0.0.1:3000").unwrap()
    };

    server.run().unwrap();
}

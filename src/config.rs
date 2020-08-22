use std::env;
use dotenv::dotenv;
use diesel::pg::PgConnection;
use actix_redis::RedisSession;
use actix::prelude::*;
use diesel::prelude::*;
use std::io::Read;

pub fn establish_connection() -> PgConnection {
    dotenv().ok();

    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    PgConnection::establish(&database_url)
        .expect(&format!("Error connecting to {}", database_url))
}

pub fn oauth_client() -> crate::oauth::OAuthClient {
    dotenv().ok();

    let client_id = env::var("CLIENT_ID")
        .expect("CLIENT_ID must be set");
    let client_secret = env::var("CLIENT_SECRET")
        .expect("CLIENT_SECRET must be set");
    let well_known_url = env::var("OAUTH_WELL_KNOWN")
        .unwrap_or("https://account.cardifftec.uk/auth/realms/wwfypc-dev/.well-known/openid-configuration".to_string());

    let config = crate::oauth::OAuthClientConfig::new(&client_id, &client_secret, &well_known_url).unwrap();

    crate::oauth::OAuthClient::new(config)
}

pub fn keycloak_client() -> crate::keycloak::KeycloakClient {
    dotenv().ok();

    let realm = env::var("KEYCLOAK_REALM")
        .unwrap_or("wwfypc-dev".to_string());
    let base_url = env::var("KEYCLOAK_BASE_URL")
        .unwrap_or("https://account.cardifftec.uk/auth/".to_string());

    let config = crate::keycloak::KeycloakClientConfig::new(&base_url, &realm).unwrap();

    crate::keycloak::KeycloakClient::new(config)
}

pub fn cookie_session() -> actix_redis::RedisSession {
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

pub fn worldpay_config() -> WorldpayConfig {
    dotenv().ok();

    WorldpayConfig {
        test_key: env::var("WORLDPAY_TEST_KEY").unwrap(),
        live_key: env::var("WORLDPAY_LIVE_KEY").unwrap(),
    }
}

pub fn mail_client() -> lettre::smtp::SmtpClient {
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

pub fn amqp_client() -> amqp::Channel {
    dotenv().ok();

    let server = env::var("AMPQ_SERVER")
        .unwrap_or("amqp://localhost//".to_string());
    let mut session = amqp::Session::open_url(&server).unwrap();
    let channel = session.open_channel(1).unwrap();
    channel
}

pub fn apple_pay_identity() -> reqwest::Client {
    dotenv().ok();
    let cert_path = env::var("APPLE_PAY_IDENTITY")
        .expect("APPLE_PAY_IDENTITY must be set");

    let mut buf = Vec::new();

    std::fs::File::open(cert_path).expect("Unable to open apple pay identity certificate")
        .read_to_end(&mut buf).expect("Unable to read apple pay identity certificate");

    let identity = reqwest::Identity::from_pkcs12_der(&buf, "").expect("Unable to decode apple pay identity certificate");

    reqwest::ClientBuilder::new()
        .identity(identity)
        .build()
        .expect("Unable to create apple pay client")
}

#[derive(Clone)]
pub struct WorldpayConfig {
    pub test_key: String,
    pub live_key: String,
}

#[derive(Clone)]
pub struct AppState {
    pub oauth: crate::oauth::OAuthClient,
    pub keycloak: crate::keycloak::KeycloakClient,
    pub worldpay: WorldpayConfig,
    pub apple_pay_client: reqwest::Client,
    pub db: Addr<crate::db::DbExecutor>,
    pub jobs_state: crate::jobs::JobsState,
}
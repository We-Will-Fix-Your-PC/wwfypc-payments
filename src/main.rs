extern crate openssl;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_derive_enum;
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

use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};

use actix::prelude::*;
use actix_cors::Cors;
use actix_web::{App, HttpResponse, HttpServer, middleware, web};
use tera::Tera;

pub mod schema;
pub mod models;
pub mod oauth;
pub mod keycloak;
pub mod db;
pub mod util;
pub mod jobs;
pub mod worldpay;
pub mod apple_pay;
pub mod config;
pub mod login_views;
pub mod payment_views;
pub mod admin_views;

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

embed_migrations!("./migrations");

lazy_static! {
    pub static ref TERA: Tera = {
        let mut tera = compile_templates!("templates/**/*");
        tera.autoescape_on(vec!["html", ".sql"]);
        tera
    };
}

fn main()  {
    openssl_probe::init_ssl_cert_env_vars();
    let _guard = sentry::init("https://e4332b5c1b9e471a88af3d05b89b46d9@o266594.ingest.sentry.io/5205059");
    sentry::integrations::panic::register_panic_handler();
    let mut log_builder = pretty_env_logger::formatted_builder();
    log_builder.parse_filters(&env::var("RUST_LOG").unwrap_or_default());
    let logger = log_builder.build();
    let options = sentry::integrations::log::LoggerOptions {
        global_filter: Some(logger.filter()),
        ..Default::default()
    };
    sentry::integrations::log::init(Some(Box::new(logger)), options);

    info!("Migrating database...");
    let connection = config::establish_connection();
    embedded_migrations::run_with_output(&connection, &mut std::io::stdout())
        .expect("Unable to run migrations");
    info!("Migrations complete!");

    actix_rt::System::new("wwfypc-payments").block_on(async move {
        let db_addr = SyncArbiter::start(3, || {
            db::DbExecutor::new(config::establish_connection())
        });

        let oauth_client = config::oauth_client();
        let keycloak_client = config::keycloak_client();
        let mail_client = config::mail_client();
        let worldpay_config = config::worldpay_config();
        let amqp_client = config::amqp_client();

        let jobs_data = jobs::JobsState {
            db: db_addr.clone(),
            keycloak: keycloak_client.clone(),
            oauth: oauth_client.clone(),
            mail_client,
            amqp: Arc::new(Mutex::new(amqp_client)),
        };

        let data = config::AppState {
            oauth: oauth_client,
            keycloak: keycloak_client,
            worldpay: worldpay_config,
            apple_pay_client: config::apple_pay_identity(),
            db: db_addr,
            jobs_state: jobs_data,
        };

        let mut server = HttpServer::new(move || {
            let generated = generate();

            App::new()
//            .middleware(sentry_actix::SentryMiddleware::new())
                .data(data.clone())
                .wrap(middleware::Logger::default())
                .wrap(middleware::Compress::default())
                .wrap(config::cookie_session())
                .service(actix_web_static_files::ResourceFiles::new(
                    "/static",
                    generated,
                ))
                .route(".well-known/apple-developer-merchantid-domain-association.txt", web::get().to(|| HttpResponse::Ok().body(
                    actix_web::dev::Body::from_slice(include_bytes!("../apple-developer-merchantid-domain-association.txt")))))
                .route("google2e531c99680612ef.html", web::get().to(|| HttpResponse::Ok().body(
                    actix_web::dev::Body::from_slice(include_bytes!("../google2e531c99680612ef.html")))))
                .route("/login/auth/", web::get().to(login_views::start_login))
                .route("/login/logout/", web::get().to(login_views::start_logout))
                .route("/login/redirect/", web::get().to(login_views::login_callback))
                .service(
                    web::resource("/login/whoami/")
                        .wrap(Cors::new()
                            .supports_credentials()
                            .finish())
                        .route(web::get().to(login_views::whoami))
                )
                .route("/apple-merchant-verification/", web::post().to(apple_pay::merchant_verification))
                .route("/payment/new/", web::post().to(payment_views::new_payment))
                .route("/payment/login-complete/", web::get().to(payment_views::render_login_complete))
                .service(
                    web::resource("/payment/{payment_id}/")
                        .wrap(Cors::new()
                            .supports_credentials()
                            .finish())
                        .route(web::get().to(payment_views::get_payment))
                )
                .service(
                    web::resource("/payments/")
                        .wrap(Cors::new()
                            .supports_credentials()
                            .finish())
                        .route(web::get().to(payment_views::get_payments))
                )
                .service(
                    web::resource("/payment/worldpay/{payment_id}/")
                        .wrap(Cors::new()
                            .supports_credentials()
                            .finish())
                        .route(web::post().to(worldpay::process_worldpay_payment))
                )
                .route("/payment/3ds/{payment_id}/", web::get().to(worldpay::render_3ds_form))
                .route("/payment/3ds-complete/{payment_id}/", web::post().to(worldpay::render_3ds_complete))
                .route("/payment/fb/{payment_id}/", web::get().to(payment_views::render_fb_payment))
                .service(
                    web::scope("/admin")
                        .default_service(web::route().to(admin_views::render_admin))
                )
        });

        let mut listenfd = listenfd::ListenFd::from_env();

        info!("Start listening...");
        server = if let Some(l) = listenfd.take_tcp_listener(0).unwrap() {
            server.listen(l).unwrap()
        } else {
            server.bind("[::]:3000").unwrap()
        };

        server.run().await
    }).unwrap()
}

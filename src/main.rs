#![feature(type_alias_impl_trait)]
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_derive_enum;
#[macro_use]
extern crate diesel_derives;
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

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

embed_migrations!("./migrations");

lazy_static! {
    pub static ref TERA: Tera = {
        let mut tera = compile_templates!("templates/**/*");
        tera.autoescape_on(vec!["html", ".sql"]);
        tera
    };
}

fn main() {
    pretty_env_logger::init();

    info!("Migrating database...");
    let connection = config::establish_connection();
    embedded_migrations::run_with_output(&connection, &mut std::io::stdout())
        .expect("Unable to run migrations");
    info!("Migrations complete!");

    let sys = actix::System::new("wwfypc-payments");

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
            .route("/login/auth/", web::get().to_async(actix_web_async_await::compat5(login_views::start_login)))
            .route("/login/logout/", web::get().to_async(actix_web_async_await::compat4(login_views::start_logout)))
            .route("/login/redirect/", web::get().to_async(actix_web_async_await::compat3(login_views::login_callback)))
            .route("/apple-merchant-verification/", web::post().to_async(actix_web_async_await::compat3(apple_pay::merchant_verification)))
            .route("/payment/new/", web::post().to_async(actix_web_async_await::compat3(payment_views::new_payment)))
            .route("/payment/login-complete/", web::get().to_async(actix_web_async_await::compat2(payment_views::render_login_complete)))
            .service(
                web::resource("/payment/{payment_id}/")
                    .wrap(Cors::new()
                        .supports_credentials())
                    .route(web::get().to_async(actix_web_async_await::compat4(payment_views::get_payment)))
            )
            .service(
                web::resource("/payment/worldpay/{payment_id}/")
                    .wrap(Cors::new()
                        .supports_credentials())
                    .route(web::post().to_async(actix_web_async_await::compat5(worldpay::process_worldpay_payment)))
            )
            .route("/payment/3ds/{payment_id}/", web::get().to_async(actix_web_async_await::compat3(worldpay::render_3ds_form)))
            .route("/payment/3ds-complete/{payment_id}/", web::post().to_async(actix_web_async_await::compat5(worldpay::render_3ds_complete)))
            .route("/payment/fb/{payment_id}/", web::get().to_async(actix_web_async_await::compat5(payment_views::render_fb_payment)))
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

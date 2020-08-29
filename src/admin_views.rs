use actix_web::{HttpResponse};

pub async fn render_admin() -> actix_web::Result<impl actix_web::Responder> {
    let context = tera::Context::new();

    match crate::TERA.render("admin/index.html", &context) {
        Ok(r) => Ok(HttpResponse::Ok().body(r)),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e))
    }
}
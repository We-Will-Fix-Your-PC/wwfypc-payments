use lettre_email::Email;
use lettre::Transport;
use chrono::prelude::*;
use futures01::future::{Future, ok, err};
use failure::{Error, Fallible};
use std::sync::{Arc, Mutex};
use crate::db;

#[derive(Clone)]
pub struct JobsState {
    pub oauth: crate::oauth::OAuthClient,
    pub keycloak: crate::keycloak::KeycloakClient,
    pub db: actix::Addr<crate::db::DbExecutor>,
    pub mail_client: lettre::smtp::SmtpClient,
    pub amqp: Arc<Mutex<amqp::Channel>>,
}

#[derive(Clone, Debug)]
pub struct CompletePayment {
    payment_id: uuid::Uuid
}

impl CompletePayment {
    pub fn new(payment_id: &uuid::Uuid) -> Self {
        Self {
            payment_id: payment_id.to_owned()
        }
    }
}

pub fn send_payment_notification(data: CompletePayment, state: JobsState) -> Fallible<()> {
    let payment = state.db.send(db::GetPayment::new(&data.payment_id))
        .from_err()
        .and_then(|res| match res {
            Ok(payment) => ok(payment),
            Err(e) => err(Error::from(e))
        }).wait()?;
    let items = state.db.send(db::GetPaymentItems::new(&payment))
        .from_err()
        .and_then(|res| match res {
            Ok(items) => ok(items),
            Err(e) => err(Error::from(e))
        }).wait()?;
    let token = futures::executor::block_on(state.oauth.get_access_token())?;
    let user = futures::executor::block_on(state.keycloak.get_user(payment.customer_id, &token))?;
    let email_items: String = items.into_iter()
        .map(|item| format!(
            "- {}x {} @{} GBP
- Item type: {}
- Item data: {}",
            item.quantity, item.title, (item.price.0 as f64) / 100.0, item.item_type, item.item_data
        ))
        .collect::<Vec<_>>()
        .join("\n\n");

    let email_content = format!(
        "New order
---
Order id: {}
Order date: {}
Environment: {}
Payment method: {}
---
Customer name: {}
Customer email: {}
Customer phone: {}
---
Items:

{}
",
        payment.id, DateTime::<Utc>::from_utc(payment.time, Utc),
        payment.environment, match &payment.payment_method {
            Some(m) => m,
            None => "N/A",
        },
        format!("{} {}", user.first_name.as_ref().unwrap_or(&"NFN".to_string()), user.last_name.as_ref().unwrap_or(&"NLN".to_string())),
        user.email.as_ref().unwrap_or(&"N/A".to_string()),
        match &user.get_attribute("phone") {
            Some(s) => s,
            None => "N/A"
        }, email_items,
    );

    let email = Email::builder()
        .to("q@misell.cymru")
        .from("noreply@noreply.wewillfixyourpc.co.uk")
        .subject("New order notification")
        .text(email_content)
        .build()?;

    state.mail_client.transport().send(email.into())?;

    Ok(())
}
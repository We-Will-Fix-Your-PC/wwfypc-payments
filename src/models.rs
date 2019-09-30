extern crate uuid;

use uuid::Uuid;

#[derive(Queryable, Clone, Debug)]
pub struct Payment {
    pub id: Uuid,
    pub time: String,
    pub state: crate::schema::PaymentState,
    pub customer_id: Uuid,
    pub environment: crate::schema::PaymentEnvironment,
    pub payment_method: Option<String>,
}
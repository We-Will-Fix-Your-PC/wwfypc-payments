use uuid::Uuid;
use super::schema::{payments, payment_items, threeds_datas, card_tokens, payment_tokens};
use chrono::prelude::*;
use diesel::data_types::PgMoney as Pence;

#[derive(Copy, Clone, Debug, Deserialize, Serialize, DbEnum, PartialEq)]
pub enum PaymentState {
    OPEN,
    PAID,
    COMPLETE
}


#[derive(Copy, Clone, Debug, Deserialize, Serialize, DbEnum, PartialEq)]
pub enum PaymentEnvironment {
    TEST,
    LIVE
}

#[derive(Queryable, Identifiable, AsChangeset, Clone, Debug, PartialEq)]
pub struct Payment {
    pub id: Uuid,
    pub time: NaiveDateTime,
    pub state: PaymentState,
    pub customer_id: Uuid,
    pub environment: PaymentEnvironment,
    pub payment_method: Option<String>,
}

#[derive(Clone, Debug, Insertable)]
#[table_name="payments"]
pub struct NewPayment<'a> {
    pub id: &'a Uuid,
    pub time: &'a NaiveDateTime,
    pub state: PaymentState,
    pub customer_id: &'a Uuid,
    pub environment:PaymentEnvironment
}

#[derive(Queryable, Identifiable, Associations, AsChangeset, Clone, Debug, PartialEq)]
#[belongs_to(Payment)]
pub struct PaymentItem {
    pub id: Uuid,
    pub payment_id: Uuid,
    pub item_type: String,
    pub item_data: serde_json::Value,
    pub title: String,
    pub quantity: i32,
    pub price: Pence
}

#[derive(Clone, Debug, Insertable)]
#[table_name="payment_items"]
pub struct NewPaymentItem<'a> {
    pub id: &'a Uuid,
    pub payment_id: &'a Uuid,
    pub item_type: &'a str,
    pub item_data: &'a serde_json::Value,
    pub title: &'a str,
    pub quantity: i32,
    pub price: &'a Pence
}

#[derive(Queryable, Identifiable, Associations, AsChangeset, Clone, Debug, PartialEq)]
#[belongs_to(Payment)]
pub struct ThreedsData {
    pub id: i64,
    pub payment_id: Uuid,
    pub one_time_3ds_token: String,
    pub redirect_url: String,
    pub order_id: String,
    pub timestamp: NaiveDateTime,
}

#[derive(Clone, Debug, Insertable)]
#[table_name="threeds_datas"]
pub struct NewThreedsData<'a> {
    pub payment_id: &'a Uuid,
    pub one_time_3ds_token: &'a str,
    pub redirect_url: &'a str,
    pub order_id: &'a str,
}

#[derive(Queryable, Identifiable, AsChangeset, Clone, Debug, PartialEq)]
pub struct CardToken {
    pub id: i64,
    pub customer_id: Uuid,
    pub token: String,
}

#[derive(Clone, Debug, Insertable)]
#[table_name="card_tokens"]
pub struct NewCardToken<'a> {
    pub customer_id: &'a Uuid,
    pub token: &'a str,
}

#[derive(Queryable, Identifiable, AsChangeset, Clone, Debug, PartialEq)]
pub struct PaymentToken {
    pub id: i64,
    pub name: String,
    pub token: Vec<u8>,
}

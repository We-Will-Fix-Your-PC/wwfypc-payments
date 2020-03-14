use actix::prelude::*;
use diesel::prelude::*;
use diesel::pg::PgConnection;
use uuid::Uuid;
use chrono::prelude::*;
use rust_decimal::prelude::*;
use crate::{models, schema};
use diesel::data_types::PgMoney as Pence;

pub struct DbExecutor(PgConnection);

impl DbExecutor {
    pub fn new(conn: PgConnection) -> Self {
        Self(conn)
    }
}

impl Actor for DbExecutor {
    type Context = SyncContext<Self>;
}

pub struct GetPayment {
    id: Uuid,
}

impl GetPayment {
    pub fn new(id: &Uuid) -> Self {
        Self {
            id: id.to_owned()
        }
    }
}

impl Message for GetPayment {
    type Result = Result<models::Payment, diesel::result::Error>;
}

impl Handler<GetPayment> for DbExecutor {
    type Result = Result<models::Payment, diesel::result::Error>;

    fn handle(&mut self, msg: GetPayment, _: &mut Self::Context) -> Self::Result {
        use schema::payments::dsl::*;

        payments.find(msg.id)
            .first::<models::Payment>(&self.0)
    }
}

pub struct GetPaymentItems {
    payment: models::Payment,
}

impl GetPaymentItems {
    pub fn new(payment: &models::Payment) -> Self {
        Self {
            payment: payment.to_owned()
        }
    }
}

impl Message for GetPaymentItems {
    type Result = Result<Vec<models::PaymentItem>, diesel::result::Error>;
}

impl Handler<GetPaymentItems> for DbExecutor {
    type Result = Result<Vec<models::PaymentItem>, diesel::result::Error>;

    fn handle(&mut self, msg: GetPaymentItems, _: &mut Self::Context) -> Self::Result {
        models::PaymentItem::belonging_to(&msg.payment)
            .load::<models::PaymentItem>(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct CreatePayment {
    id: Uuid,
    time: NaiveDateTime,
    state: models::PaymentState,
    environment: models::PaymentEnvironment,
    customer_id: Uuid,
    items: Vec<CreatePaymentItem>,
}

#[derive(Debug, Clone)]
pub struct CreatePaymentItem {
    id: Uuid,
    item_type: String,
    item_data: serde_json::Value,
    title: String,
    quantity: i32,
    price: rust_decimal::Decimal,
}

impl CreatePayment {
    pub fn new(id: &Uuid, time: &NaiveDateTime, state: models::PaymentState, environment: models::PaymentEnvironment, customer_id: &Uuid, items: &[CreatePaymentItem]) -> Self {
        Self {
            id: id.to_owned(),
            time: time.to_owned(),
            state,
            environment,
            customer_id: customer_id.to_owned(),
            items: items.to_vec(),
        }
    }
}

impl CreatePaymentItem {
    pub fn new(id: &Uuid, item_type: &str, item_data: &serde_json::Value, title: &str, quantity: i32, price: &rust_decimal::Decimal) -> Self {
        Self {
            id: id.to_owned(),
            item_type: item_type.to_owned(),
            item_data: item_data.to_owned(),
            title: title.to_owned(),
            quantity,
            price: price.to_owned(),
        }
    }
}

impl Message for CreatePayment {
    type Result = Result<models::Payment, diesel::result::Error>;
}

impl Handler<CreatePayment> for DbExecutor {
    type Result = Result<models::Payment, diesel::result::Error>;

    fn handle(&mut self, msg: CreatePayment, _: &mut Self::Context) -> Self::Result {
        self.0.transaction(|| {
            let new_payment = models::NewPayment {
                id: &msg.id,
                time: &msg.time,
                state: msg.state,
                environment: msg.environment,
                customer_id: &msg.customer_id,
            };

            let payment = diesel::insert_into(schema::payments::table)
                .values(&new_payment)
                .get_result(&self.0)?;

            for item in msg.items.iter() {
                let new_payment_item = models::NewPaymentItem {
                    id: &item.id,
                    payment_id: &msg.id,
                    item_type: &item.item_type,
                    item_data: &item.item_data,
                    title: &item.title,
                    quantity: item.quantity,
                    price: &Pence((item.price * rust_decimal::Decimal::new(100, 0)).to_i64().unwrap()),
                };

                diesel::insert_into(schema::payment_items::table)
                    .values(&new_payment_item)
                    .execute(&self.0)?;
            }

            Ok(payment)
        })
    }
}

#[derive(Debug, Clone)]
pub struct UpdatePaymentState {
    id: Uuid,
    state: models::PaymentState,
    payment_method: Option<String>,
}

impl UpdatePaymentState {
    pub fn new(id: &Uuid, state: models::PaymentState, payment_method: Option<&str>) -> Self {
        Self {
            id: id.to_owned(),
            state,
            payment_method: match payment_method {
                Some(s) => Some(s.to_owned()),
                None => None
            }
        }
    }
}

impl Message for UpdatePaymentState {
    type Result = Result<(), diesel::result::Error>;
}

impl Handler<UpdatePaymentState> for DbExecutor {
    type Result = Result<(), diesel::result::Error>;

    fn handle(&mut self, msg: UpdatePaymentState, _: &mut Self::Context) -> Self::Result {
       let q = diesel::update(schema::payments::table.find(msg.id));

        if let Some(pm) = msg.payment_method {
            q.set((schema::payments::state.eq(msg.state), schema::payments::payment_method.eq(pm))).execute(&self.0)?;
        } else {
            q.set(schema::payments::state.eq(msg.state)).execute(&self.0)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CreateThreedsData {
    payment_id: Uuid,
    one_time_3ds_token: String,
    redirect_url: String,
    order_id: String,
}

impl CreateThreedsData {
    pub fn new(payment_id: &Uuid, one_time_3ds_token: &str, redirect_url: &str, order_id: &str) -> Self {
        Self {
            payment_id: payment_id.to_owned(),
            one_time_3ds_token: one_time_3ds_token.to_owned(),
            redirect_url: redirect_url.to_owned(),
            order_id: order_id.to_owned()
        }
    }
}

impl Message for CreateThreedsData {
    type Result = Result<models::ThreedsData, diesel::result::Error>;
}

impl Handler<CreateThreedsData> for DbExecutor {
    type Result = Result<models::ThreedsData, diesel::result::Error>;

    fn handle(&mut self, msg: CreateThreedsData, _: &mut Self::Context) -> Self::Result {
        let new_data = models::NewThreedsData {
            payment_id: &msg.payment_id,
            one_time_3ds_token: &msg.one_time_3ds_token,
            redirect_url: &msg.redirect_url,
            order_id: &msg.order_id
        };

        diesel::insert_into(schema::threeds_datas::table)
            .values(&new_data)
            .get_result(&self.0)
    }
}

pub struct GetThreedsData {
    payment: models::Payment,
}

impl GetThreedsData {
    pub fn new(payment: &models::Payment) -> Self {
        Self {
            payment: payment.to_owned()
        }
    }
}

impl Message for GetThreedsData {
    type Result = Result<models::ThreedsData, diesel::result::Error>;
}

impl Handler<GetThreedsData> for DbExecutor {
    type Result = Result<models::ThreedsData, diesel::result::Error>;

    fn handle(&mut self, msg: GetThreedsData, _: &mut Self::Context) -> Self::Result {
        models::ThreedsData::belonging_to(&msg.payment)
            .order_by(schema::threeds_datas::timestamp.desc())
            .first::<models::ThreedsData>(&self.0)
    }
}

pub struct DeleteThreedsData {
    payment: models::Payment,
}

impl DeleteThreedsData {
    pub fn new(payment: &models::Payment) -> Self {
        Self {
            payment: payment.to_owned()
        }
    }
}

impl Message for DeleteThreedsData {
    type Result = Result<(), diesel::result::Error>;
}

impl Handler<DeleteThreedsData> for DbExecutor {
    type Result = Result<(), diesel::result::Error>;

    fn handle(&mut self, msg: DeleteThreedsData, _: &mut Self::Context) -> Self::Result {
        diesel::delete(models::ThreedsData::belonging_to(&msg.payment))
            .execute(&self.0)?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CreateCard {
    customer_id: Uuid,
    pan: String,
    exp_month: u32,
    exp_year: u32,
    name_on_card: String,
}

impl CreateCard {
    pub fn new(customer_id: &Uuid, pan: &str, exp_month: u32, exp_year: u32, name_on_card: &str) -> Self {
        Self {
            customer_id: customer_id.to_owned(),
            pan: pan.to_string(),
            exp_month,
            exp_year,
            name_on_card: name_on_card.to_string()
        }
    }
}

impl Message for CreateCard {
    type Result = Result<models::Card, diesel::result::Error>;
}

impl Handler<CreateCard> for DbExecutor {
    type Result = Result<models::Card, diesel::result::Error>;

    fn handle(&mut self, msg: CreateCard, _: &mut Self::Context) -> Self::Result {
        use schema::cards::dsl::*;
        let existing_card = cards.filter(pan.eq(&msg.pan))
            .first::<models::Card>(&self.0);

        match existing_card {
            Ok(card) => Ok(card),
            Err(diesel::result::Error::NotFound) => {
                let new_data = models::NewCard {
                    customer_id: &msg.customer_id,
                    pan: &msg.pan,
                    exp_month: msg.exp_month as i32,
                    exp_year: msg.exp_year as i32,
                    name_on_card: &msg.name_on_card
                };

                diesel::insert_into(schema::cards::table)
                    .values(&new_data)
                    .get_result(&self.0)
            }
            Err(e) => Err(e),
        }
    }
}


pub struct GetPaymentTokens {
}

impl GetPaymentTokens {
    pub fn new() -> Self {
        Self {}
    }
}

impl Message for GetPaymentTokens {
    type Result = Result<Vec<models::PaymentToken>, diesel::result::Error>;
}

impl Handler<GetPaymentTokens> for DbExecutor {
    type Result = Result<Vec<models::PaymentToken>, diesel::result::Error>;

    fn handle(&mut self, _msg: GetPaymentTokens, _: &mut Self::Context) -> Self::Result {
        schema::payment_tokens::table.load::<models::PaymentToken>(&self.0)
    }
}

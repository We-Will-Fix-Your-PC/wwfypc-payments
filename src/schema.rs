#[derive(Clone, Debug, Deserialize, DbEnum)]
pub enum PaymentState {
    OPEN,
    PAID,
    COMPLETE
}


#[derive(Clone, Debug, Deserialize, DbEnum)]
pub enum PaymentEnvironment {
    TEST,
    LIVE
}

table! {
    use diesel::sql_types::*;
    use super::{PaymentState, PaymentEnvironment};
    payments (id) {
        id -> Uuid,
        time -> Timestamp,
        state -> PaymentState,
        customer_id -> Uuid,
        environment -> PaymentEnvironment,
        payment_method -> Nullable<Varchar>,
    }
}

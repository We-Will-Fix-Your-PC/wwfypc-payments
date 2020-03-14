table! {
    cards (id) {
        id -> Uuid,
        customer_id -> Uuid,
        pan -> Varchar,
        exp_month -> Int4,
        exp_year -> Int4,
        name_on_card -> Varchar,
    }
}

table! {
    payment_items (id) {
        id -> Uuid,
        payment_id -> Uuid,
        item_type -> Varchar,
        item_data -> Jsonb,
        title -> Varchar,
        quantity -> Int4,
        price -> Money,
    }
}

table! {
    payments (id) {
        id -> Uuid,
        time -> Timestamp,
        state -> crate::models::PaymentStateMapping,
        customer_id -> Uuid,
        environment -> crate::models::PaymentEnvironmentMapping,
        payment_method -> Nullable<Varchar>,
    }
}

table! {
    payment_tokens (id) {
        id -> Int8,
        name -> Varchar,
        token -> Bytea,
    }
}

table! {
    threeds_datas (id) {
        id -> Int8,
        payment_id -> Uuid,
        one_time_3ds_token -> Text,
        redirect_url -> Text,
        order_id -> Varchar,
        timestamp -> Timestamp,
    }
}

joinable!(payment_items -> payments (payment_id));
joinable!(threeds_datas -> payments (payment_id));

allow_tables_to_appear_in_same_query!(
    cards,
    payment_items,
    payments,
    payment_tokens,
    threeds_datas,
);

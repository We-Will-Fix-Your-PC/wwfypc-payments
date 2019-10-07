create table threeds_datas (
    id bigserial not null primary key,
    payment_id uuid not null references payments(id),
    one_time_3ds_token text not null,
    redirect_url text not null,
    order_id varchar not null,
    timestamp timestamp not null default now()
);
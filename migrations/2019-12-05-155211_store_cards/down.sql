create table card_tokens (
                             id bigserial not null primary key,
                             customer_id uuid not null,
                             token text not null
);
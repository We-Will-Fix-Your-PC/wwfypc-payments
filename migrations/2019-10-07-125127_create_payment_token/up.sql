create table payment_tokens (
    id bigserial not null primary key,
    name varchar not null,
    token bytea not null
);
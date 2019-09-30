create type payment_state AS ENUM ('open', 'paid', 'complete');
create type payment_environment AS ENUM ('test', 'live');

create table payments (
    id uuid not null primary key,
    time timestamp not null default now(),
    state payment_state not null default 'open',
    customer_id uuid not null,
    environment payment_environment not null default 'live',
    payment_method varchar
);
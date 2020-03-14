drop table card_tokens;

create table cards (
   id uuid not null primary key,
   customer_id uuid not null,
   pan varchar not null,
   exp_month int not null,
   exp_year int not null,
   name_on_card varchar not null
);
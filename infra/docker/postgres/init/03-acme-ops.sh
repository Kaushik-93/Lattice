#!/usr/bin/env bash
# A small operational database standing in for a customer's Postgres, so the Postgres connector
# can be tried (and e2e-tested) against something realistic. Read-only user: acme_reader.
set -euo pipefail

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
  -v reader_password="${ACME_READER_PASSWORD:-acme-reader}" <<'SQL'
CREATE DATABASE acme_ops;
CREATE ROLE acme_reader LOGIN PASSWORD :'reader_password' NOSUPERUSER NOCREATEDB NOCREATEROLE;
SQL

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname acme_ops <<'SQL'
CREATE TABLE stores (
    store_id    int PRIMARY KEY,
    store_name  text NOT NULL,
    region      text NOT NULL,
    city        text NOT NULL,
    opened_on   date NOT NULL
);
INSERT INTO stores
SELECT i, 'Store ' || lpad(i::text, 3, '0'),
       (ARRAY['AMER', 'EMEA', 'APAC'])[1 + i % 3],
       (ARRAY['New York', 'London', 'Singapore', 'Chicago', 'Berlin', 'Tokyo',
              'Toronto', 'Paris', 'Sydney', 'São Paulo', 'Madrid', 'Mumbai'])[1 + i % 12],
       DATE '2019-03-01' + i * 37
FROM generate_series(1, 60) AS i;

CREATE TABLE revenue_targets (
    fiscal_quarter  text NOT NULL,
    region          text NOT NULL,
    target_usd      numeric(14, 2) NOT NULL,
    PRIMARY KEY (fiscal_quarter, region)
);
INSERT INTO revenue_targets
SELECT 'FY26 Q' || q, r, (2500000 + q * 150000 + length(r) * 40000)::numeric(14, 2)
FROM generate_series(1, 4) AS q, unnest(ARRAY['AMER', 'EMEA', 'APAC']) AS r;

CREATE TABLE refunds (
    refund_id    bigint PRIMARY KEY,
    order_id     bigint NOT NULL,
    refunded_at  timestamptz NOT NULL,
    amount_usd   numeric(12, 2) NOT NULL,
    reason       text NOT NULL
);
INSERT INTO refunds
SELECT i, (i * 7919) % 1000000 + 1,
       TIMESTAMPTZ '2024-02-01' + (i % 1000) * interval '1 day',
       ((i * 37) % 20000) / 100.0 + 5,
       (ARRAY['damaged', 'late delivery', 'wrong item', 'changed mind'])[1 + i % 4]
FROM generate_series(1, 5000) AS i;

GRANT CONNECT ON DATABASE acme_ops TO acme_reader;
GRANT USAGE ON SCHEMA public TO acme_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO acme_reader;
SQL

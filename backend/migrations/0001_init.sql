-- money-data schema. Money is stored as DOUBLE PRECISION to match the MVP's
-- f64 math (fx.rs); switch to NUMERIC for production-grade money handling.

CREATE TABLE operators (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    region              TEXT NOT NULL,
    settlement_currency TEXT NOT NULL
);

CREATE TABLE wallets (
    currency  TEXT PRIMARY KEY,
    available DOUBLE PRECISION NOT NULL DEFAULT 0,
    pending   DOUBLE PRECISION NOT NULL DEFAULT 0,
    position  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE payment_links (
    id                         TEXT PRIMARY KEY,
    reference                  TEXT NOT NULL,
    guest_name                 TEXT NOT NULL,
    description                TEXT NOT NULL,
    settlement_currency        TEXT NOT NULL,
    settlement_amount          DOUBLE PRECISION NOT NULL,
    presentment_currency       TEXT NOT NULL,
    mid_rate                   DOUBLE PRECISION NOT NULL,
    quoted_rate                DOUBLE PRECISION NOT NULL,
    converted_amount           DOUBLE PRECISION NOT NULL,
    processing_fee             DOUBLE PRECISION NOT NULL,
    total_charged              DOUBLE PRECISION NOT NULL,
    fx_markup_revenue_usd      DOUBLE PRECISION NOT NULL,
    processing_fee_revenue_usd DOUBLE PRECISION NOT NULL,
    status                     TEXT NOT NULL,
    url                        TEXT NOT NULL,
    created_at                 TIMESTAMPTZ NOT NULL,
    paid_at                    TIMESTAMPTZ
);

CREATE TABLE refunds (
    id                TEXT PRIMARY KEY,
    payment_id        TEXT NOT NULL,
    payment_reference TEXT NOT NULL,
    method            TEXT NOT NULL,
    amount            DOUBLE PRECISION NOT NULL,
    currency          TEXT NOT NULL,
    status            TEXT NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL
);

CREATE TABLE settlement_batches (
    id         TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL,
    total_usd  DOUBLE PRECISION NOT NULL
);

CREATE TABLE settlement_lines (
    id       BIGSERIAL PRIMARY KEY,
    batch_id TEXT NOT NULL REFERENCES settlement_batches(id),
    currency TEXT NOT NULL,
    amount   DOUBLE PRECISION NOT NULL,
    payments BIGINT NOT NULL
);

-- Single-row table of accumulated platform revenue (USD).
CREATE TABLE platform_revenue (
    id                     INTEGER PRIMARY KEY DEFAULT 1,
    fx_markup_revenue_usd  DOUBLE PRECISION NOT NULL DEFAULT 0,
    processing_revenue_usd DOUBLE PRECISION NOT NULL DEFAULT 0,
    CONSTRAINT single_row CHECK (id = 1)
);

CREATE INDEX idx_payment_links_created_at ON payment_links (created_at DESC);
CREATE INDEX idx_payment_links_status ON payment_links (status);

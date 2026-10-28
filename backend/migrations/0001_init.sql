CREATE TABLE quotes (
    id                   TEXT PRIMARY KEY,
    source_currency      TEXT NOT NULL,
    destination_currency TEXT NOT NULL,
    destination_rail     TEXT NOT NULL,
    -- 0 when the payout provider converts at its own rate and destination_amount is an estimate.
    payout_amount_guaranteed INTEGER NOT NULL,
    amount_side          TEXT NOT NULL,
    source_amount        INTEGER NOT NULL,
    destination_amount   INTEGER NOT NULL,
    amount_sats          INTEGER NOT NULL,
    fee_sats             INTEGER NOT NULL,
    fee_source_amount    INTEGER NOT NULL,
    fee_bps              INTEGER NOT NULL,
    exchange_rate        TEXT NOT NULL,
    rate_source          TEXT NOT NULL,
    -- When the oldest BTC price behind this quote was fetched.
    rate_as_of           TEXT NOT NULL,
    expires_at           TEXT NOT NULL,
    created_at           TEXT NOT NULL
);

CREATE TABLE payment_intents (
    id                   TEXT PRIMARY KEY,
    -- One payment per quote: a quote can only be spent once.
    quote_id             TEXT NOT NULL UNIQUE REFERENCES quotes(id),
    source_currency      TEXT NOT NULL,
    destination_currency TEXT NOT NULL,
    source_amount        INTEGER NOT NULL,
    expected_payout      INTEGER NOT NULL,
    amount_sats          INTEGER NOT NULL,
    fee_sats             INTEGER NOT NULL,
    exchange_rate        TEXT NOT NULL,
    recipient            TEXT NOT NULL,
    recipient_rail       TEXT NOT NULL,
    recipient_masked     TEXT NOT NULL,
    recipient_name       TEXT,
    status               TEXT NOT NULL,
    failure_reason       TEXT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    expires_at           TEXT NOT NULL
);
CREATE INDEX idx_payment_intents_created_at ON payment_intents(created_at);

CREATE TABLE lightning_payments (
    payment_intent_id TEXT PRIMARY KEY REFERENCES payment_intents(id),
    payment_request   TEXT NOT NULL,
    payment_hash      TEXT NOT NULL UNIQUE,
    amount_sats       INTEGER NOT NULL,
    status            TEXT NOT NULL,
    expires_at        TEXT NOT NULL,
    settled_at        TEXT,
    created_at        TEXT NOT NULL
);

CREATE TABLE payouts (
    -- Primary key on the payment guarantees at most one payout per payment.
    payment_intent_id     TEXT PRIMARY KEY REFERENCES payment_intents(id),
    provider              TEXT NOT NULL,
    rail                  TEXT NOT NULL,
    destination_currency  TEXT NOT NULL,
    destination_amount    INTEGER NOT NULL,
    destination_reference TEXT NOT NULL,
    account_name          TEXT,
    provider_reference    TEXT UNIQUE,
    status                TEXT NOT NULL,
    failure_reason        TEXT,
    created_at            TEXT NOT NULL,
    completed_at          TEXT,
    reversed_at           TEXT
);

CREATE TABLE recipient_resolutions (
    id                  TEXT PRIMARY KEY,
    currency            TEXT NOT NULL,
    bank_code           TEXT NOT NULL,
    account_number_hash TEXT NOT NULL,
    account_name        TEXT NOT NULL,
    provider            TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    expires_at          TEXT NOT NULL
);

CREATE TABLE audit_events (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    payment_intent_id TEXT NOT NULL REFERENCES payment_intents(id),
    event_type        TEXT NOT NULL,
    from_status       TEXT,
    to_status         TEXT,
    detail            TEXT,
    created_at        TEXT NOT NULL
);
CREATE INDEX idx_audit_events_payment ON audit_events(payment_intent_id);

-- Webhook replay/duplicate protection.
CREATE TABLE processed_webhooks (
    source      TEXT NOT NULL,
    event_id    TEXT NOT NULL,
    received_at TEXT NOT NULL,
    PRIMARY KEY (source, event_id)
);

-- Per-user API tokens (stored hashed).
CREATE TABLE users (
    id         TEXT PRIMARY KEY,
    contact    TEXT,
    token_hash TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

CREATE TABLE quotes (
    id                       TEXT PRIMARY KEY,
    user_id                  TEXT REFERENCES users(id),
    source_currency          TEXT NOT NULL,
    destination_currency     TEXT NOT NULL,
    destination_rail         TEXT NOT NULL,
    -- 0 when the payout provider converts at its own rate and destination_amount is an estimate.
    payout_amount_guaranteed INTEGER NOT NULL,
    amount_side              TEXT NOT NULL,
    -- What the sender pays, in source minor units.
    source_amount            INTEGER NOT NULL,
    fee_source_amount        INTEGER NOT NULL,
    fee_bps                  INTEGER NOT NULL,
    -- source_amount - fee: what is converted on Stellar.
    send_amount              INTEGER NOT NULL,
    -- What the recipient gets, in destination minor units; the path payment's destMin.
    destination_amount       INTEGER NOT NULL,
    -- Destination units per 1 source unit actually converted (after DEX spread, before slippage buffer).
    exchange_rate            TEXT NOT NULL,
    rate_source              TEXT NOT NULL,
    -- Asset codes along the DEX path, e.g. ["NGN","USD","KES"].
    conversion_path          TEXT NOT NULL,
    rate_as_of               TEXT NOT NULL,
    expires_at               TEXT NOT NULL,
    created_at               TEXT NOT NULL
);

CREATE TABLE payment_intents (
    id                   TEXT PRIMARY KEY,
    user_id              TEXT REFERENCES users(id),
    -- One payment per quote: a quote can only be spent once.
    quote_id             TEXT NOT NULL UNIQUE REFERENCES quotes(id),
    source_currency      TEXT NOT NULL,
    destination_currency TEXT NOT NULL,
    source_amount        INTEGER NOT NULL,
    fee_source_amount    INTEGER NOT NULL,
    send_amount          INTEGER NOT NULL,
    expected_payout      INTEGER NOT NULL,
    exchange_rate        TEXT NOT NULL,
    conversion_path      TEXT NOT NULL,
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
CREATE INDEX idx_payment_intents_user ON payment_intents(user_id, created_at);
CREATE INDEX idx_payment_intents_status ON payment_intents(status);

-- Bank-transfer collection of the sender's money (the on-ramp).
CREATE TABLE deposits (
    payment_intent_id  TEXT PRIMARY KEY REFERENCES payment_intents(id),
    provider           TEXT NOT NULL,
    currency           TEXT NOT NULL,
    expected_amount    INTEGER NOT NULL,
    -- What the sender must quote with the transfer; how the provider's webhook finds the deposit.
    reference          TEXT NOT NULL UNIQUE,
    bank_name          TEXT NOT NULL,
    account_number     TEXT NOT NULL,
    account_name       TEXT NOT NULL,
    provider_reference TEXT,
    status             TEXT NOT NULL,
    received_amount    INTEGER,
    -- The account the sender paid from (JSON), where the provider reports it; refunds go here.
    sender_account     TEXT,
    expires_at         TEXT NOT NULL,
    received_at        TEXT,
    created_at         TEXT NOT NULL
);

-- Every Stellar transaction, stored before it is submitted so its outcome can always be found.
CREATE TABLE stellar_transactions (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    payment_intent_id  TEXT NOT NULL REFERENCES payment_intents(id),
    kind               TEXT NOT NULL, -- settle | redeem | reverse
    tx_hash            TEXT NOT NULL UNIQUE,
    envelope_xdr       TEXT NOT NULL,
    operation          TEXT NOT NULL, -- the LedgerOp, JSON
    status             TEXT NOT NULL, -- submitted | succeeded | failed
    -- Path payment output, in stroops, once succeeded.
    delivered          INTEGER,
    ledger             INTEGER,
    error              TEXT,
    valid_until        TEXT NOT NULL,
    created_at         TEXT NOT NULL,
    confirmed_at       TEXT
);
-- At most one live (not failed) transaction of each kind per payment.
CREATE UNIQUE INDEX idx_stellar_tx_live ON stellar_transactions(payment_intent_id, kind) WHERE status != 'failed';

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
    reversed_at           TEXT,
    -- Submission attempts, for the recovery worker.
    attempts              INTEGER NOT NULL DEFAULT 0,
    last_attempt_at       TEXT,
    -- Set when a payout can't be safely retried automatically; shown on the ops dashboard.
    needs_review          INTEGER NOT NULL DEFAULT 0,
    review_reason         TEXT
);

-- Refund of the sender's deposit, in the source currency, over a bank payout rail.
CREATE TABLE refunds (
    payment_intent_id     TEXT PRIMARY KEY REFERENCES payment_intents(id),
    provider              TEXT NOT NULL,
    currency              TEXT NOT NULL,
    amount                INTEGER NOT NULL,
    destination           TEXT NOT NULL, -- Recipient JSON
    destination_reference TEXT NOT NULL,
    account_name          TEXT,
    provider_reference    TEXT UNIQUE,
    status                TEXT NOT NULL, -- pending | submitted | completed | failed
    failure_reason        TEXT,
    attempts              INTEGER NOT NULL DEFAULT 0,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL,
    completed_at          TEXT
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

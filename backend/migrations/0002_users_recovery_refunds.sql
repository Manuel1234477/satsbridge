-- Per-user API tokens (stored hashed).
CREATE TABLE users (
    id         TEXT PRIMARY KEY,
    contact    TEXT,
    token_hash TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

ALTER TABLE quotes ADD COLUMN user_id TEXT REFERENCES users(id);
ALTER TABLE payment_intents ADD COLUMN user_id TEXT REFERENCES users(id);
CREATE INDEX idx_payment_intents_user ON payment_intents(user_id, created_at);
CREATE INDEX idx_payment_intents_status ON payment_intents(status);

-- Payout submission attempts, for the recovery worker.
ALTER TABLE payouts ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE payouts ADD COLUMN last_attempt_at TEXT;
-- Set when a payout can't be safely retried automatically; shown on the ops dashboard.
ALTER TABLE payouts ADD COLUMN needs_review INTEGER NOT NULL DEFAULT 0;
ALTER TABLE payouts ADD COLUMN review_reason TEXT;

-- Lightning refund to the sender. refund_payment_hash doubles as the "refund in progress" lock.
ALTER TABLE payment_intents ADD COLUMN refund_payment_hash TEXT;
ALTER TABLE payment_intents ADD COLUMN refund_sats INTEGER;
ALTER TABLE payment_intents ADD COLUMN refunded_at TEXT;

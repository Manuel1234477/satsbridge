//! Integrations behind traits so real providers can replace the demo mocks (PRD FR-09 / FR-12).

pub mod fx;
pub mod lightning;
pub mod payout;

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("invalid recipient: {0}")]
    InvalidRecipient(String),
    #[error("recipient account not found")]
    RecipientNotFound,
    /// Transient: safe to retry with the same idempotency key.
    #[error("provider unavailable: {0}")]
    Unavailable(String),
    #[error("provider rejected the request: {0}")]
    Rejected(String),
}

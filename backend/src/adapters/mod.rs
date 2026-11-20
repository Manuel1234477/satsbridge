//! Integrations behind traits for real providers.

pub mod collection;
pub mod fx;
pub mod payout;
pub mod stellar;

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

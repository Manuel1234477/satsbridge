//! Collection rails: how the sender's money comes in. For NGN this is a bank transfer to a
//! per-payment virtual account; the provider reports the deposit on
//! `POST /api/v1/webhooks/collections`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::AdapterError;
use crate::domain::Currency;

pub mod mock;

pub struct DepositRequest<'a> {
    /// The payment intent ID. Providers must return the same account for repeats.
    pub idempotency_key: &'a str,
    pub currency: Currency,
    pub amount_minor: i64,
    pub expires_at: DateTime<Utc>,
}

/// Where the sender sends their bank transfer.
#[derive(Clone, Debug, Serialize)]
pub struct DepositInstructions {
    pub bank_name: String,
    pub account_number: String,
    pub account_name: String,
    /// Identifies this deposit in the provider's webhook (a virtual account number, or a
    /// narration reference for pooled accounts).
    pub reference: String,
    pub provider_reference: Option<String>,
}

/// The account a deposit came from, as reported by the provider. Refunds go back here.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SenderAccount {
    pub bank_code: String,
    pub account_number: String,
    #[serde(default)]
    pub account_name: Option<String>,
}

#[async_trait]
pub trait CollectionAdapter: Send + Sync {
    fn provider(&self) -> &'static str;
    fn currency(&self) -> Currency;

    fn max_deposit_minor(&self) -> Option<i64> {
        None
    }

    async fn create_deposit(&self, request: DepositRequest<'_>) -> Result<DepositInstructions, AdapterError>;
}

/// One collection rail per source currency.
#[derive(Default)]
pub struct CollectionRegistry {
    adapters: std::collections::BTreeMap<Currency, std::sync::Arc<dyn CollectionAdapter>>,
}

impl CollectionRegistry {
    pub fn register(&mut self, adapter: std::sync::Arc<dyn CollectionAdapter>) {
        self.adapters.insert(adapter.currency(), adapter);
    }

    pub fn get(&self, currency: Currency) -> Option<&std::sync::Arc<dyn CollectionAdapter>> {
        self.adapters.get(&currency)
    }

    pub fn currencies(&self) -> impl Iterator<Item = Currency> + '_ {
        self.adapters.keys().copied()
    }
}

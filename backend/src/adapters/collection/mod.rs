//! Collection rails: how the sender's money comes in, one per source currency. NGN is a bank
//! transfer to a per-payment virtual account; KES is an M-Pesa Paybill payment; GHS, XOF, XAF
//! and GNF are mobile money merchant payments. Either way the provider reports the deposit on
//! `POST /api/v1/webhooks/collections`, with the account it came from (refunds go back there).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::AdapterError;
use crate::domain::Currency;

pub struct DepositRequest<'a> {
    /// The payment intent ID. Providers must return the same account for repeats.
    pub idempotency_key: &'a str,
    pub currency: Currency,
    pub amount_minor: i64,
    pub expires_at: DateTime<Utc>,
}

/// How the sender pays in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum DepositMethod {
    /// Bank transfer to `account_number` at `bank_name`.
    BankTransfer,
    /// Mobile money payment to the merchant/Paybill number `account_number` on `bank_name`
    /// (the network), quoting `reference`.
    MobileMoney,
}

/// Where the sender sends their money.
#[derive(Clone, Debug, Serialize)]
pub struct DepositInstructions {
    pub method: DepositMethod,
    /// Bank, or mobile money network(s).
    pub bank_name: String,
    pub account_number: String,
    pub account_name: String,
    /// Identifies this deposit in the provider's webhook (a virtual account number, or a
    /// narration reference for pooled accounts).
    pub reference: String,
    pub provider_reference: Option<String>,
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

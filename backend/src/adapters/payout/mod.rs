use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use serde::Serialize;

use super::AdapterError;
use crate::{
    domain::{Currency, Rail, Recipient},
    services::payments::PayoutEventStatus,
};

pub mod lightning_address;
pub mod mobile_money_mock;
pub mod ngn_mock;
pub mod tando_mock;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstitutionKind {
    Bank,
    Wallet,
    MobileMoney,
}

#[derive(Clone, Debug, Serialize)]
pub struct Institution {
    pub code: String,
    pub name: String,
    pub kind: InstitutionKind,
}

#[derive(Debug, Default)]
pub struct ResolvedRecipient {
    pub account_name: Option<String>,
    /// Amount bounds the destination itself accepts (e.g. LNURL min/maxSendable).
    pub min_sats: Option<i64>,
    pub max_sats: Option<i64>,
    /// Destination-amount limits for this recipient's country, where they differ by country.
    pub min_minor: Option<i64>,
    pub max_minor: Option<i64>,
}

pub struct PayoutRequest<'a> {
    /// The payment intent ID. Providers must treat repeats of this key as the same payout.
    pub idempotency_key: &'a str,
    /// 1 for the first submission; retries count up.
    pub attempt: i64,
    pub recipient: &'a Recipient,
    /// Name confirmed by the sender (from account resolution), where the rail has one.
    pub account_name: Option<&'a str>,
    /// Destination amount in minor units, as quoted.
    pub amount_minor: i64,
    /// Sats available for this payout after SatsBridge's fee.
    pub amount_sats: i64,
    pub currency: Currency,
    pub narration: String,
}

pub struct PayoutAccepted {
    pub provider_reference: String,
    /// True when the rail confirms delivery synchronously (e.g. a settled Lightning payment);
    /// otherwise completion arrives via a provider webhook.
    pub completed: bool,
}

/// One payout rail for one destination currency.
#[async_trait]
pub trait PayoutAdapter: Send + Sync {
    fn provider(&self) -> &'static str;
    fn currency(&self) -> Currency;
    fn rail(&self) -> Rail;

    fn min_payout_minor(&self) -> Option<i64> {
        None
    }

    fn max_payout_minor(&self) -> Option<i64> {
        None
    }

    /// False when the provider converts at its own rate, so the quoted destination amount is an estimate.
    fn guarantees_destination_amount(&self) -> bool {
        true
    }

    /// Countries this rail pays into (ISO alpha-2), for multi-country rails; empty otherwise.
    fn countries(&self) -> Vec<String> {
        Vec::new()
    }

    /// Whether the recovery worker may resubmit after an unconfirmed attempt. Rails that can't
    /// tell whether an earlier attempt went through must return false; those go to manual review.
    fn safe_to_retry(&self) -> bool {
        true
    }

    /// `country` is required by multi-country rails (mobile money).
    async fn institutions(&self, country: Option<&str>) -> Result<Vec<Institution>, AdapterError>;

    /// Validates the destination and, where the rail supports it, looks up the account holder's name.
    async fn resolve_recipient(&self, recipient: &Recipient) -> Result<ResolvedRecipient, AdapterError>;

    async fn create_payout(&self, request: PayoutRequest<'_>) -> Result<PayoutAccepted, AdapterError>;

    /// Looks for a payout an earlier attempt already got accepted, before resubmitting.
    async fn find_payout(&self, _idempotency_key: &str) -> Result<Option<PayoutAccepted>, AdapterError> {
        Ok(None)
    }

    /// Final status of an accepted payout, if the provider has one, for when webhooks don't arrive.
    async fn payout_status(&self, _provider_reference: &str) -> Result<Option<PayoutEventStatus>, AdapterError> {
        Ok(None)
    }
}

#[derive(Default)]
pub struct PayoutRegistry {
    adapters: BTreeMap<(Currency, &'static str), Arc<dyn PayoutAdapter>>,
}

impl PayoutRegistry {
    pub fn register(&mut self, adapter: Arc<dyn PayoutAdapter>) {
        self.adapters
            .insert((adapter.currency(), adapter.rail().as_str()), adapter);
    }

    pub fn get(&self, currency: Currency, rail: Rail) -> Option<&Arc<dyn PayoutAdapter>> {
        self.adapters.get(&(currency, rail.as_str()))
    }

    pub fn for_currency(&self, currency: Currency) -> impl Iterator<Item = &Arc<dyn PayoutAdapter>> {
        self.adapters
            .iter()
            .filter(move |((c, _), _)| *c == currency)
            .map(|(_, a)| a)
    }

    /// The rail used when a quote doesn't name one: bank/mobile money before Lightning Address,
    /// real providers before mocks.
    pub fn default_for(&self, currency: Currency) -> Option<&Arc<dyn PayoutAdapter>> {
        let mut adapters: Vec<_> = self.for_currency(currency).collect();
        adapters.sort_by_key(|a| (a.rail() == Rail::LightningAddress, a.provider().ends_with("-mock")));
        adapters.into_iter().next()
    }
}

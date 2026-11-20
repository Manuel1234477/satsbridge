//! Stellar settlement layer.
//!
//! Every supported fiat currency has a token on Stellar (`NGN`, `KES`, ...) issued by our issuer
//! account, one token unit per unit of fiat held in our local accounts. A payment settles as one
//! atomic transaction: the issuer mints the sender's deposit to the treasury, and the treasury
//! converts it into the destination token with a strict-send path payment over the Stellar DEX
//! (via a bridge asset, `USD` by default), with the quoted payout as `destMin`. After the local
//! payout the destination tokens are burned (`Redeem`); if the payout fails the conversion is
//! undone (`Reverse`) and the sender is refunded in their own currency.
//!
//! - [`horizon::HorizonLedger`]: the real network (testnet), through Horizon.
//! - [`simulated::SimulatedLedger`]: an in-memory ledger priced from the FX feed, for tests and
//!   offline demos.
//!
//! Amounts on this interface are in stroops (7 decimal places), Stellar's native precision.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::AdapterError;
use crate::domain::Currency;

pub mod horizon;
pub mod keys;
pub mod pricing;
pub mod simulated;
pub mod tx;

pub const STROOPS_PER_UNIT: i64 = 10_000_000;
pub const TESTNET_PASSPHRASE: &str = "Test SDF Network ; September 2015";
pub const TESTNET_HORIZON: &str = "https://horizon-testnet.stellar.org";
pub const TESTNET_FRIENDBOT: &str = "https://friendbot.stellar.org";

/// A credit asset on Stellar, e.g. `NGN` issued by our issuer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StellarAsset {
    pub code: String,
    pub issuer: String,
}

impl StellarAsset {
    /// `CODE:ISSUER`, the form Horizon takes in `source_assets` / `destination_assets`.
    pub fn canonical(&self) -> String {
        format!("{}:{}", self.code, self.issuer)
    }
}

/// Result of a path search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathQuote {
    pub source_amount: i64,
    pub destination_amount: i64,
    /// Asset codes from source to destination, e.g. `["NGN", "USD", "KES"]`.
    pub hops: Vec<String>,
}

/// What a ledger transaction does. Amounts in stroops.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LedgerOp {
    /// Mint `mint` of `source` to the treasury, then convert `send` of it into at least
    /// `dest_min` of `destination`. One transaction, so it either all happens or none of it does.
    Settle {
        source: Currency,
        destination: Currency,
        mint: i64,
        send: i64,
        dest_min: i64,
    },
    /// Burn `amount` of `currency` from the treasury (the fiat has left our local account).
    Redeem { currency: Currency, amount: i64 },
    /// Undo a settlement: convert `amount` of `from` back into `to` and burn what comes back,
    /// and burn `burn` more of `to` (the fee kept from the original mint).
    Reverse {
        from: Currency,
        to: Currency,
        amount: i64,
        burn: i64,
    },
}

impl LedgerOp {
    pub fn kind(&self) -> &'static str {
        match self {
            LedgerOp::Settle { .. } => "settle",
            LedgerOp::Redeem { .. } => "redeem",
            LedgerOp::Reverse { .. } => "reverse",
        }
    }
}

/// A signed transaction, persisted before it is submitted so its outcome can always be found.
#[derive(Clone, Debug)]
pub struct PreparedTx {
    pub hash: String,
    pub envelope_xdr: String,
    /// The transaction's `maxTime`: if it isn't on the ledger by then, it never will be.
    pub valid_until: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TxOutcome {
    /// Applied successfully. `delivered` is the path payment's destination amount, if it had one.
    Succeeded { ledger: i64, delivered: Option<i64> },
    /// Definitely not applied (rejected, or applied with failing operations). Safe to rebuild.
    Failed { reason: String },
    /// Not found on the ledger (yet).
    NotFound,
}

#[async_trait]
pub trait StellarLedger: Send + Sync {
    /// `stellar-testnet` or `simulated`.
    fn name(&self) -> &'static str;

    fn network_passphrase(&self) -> &str;

    /// The token for a currency, or `None` if it has no asset on this ledger.
    fn asset(&self, currency: Currency) -> Option<StellarAsset>;

    /// The bridge asset paths route through (`USD`).
    fn bridge_asset(&self) -> StellarAsset;

    /// Treasury account that holds tokens in flight.
    fn treasury(&self) -> String;

    /// Link to a block explorer for a transaction hash.
    fn explorer_url(&self, _hash: &str) -> Option<String> {
        None
    }

    /// Best conversion of exactly `amount` of `source`.
    async fn quote_send(&self, source: Currency, amount: i64, destination: Currency)
    -> Result<PathQuote, AdapterError>;

    /// Cheapest way to deliver exactly `amount` of `destination`.
    async fn quote_receive(
        &self,
        source: Currency,
        destination: Currency,
        amount: i64,
    ) -> Result<PathQuote, AdapterError>;

    /// Builds and signs a transaction. `memo` ties it to the payment (max 28 bytes).
    async fn prepare(&self, op: &LedgerOp, memo: &str) -> Result<PreparedTx, AdapterError>;

    /// Submits a prepared transaction. `Err(Unavailable)` means the outcome is unknown: look it up.
    async fn submit(&self, tx: &PreparedTx) -> Result<TxOutcome, AdapterError>;

    async fn lookup(&self, hash: &str) -> Result<TxOutcome, AdapterError>;
}

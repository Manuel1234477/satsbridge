//! In-memory stand-in for the Stellar network, for tests and offline demos.
//!
//! Every currency trades against the bridge asset at the reference FX rate, minus `spread_bps`
//! per hop, like the market maker `stellar_setup` puts on testnet. Transactions apply atomically
//! with the same outcome rules as the real ledger (`destMin`, balances, time bounds), and tests
//! can inject failures and price moves.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use sha2::{Digest, Sha256};

use super::{LedgerOp, PathQuote, PreparedTx, StellarAsset, StellarLedger, TESTNET_PASSPHRASE, TxOutcome};
use crate::{
    adapters::{AdapterError, fx::FxProvider},
    domain::Currency,
};

pub const SIM_ISSUER: &str = "GSIMULATEDISSUER";
pub const SIM_TREASURY: &str = "GSIMULATEDTREASURY";

/// What the next `submit` does, for tests.
#[derive(Clone, Debug)]
pub enum Inject {
    /// Applies the transaction but reports an unknown outcome (Horizon timeout).
    ApplyButTimeout,
    /// Reports an unknown outcome and never applies the transaction.
    DropAndTimeout,
    /// Rejects the transaction with this reason.
    Reject(String),
}

#[derive(Default)]
struct State {
    pending: HashMap<String, (LedgerOp, PreparedTx)>,
    applied: HashMap<String, TxOutcome>,
    /// Treasury token balances, in stroops.
    treasury: HashMap<Currency, i64>,
    injections: VecDeque<Inject>,
    /// Applied to every conversion, e.g. -200 makes the DEX deliver 2% less.
    price_shift_bps: i64,
    ledger: i64,
    submissions: usize,
}

pub struct SimulatedLedger {
    fx: Arc<dyn FxProvider>,
    spread_bps: i64,
    tx_timeout_secs: i64,
    state: Mutex<State>,
}

impl SimulatedLedger {
    pub fn new(fx: Arc<dyn FxProvider>, spread_bps: i64, tx_timeout_secs: i64) -> Self {
        Self {
            fx,
            spread_bps,
            tx_timeout_secs,
            state: Mutex::new(State {
                ledger: 1_000,
                ..Default::default()
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("simulated ledger lock")
    }

    pub fn inject(&self, injection: Inject) {
        self.state().injections.push_back(injection);
    }

    pub fn set_price_shift_bps(&self, bps: i64) {
        self.state().price_shift_bps = bps;
    }

    /// Treasury balance of a token, in stroops.
    pub fn treasury_balance(&self, currency: Currency) -> i64 {
        self.state().treasury.get(&currency).copied().unwrap_or(0)
    }

    pub fn submissions(&self) -> usize {
        self.state().submissions
    }

    /// Destination stroops for `amount` source stroops at the current reference rates.
    async fn convert(&self, source: Currency, amount: i64, destination: Currency) -> Result<i64, AdapterError> {
        let (src, dst) = tokio::try_join!(self.fx.usd_rate(source), self.fx.usd_rate(destination))?;
        let shift = self.state().price_shift_bps;
        let hop = Decimal::from(10_000 - self.spread_bps) / Decimal::from(10_000);
        let shift = Decimal::from(10_000 + shift) / Decimal::from(10_000);
        let out = Decimal::from(amount) / src.rate * dst.rate * hop * hop * shift;
        out.floor()
            .to_i64()
            .ok_or_else(|| AdapterError::Rejected("amount out of range".into()))
    }

    fn hops(source: Currency, destination: Currency) -> Vec<String> {
        vec![source.code().into(), "USD".into(), destination.code().into()]
    }

    async fn apply(&self, op: &LedgerOp) -> Result<Option<i64>, String> {
        let balance = |s: &State, c: Currency| s.treasury.get(&c).copied().unwrap_or(0);
        match op {
            LedgerOp::Settle {
                source,
                destination,
                mint,
                send,
                dest_min,
            } => {
                if send > mint && balance(&self.state(), *source) + mint < *send {
                    return Err("tx_failed: path_payment_strict_send:Underfunded".into());
                }
                let out = self
                    .convert(*source, *send, *destination)
                    .await
                    .map_err(|e| format!("tx_failed: {e}"))?;
                if out < *dest_min {
                    return Err("tx_failed: payment:Success, path_payment_strict_send:UnderDestmin".into());
                }
                let mut s = self.state();
                *s.treasury.entry(*source).or_default() += mint - send;
                *s.treasury.entry(*destination).or_default() += out;
                Ok(Some(out))
            }
            LedgerOp::Redeem { currency, amount } => {
                let mut s = self.state();
                if balance(&s, *currency) < *amount {
                    return Err("tx_failed: payment:Underfunded".into());
                }
                *s.treasury.entry(*currency).or_default() -= amount;
                Ok(None)
            }
            LedgerOp::Reverse { from, to, amount, burn } => {
                {
                    let s = self.state();
                    if balance(&s, *from) < *amount || balance(&s, *to) < *burn {
                        return Err("tx_failed: path_payment_strict_send:Underfunded".into());
                    }
                }
                let out = self
                    .convert(*from, *amount, *to)
                    .await
                    .map_err(|e| format!("tx_failed: {e}"))?;
                let mut s = self.state();
                *s.treasury.entry(*from).or_default() -= amount;
                *s.treasury.entry(*to).or_default() -= burn;
                Ok(Some(out))
            }
        }
    }
}

#[async_trait]
impl StellarLedger for SimulatedLedger {
    fn name(&self) -> &'static str {
        "simulated"
    }

    fn network_passphrase(&self) -> &str {
        TESTNET_PASSPHRASE
    }

    fn asset(&self, currency: Currency) -> Option<StellarAsset> {
        Some(StellarAsset {
            code: currency.code().into(),
            issuer: SIM_ISSUER.into(),
        })
    }

    fn bridge_asset(&self) -> StellarAsset {
        StellarAsset {
            code: "USD".into(),
            issuer: SIM_ISSUER.into(),
        }
    }

    fn treasury(&self) -> String {
        SIM_TREASURY.into()
    }

    async fn quote_send(
        &self,
        source: Currency,
        amount: i64,
        destination: Currency,
    ) -> Result<PathQuote, AdapterError> {
        Ok(PathQuote {
            source_amount: amount,
            destination_amount: self.convert(source, amount, destination).await?,
            hops: Self::hops(source, destination),
        })
    }

    async fn quote_receive(
        &self,
        source: Currency,
        destination: Currency,
        amount: i64,
    ) -> Result<PathQuote, AdapterError> {
        // Invert the conversion, then step up until it really delivers `amount`.
        let unit = self.convert(source, 10_000_000_000, destination).await?.max(1);
        let mut needed = (i128::from(amount) * 10_000_000_000 / i128::from(unit)) as i64;
        while self.convert(source, needed, destination).await? < amount {
            needed += (needed / 100_000).max(1);
        }
        Ok(PathQuote {
            source_amount: needed,
            destination_amount: amount,
            hops: Self::hops(source, destination),
        })
    }

    async fn prepare(&self, op: &LedgerOp, memo: &str) -> Result<PreparedTx, AdapterError> {
        let mut s = self.state();
        s.ledger += 1;
        let digest = Sha256::digest(format!("{memo}:{op:?}:{}:{}", s.ledger, uuid::Uuid::new_v4()));
        let tx = PreparedTx {
            hash: hex::encode(digest),
            envelope_xdr: format!("simulated:{}", serde_json::to_string(op).unwrap_or_default()),
            valid_until: Utc::now() + Duration::seconds(self.tx_timeout_secs),
        };
        s.pending.insert(tx.hash.clone(), (op.clone(), tx.clone()));
        Ok(tx)
    }

    async fn submit(&self, prepared: &PreparedTx) -> Result<TxOutcome, AdapterError> {
        let (op, injection) = {
            let mut s = self.state();
            s.submissions += 1;
            if let Some(done) = s.applied.get(&prepared.hash) {
                return Ok(done.clone());
            }
            let Some((op, tx)) = s.pending.get(&prepared.hash).cloned() else {
                return Ok(TxOutcome::Failed {
                    reason: "tx_bad_seq".into(),
                });
            };
            let injection = s.injections.pop_front();
            // Lost on the way: never reaches the ledger.
            if matches!(injection, Some(Inject::DropAndTimeout)) {
                return Err(AdapterError::Unavailable("simulated timeout".into()));
            }
            if tx.valid_until <= Utc::now() {
                s.pending.remove(&prepared.hash);
                return Ok(TxOutcome::Failed {
                    reason: "tx_too_late".into(),
                });
            }
            (op, injection)
        };
        if let Some(Inject::Reject(reason)) = injection {
            // Rejected at submission: never applied, never recorded.
            self.state().pending.remove(&prepared.hash);
            return Ok(TxOutcome::Failed { reason });
        }
        let outcome = match self.apply(&op).await {
            Ok(delivered) => {
                let mut s = self.state();
                s.ledger += 1;
                TxOutcome::Succeeded {
                    ledger: s.ledger,
                    delivered,
                }
            }
            Err(reason) => TxOutcome::Failed { reason },
        };
        {
            let mut s = self.state();
            s.pending.remove(&prepared.hash);
            s.applied.insert(prepared.hash.clone(), outcome.clone());
        }
        if matches!(injection, Some(Inject::ApplyButTimeout)) {
            return Err(AdapterError::Unavailable("simulated timeout".into()));
        }
        Ok(outcome)
    }

    async fn lookup(&self, hash: &str) -> Result<TxOutcome, AdapterError> {
        Ok(self.state().applied.get(hash).cloned().unwrap_or(TxOutcome::NotFound))
    }
}

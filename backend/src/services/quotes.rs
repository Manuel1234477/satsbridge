//! Quotes priced from the Stellar DEX.
//!
//! The fee is taken in the source currency; the rest (`send_amount`) is converted on Stellar with
//! a strict-send path payment. The quoted payout is the DEX's current output minus a slippage
//! buffer (`SLIPPAGE_BPS`), and becomes the path payment's `destMin`: the recipient gets at
//! least that, or the settlement fails as a whole and the sender is refunded.

use chrono::{Duration, Utc};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::Deserialize;
use sqlx::types::Json;

use super::new_id;
use crate::{
    adapters::stellar::{
        PathQuote, StellarLedger,
        pricing::{minor_to_stroops, stroops_to_minor_ceil, stroops_to_minor_floor},
    },
    db::{self, AmountSide, Quote},
    domain::{Currency, Rail},
    error::{AppError, AppResult},
    state::AppState,
};

/// Upper bound on any input amount in minor units, to keep stroop math far from overflow.
const MAX_AMOUNT_MINOR: i64 = 100_000_000_000_000;

#[derive(Debug, Deserialize)]
pub struct CreateQuoteRequest {
    #[serde(default = "default_source")]
    pub source_currency: Currency,
    pub destination_currency: Currency,
    /// Minor units of the currency selected by `amount_side`.
    pub amount: i64,
    #[serde(default)]
    pub amount_side: AmountSide,
    /// Payout rail; defaults to the currency's bank/mobile-money rail.
    #[serde(default)]
    pub rail: Option<Rail>,
}

fn default_source() -> Currency {
    Currency::Ngn
}

#[derive(Debug, PartialEq, Eq)]
pub struct QuoteAmounts {
    pub source_amount: i64,
    pub fee_source_amount: i64,
    pub send_amount: i64,
    pub destination_amount: i64,
    pub exchange_rate: Decimal,
    pub hops: Vec<String>,
}

pub struct Fees {
    pub fee_bps: i64,
    pub flat_fee_minor: i64,
    pub slippage_bps: i64,
}

fn too_small() -> AppError {
    AppError::Validation("amount is too small to cover fees".into())
}

fn overflow() -> AppError {
    AppError::Validation("amount is too large".into())
}

fn bps_of(amount: i64, bps: i64) -> Option<i64> {
    (Decimal::from(amount) * Decimal::from(bps) / Decimal::from(10_000))
        .ceil()
        .to_i64()
}

/// Rounding always favours the platform by at most one minor unit.
pub async fn compute(
    ledger: &dyn StellarLedger,
    source: Currency,
    destination: Currency,
    amount: i64,
    side: AmountSide,
    fees: &Fees,
) -> AppResult<QuoteAmounts> {
    let keep = Decimal::from(10_000 - fees.slippage_bps) / Decimal::from(10_000);
    let (source_amount, fee, send, destination_amount, path): (i64, i64, i64, i64, PathQuote) = match side {
        AmountSide::Source => {
            let fee = bps_of(amount, fees.fee_bps).ok_or_else(overflow)? + fees.flat_fee_minor;
            let send = amount - fee;
            if send <= 0 {
                return Err(too_small());
            }
            let path = ledger
                .quote_send(
                    source,
                    minor_to_stroops(send, source).ok_or_else(overflow)?,
                    destination,
                )
                .await?;
            let buffered = (Decimal::from(path.destination_amount) * keep)
                .floor()
                .to_i64()
                .ok_or_else(overflow)?;
            (amount, fee, send, stroops_to_minor_floor(buffered, destination), path)
        }
        AmountSide::Destination => {
            let target = (Decimal::from(minor_to_stroops(amount, destination).ok_or_else(overflow)?) / keep)
                .ceil()
                .to_i64()
                .ok_or_else(overflow)?;
            let path = ledger.quote_receive(source, destination, target).await?;
            let send = stroops_to_minor_ceil(path.source_amount, source);
            let gross = (Decimal::from(send + fees.flat_fee_minor) * Decimal::from(10_000)
                / Decimal::from(10_000 - fees.fee_bps))
            .ceil()
            .to_i64()
            .ok_or_else(overflow)?;
            (gross, gross - send, send, amount, path)
        }
    };
    if destination_amount <= 0 {
        return Err(too_small());
    }
    // Units are stroops on both sides, so the ratio is destination units per source unit.
    let exchange_rate = if path.source_amount > 0 {
        (Decimal::from(path.destination_amount) / Decimal::from(path.source_amount))
            .round_dp(8)
            .normalize()
    } else {
        Decimal::ZERO
    };
    Ok(QuoteAmounts {
        source_amount,
        fee_source_amount: fee,
        send_amount: send,
        destination_amount,
        exchange_rate,
        hops: path.hops,
    })
}

pub async fn create_quote(state: &AppState, user_id: &str, req: CreateQuoteRequest) -> AppResult<Quote> {
    let (source, destination) = (req.source_currency, req.destination_currency);
    if source == destination {
        return Err(AppError::Validation(
            "source and destination currencies must differ".into(),
        ));
    }
    let collection = state
        .collections
        .get(source)
        .ok_or_else(|| AppError::Validation(format!("senders can't pay in {source} yet")))?;
    let adapter = match req.rail {
        Some(rail) => state.payouts.get(destination, rail),
        None => state.payouts.default_for(destination),
    }
    .ok_or_else(|| AppError::Validation(format!("payouts in {destination} are not supported on that rail")))?;
    if !(1..=MAX_AMOUNT_MINOR).contains(&req.amount) {
        return Err(AppError::Validation(
            "amount must be a positive integer in minor units".into(),
        ));
    }

    let fees = Fees {
        fee_bps: state.config.fee_bps,
        flat_fee_minor: state.config.flat_fee_minor,
        slippage_bps: state.config.slippage_bps,
    };
    let amounts = compute(
        state.ledger.as_ref(),
        source,
        destination,
        req.amount,
        req.amount_side,
        &fees,
    )
    .await?;

    if let Some(max) = collection.max_deposit_minor()
        && amounts.source_amount > max
    {
        return Err(AppError::Validation(format!(
            "deposit exceeds the {source} per-transaction limit of {max} minor units"
        )));
    }
    if let Some(max) = adapter.max_payout_minor()
        && amounts.destination_amount > max
    {
        return Err(AppError::Validation(format!(
            "payout exceeds the {destination} per-transaction limit of {max} minor units"
        )));
    }
    if let Some(min) = adapter.min_payout_minor()
        && amounts.destination_amount < min
    {
        return Err(AppError::Validation(format!(
            "payout is below the {destination} minimum of {min} minor units"
        )));
    }

    let now = Utc::now();
    let quote = Quote {
        id: new_id("quote"),
        user_id: Some(user_id.to_string()),
        source_currency: source,
        destination_currency: destination,
        destination_rail: adapter.rail(),
        payout_amount_guaranteed: adapter.guarantees_destination_amount(),
        amount_side: req.amount_side,
        source_amount: amounts.source_amount,
        fee_source_amount: amounts.fee_source_amount,
        fee_bps: state.config.fee_bps,
        send_amount: amounts.send_amount,
        destination_amount: amounts.destination_amount,
        exchange_rate: amounts.exchange_rate.to_string(),
        rate_source: state.ledger.name().to_string(),
        conversion_path: Json(amounts.hops),
        rate_as_of: now,
        expires_at: now + Duration::seconds(state.config.quote_ttl_secs),
        created_at: now,
    };
    db::insert_quote(&state.pool, &quote).await?;
    Ok(quote)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use super::*;
    use crate::adapters::{fx::StaticFxProvider, stellar::simulated::SimulatedLedger};

    fn ledger(spread_bps: i64) -> SimulatedLedger {
        let fx = StaticFxProvider::new(HashMap::from([
            (Currency::Ngn, Decimal::from(1_500)),
            (Currency::Kes, Decimal::from(130)),
        ]));
        SimulatedLedger::new(Arc::new(fx), spread_bps, 60)
    }

    fn fees() -> Fees {
        Fees {
            fee_bps: 100,
            flat_fee_minor: 0,
            slippage_bps: 50,
        }
    }

    #[tokio::test]
    async fn source_side_ngn_to_kes() {
        let q = compute(
            &ledger(0),
            Currency::Ngn,
            Currency::Kes,
            15_000_000,
            AmountSide::Source,
            &fees(),
        )
        .await
        .unwrap();
        // ₦150,000; 1% fee = ₦1,500; ₦148,500 = 99 USD = KSh 12,870; minus 0.5% = KSh 12,805.65.
        assert_eq!(q.fee_source_amount, 150_000);
        assert_eq!(q.send_amount, 14_850_000);
        assert_eq!(q.destination_amount, 1_280_565);
        assert_eq!(q.hops, ["NGN", "USD", "KES"]);
        assert_eq!(q.exchange_rate.to_string(), "0.08666667");
    }

    #[tokio::test]
    async fn destination_side_delivers_at_least_the_requested_amount() {
        let l = ledger(30);
        let q = compute(
            &l,
            Currency::Ngn,
            Currency::Kes,
            1_280_565,
            AmountSide::Destination,
            &fees(),
        )
        .await
        .unwrap();
        assert_eq!(q.destination_amount, 1_280_565);
        assert_eq!(q.source_amount - q.fee_source_amount, q.send_amount);
        let out = l
            .quote_send(
                Currency::Ngn,
                minor_to_stroops(q.send_amount, Currency::Ngn).unwrap(),
                Currency::Kes,
            )
            .await
            .unwrap();
        assert!(out.destination_amount >= minor_to_stroops(1_280_565, Currency::Kes).unwrap());
    }

    #[tokio::test]
    async fn rejects_amounts_that_do_not_cover_fees() {
        let f = Fees {
            flat_fee_minor: 1_000,
            ..fees()
        };
        assert!(
            compute(&ledger(0), Currency::Ngn, Currency::Kes, 500, AmountSide::Source, &f)
                .await
                .is_err()
        );
    }
}

use chrono::{Duration, Utc};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::Deserialize;

use super::new_id;
use crate::{
    db::{self, AmountSide, Quote},
    domain::{
        Currency, Rail,
        money::{SATS_PER_BTC, major_to_minor_ceil, major_to_minor_floor, minor_to_major},
    },
    error::{AppError, AppResult},
    state::AppState,
};

/// Upper bound on any input amount in minor units, to keep decimal math far from overflow.
const MAX_AMOUNT_MINOR: i64 = 1_000_000_000_000_000;

#[derive(Debug, Deserialize)]
pub struct CreateQuoteRequest {
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

#[derive(Debug, PartialEq, Eq)]
pub struct QuoteAmounts {
    pub source_amount: i64,
    pub destination_amount: i64,
    /// Invoice amount the sender pays.
    pub gross_sats: i64,
    pub fee_sats: i64,
    pub fee_source_amount: i64,
    pub exchange_rate: Decimal,
}

pub struct Pricing {
    /// Source-currency major units per 1 BTC.
    pub source_btc_price: Decimal,
    pub destination_btc_price: Decimal,
    pub fee_bps: i64,
    pub flat_fee_sats: i64,
}

fn too_small() -> AppError {
    AppError::Validation("amount is too small to cover fees".into())
}

fn overflow() -> AppError {
    AppError::Validation("amount is too large".into())
}

/// source amount -> BTC (sats) -> minus fee -> destination amount. Fees are taken in sats.
/// Rounding always favours the platform by at most one minor unit / one sat.
pub fn compute(
    source: Currency,
    destination: Currency,
    amount: i64,
    side: AmountSide,
    p: &Pricing,
) -> AppResult<QuoteAmounts> {
    let sats_per_btc = Decimal::from(SATS_PER_BTC);
    let bps = Decimal::from(p.fee_bps);
    let ten_k = Decimal::from(10_000);

    let (source_amount, destination_amount, gross_sats, fee_sats) = match side {
        AmountSide::Source => {
            let source_major = minor_to_major(amount, source);
            let gross = (source_major * sats_per_btc / p.source_btc_price)
                .floor()
                .to_i64()
                .ok_or_else(overflow)?;
            let fee = (Decimal::from(gross) * bps / ten_k)
                .ceil()
                .to_i64()
                .ok_or_else(overflow)?
                + p.flat_fee_sats;
            let net = gross - fee;
            if net <= 0 {
                return Err(too_small());
            }
            let destination_major = Decimal::from(net) * p.destination_btc_price / sats_per_btc;
            let destination_amount = major_to_minor_floor(destination_major, destination).ok_or_else(overflow)?;
            (amount, destination_amount, gross, fee)
        }
        AmountSide::Destination => {
            let destination_major = minor_to_major(amount, destination);
            let net = (destination_major * sats_per_btc / p.destination_btc_price)
                .ceil()
                .to_i64()
                .ok_or_else(overflow)?;
            let gross = (Decimal::from(net + p.flat_fee_sats) * ten_k / (ten_k - bps))
                .ceil()
                .to_i64()
                .ok_or_else(overflow)?;
            let source_major = Decimal::from(gross) * p.source_btc_price / sats_per_btc;
            let source_amount = major_to_minor_ceil(source_major, source).ok_or_else(overflow)?;
            (source_amount, amount, gross, gross - net)
        }
    };
    if destination_amount <= 0 {
        return Err(too_small());
    }

    let fee_source_amount = major_to_minor_floor(Decimal::from(fee_sats) * p.source_btc_price / sats_per_btc, source)
        .ok_or_else(overflow)?;
    let exchange_rate = (p.destination_btc_price / p.source_btc_price).round_dp(8).normalize();

    Ok(QuoteAmounts {
        source_amount,
        destination_amount,
        gross_sats,
        fee_sats,
        fee_source_amount,
        exchange_rate,
    })
}

pub async fn create_quote(state: &AppState, req: CreateQuoteRequest) -> AppResult<Quote> {
    let (source, destination) = (req.source_currency, req.destination_currency);
    if source == destination {
        return Err(AppError::Validation(
            "source and destination currencies must differ".into(),
        ));
    }
    if !source.can_send() {
        return Err(AppError::Validation(format!(
            "sending from {source} is not supported yet"
        )));
    }
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

    let (source_price, destination_price) =
        tokio::try_join!(state.fx.btc_price(source), state.fx.btc_price(destination))?;
    let rate_source = if source_price.source == destination_price.source {
        source_price.source.clone()
    } else {
        format!("{}+{}", source_price.source, destination_price.source)
    };
    let (source_btc_price, destination_btc_price) = (source_price.price, destination_price.price);
    let pricing = Pricing {
        source_btc_price,
        destination_btc_price,
        fee_bps: state.config.fee_bps,
        flat_fee_sats: state.config.flat_fee_sats,
    };
    let amounts = compute(source, destination, req.amount, req.amount_side, &pricing)?;

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
        source_currency: source,
        destination_currency: destination,
        destination_rail: adapter.rail(),
        payout_amount_guaranteed: adapter.guarantees_destination_amount(),
        amount_side: req.amount_side,
        source_amount: amounts.source_amount,
        destination_amount: amounts.destination_amount,
        amount_sats: amounts.gross_sats,
        fee_sats: amounts.fee_sats,
        fee_source_amount: amounts.fee_source_amount,
        fee_bps: state.config.fee_bps,
        exchange_rate: amounts.exchange_rate.to_string(),
        rate_source,
        rate_as_of: source_price.as_of.min(destination_price.as_of),
        expires_at: now + Duration::seconds(state.config.quote_ttl_secs),
        created_at: now,
    };
    db::insert_quote(&state.pool, &quote).await?;
    Ok(quote)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pricing() -> Pricing {
        Pricing {
            source_btc_price: Decimal::from(60_000_000),       // XOF per BTC
            destination_btc_price: Decimal::from(150_000_000), // NGN per BTC
            fee_bps: 100,
            flat_fee_sats: 0,
        }
    }

    #[test]
    fn source_side_xof_to_ngn() {
        let q = compute(Currency::Xof, Currency::Ngn, 100_000, AmountSide::Source, &pricing()).unwrap();
        // 100,000 XOF = 166,666 sats; 1% fee = 1,667 sats; 164,999 sats = ₦247,498.50.
        assert_eq!(q.gross_sats, 166_666);
        assert_eq!(q.fee_sats, 1_667);
        assert_eq!(q.destination_amount, 24_749_850);
        assert_eq!(q.fee_source_amount, 1_000);
        assert_eq!(q.exchange_rate.to_string(), "2.5");
    }

    #[test]
    fn destination_side_delivers_at_least_the_requested_amount() {
        let q = compute(
            Currency::Xof,
            Currency::Ngn,
            24_749_850,
            AmountSide::Destination,
            &pricing(),
        )
        .unwrap();
        assert_eq!(q.destination_amount, 24_749_850);
        let net_sats = q.gross_sats - q.fee_sats;
        let delivered = Decimal::from(net_sats) * Decimal::from(150_000_000) / Decimal::from(SATS_PER_BTC);
        assert!(delivered * Decimal::from(100) >= Decimal::from(24_749_850));
    }

    #[test]
    fn rejects_amounts_that_do_not_cover_fees() {
        let p = Pricing {
            flat_fee_sats: 1_000,
            ..pricing()
        };
        assert!(compute(Currency::Xof, Currency::Ngn, 100, AmountSide::Source, &p).is_err());
    }
}

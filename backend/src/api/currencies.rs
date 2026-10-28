use axum::{Json, extract::State};
use serde::Serialize;

use crate::{
    domain::{Currency, Rail},
    state::AppState,
};

#[derive(Serialize)]
pub struct CurrencyInfo {
    code: Currency,
    name: &'static str,
    /// Decimal places of the minor unit; all API amounts are integers in minor units.
    exponent: u32,
    can_send: bool,
    /// Available payout rails; the first is the default when a quote names none.
    payout_rails: Vec<PayoutInfo>,
}

#[derive(Serialize)]
pub struct PayoutInfo {
    rail: Rail,
    provider: &'static str,
    min_payout_minor: Option<i64>,
    max_payout_minor: Option<i64>,
    /// False when the provider converts at its own rate, so quoted amounts are estimates.
    guaranteed_amount: bool,
    /// Countries served, for mobile money (XOF and XAF span several countries).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    countries: Vec<String>,
}

pub async fn list(State(state): State<AppState>) -> Json<Vec<CurrencyInfo>> {
    let currencies = Currency::ALL
        .into_iter()
        .map(|code| CurrencyInfo {
            code,
            name: code.name(),
            exponent: code.exponent(),
            can_send: code.can_send(),
            payout_rails: {
                let mut rails: Vec<_> = state
                    .payouts
                    .for_currency(code)
                    .map(|a| PayoutInfo {
                        rail: a.rail(),
                        provider: a.provider(),
                        min_payout_minor: a.min_payout_minor(),
                        max_payout_minor: a.max_payout_minor(),
                        guaranteed_amount: a.guarantees_destination_amount(),
                        countries: a.countries(),
                    })
                    .collect();
                rails.sort_by_key(|r| (r.rail == Rail::LightningAddress, r.provider.ends_with("-mock")));
                rails
            },
        })
        .collect();
    Json(currencies)
}

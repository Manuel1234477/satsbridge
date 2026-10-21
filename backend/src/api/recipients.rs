use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;

use crate::{
    adapters::payout::Institution,
    domain::{Currency, Rail},
    error::{AppError, AppResult},
    services::recipients::{self, ResolveRequest, ResolveResponse},
    state::AppState,
};

#[derive(Deserialize)]
pub struct InstitutionParams {
    currency: Currency,
    rail: Option<Rail>,
}

pub async fn institutions(
    State(state): State<AppState>,
    Query(params): Query<InstitutionParams>,
) -> AppResult<Json<Vec<Institution>>> {
    let adapter = match params.rail {
        Some(rail) => state.payouts.get(params.currency, rail),
        None => state.payouts.default_for(params.currency),
    }
    .ok_or_else(|| AppError::Validation(format!("payouts in {} are not supported on that rail", params.currency)))?;
    Ok(Json(adapter.institutions().await?))
}

pub async fn resolve(
    State(state): State<AppState>,
    Json(req): Json<ResolveRequest>,
) -> AppResult<Json<ResolveResponse>> {
    Ok(Json(recipients::resolve(&state, req).await?))
}

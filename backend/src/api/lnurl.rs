use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use serde_json::Value;

use crate::{services::lnurl, state::AppState};

pub async fn pay_request(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    Json(lnurl::pay_request(&state, &id).await)
}

#[derive(Deserialize)]
pub struct CallbackParams {
    /// Millisats.
    amount: Option<i64>,
}

pub async fn callback(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<CallbackParams>,
) -> Json<Value> {
    Json(lnurl::callback(&state, &id, params.amount).await)
}

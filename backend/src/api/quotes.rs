use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::{
    db::{self, Quote},
    error::{AppError, AppResult},
    services::quotes::{self, CreateQuoteRequest},
    state::AppState,
};

pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateQuoteRequest>,
) -> AppResult<(StatusCode, Json<Quote>)> {
    Ok((StatusCode::CREATED, Json(quotes::create_quote(&state, req).await?)))
}

pub async fn get(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Json<Quote>> {
    db::get_quote(&state.pool, &id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound("quote"))
}

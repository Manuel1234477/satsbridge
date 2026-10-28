use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};

use super::CurrentUser;
use crate::{
    db::{self, Quote},
    error::{AppError, AppResult},
    services::quotes::{self, CreateQuoteRequest},
    state::AppState,
};

pub async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Json(req): Json<CreateQuoteRequest>,
) -> AppResult<(StatusCode, Json<Quote>)> {
    Ok((
        StatusCode::CREATED,
        Json(quotes::create_quote(&state, &user.0, req).await?),
    ))
}

pub async fn get(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> AppResult<Json<Quote>> {
    db::get_quote(&state.pool, &id)
        .await?
        .filter(|q| q.user_id.as_deref() == Some(user.0.as_str()))
        .map(Json)
        .ok_or(AppError::NotFound("quote"))
}

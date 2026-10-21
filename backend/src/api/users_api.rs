use axum::{Json, extract::State, http::StatusCode};

use crate::{
    error::AppResult,
    services::users::{self, CreateUserRequest, CreatedUser},
    state::AppState,
};

pub async fn create(
    State(state): State<AppState>,
    body: Option<Json<CreateUserRequest>>,
) -> AppResult<(StatusCode, Json<CreatedUser>)> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    Ok((StatusCode::CREATED, Json(users::create_user(&state, req).await?)))
}

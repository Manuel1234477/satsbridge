//! Users authenticate with a bearer token issued once at sign-up; only its SHA-256 is stored.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::new_id;
use crate::{
    db::{self, User},
    error::{AppError, AppResult},
    state::AppState,
};

#[derive(Debug, Deserialize, Default)]
pub struct CreateUserRequest {
    /// Optional email or phone, for support. Not verified.
    #[serde(default)]
    pub contact: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreatedUser {
    pub user_id: String,
    /// Shown once. Send as `Authorization: Bearer <token>`.
    pub token: String,
    pub created_at: DateTime<Utc>,
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub async fn create_user(state: &AppState, req: CreateUserRequest) -> AppResult<CreatedUser> {
    let contact = req.contact.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
    if contact.as_ref().is_some_and(|c| c.len() > 200) {
        return Err(AppError::Validation("contact is too long".into()));
    }
    let token = format!("sbu_{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let user = User {
        id: new_id("usr"),
        contact,
        token_hash: hash_token(&token),
        created_at: Utc::now(),
    };
    db::insert_user(&state.pool, &user).await?;
    Ok(CreatedUser {
        user_id: user.id,
        token,
        created_at: user.created_at,
    })
}

pub async fn authenticate(state: &AppState, token: &str) -> AppResult<Option<User>> {
    Ok(db::get_user_by_token_hash(&state.pool, &hash_token(token)).await?)
}

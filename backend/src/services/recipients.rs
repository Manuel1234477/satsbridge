use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::new_id;
use crate::{
    adapters::{
        AdapterError,
        payout::{PayoutAdapter, ResolvedRecipient},
    },
    db::{self, RecipientResolution},
    domain::{Currency, Rail, Recipient, recipient::mask},
    error::{AppError, AppResult},
    security::hash_account,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct ResolveRequest {
    pub currency: Currency,
    pub bank_code: String,
    pub account_number: String,
}

#[derive(Debug, Serialize)]
pub struct ResolveResponse {
    pub resolution_id: String,
    pub account_name: String,
    pub bank_code: String,
    pub account_number_masked: String,
    pub expires_at: DateTime<Utc>,
}

/// NGN account-name enquiry (addendum FR-NGN-03). The sender must see and confirm the name.
pub async fn resolve(state: &AppState, req: ResolveRequest) -> AppResult<ResolveResponse> {
    if req.currency != Currency::Ngn {
        return Err(AppError::Validation(format!(
            "account resolution is not used for {}",
            req.currency
        )));
    }
    let adapter = state
        .payouts
        .get(Currency::Ngn, Rail::NgBankTransfer)
        .ok_or_else(|| AppError::Validation("NGN bank payouts are not configured".into()))?;
    let recipient = Recipient::NgBankTransfer {
        bank_code: req.bank_code.clone(),
        account_number: req.account_number.clone(),
        resolution_id: None,
    };
    let account_name = adapter
        .resolve_recipient(&recipient)
        .await?
        .account_name
        .ok_or(AdapterError::RecipientNotFound)?;

    let now = Utc::now();
    let resolution = RecipientResolution {
        id: new_id("res"),
        currency: Currency::Ngn,
        bank_code: req.bank_code.clone(),
        account_number_hash: hash_account(&req.bank_code, &req.account_number),
        account_name: account_name.clone(),
        provider: adapter.provider().to_string(),
        created_at: now,
        expires_at: now + Duration::seconds(state.config.resolution_ttl_secs),
    };
    db::insert_resolution(&state.pool, &resolution).await?;

    Ok(ResolveResponse {
        resolution_id: resolution.id,
        account_name,
        bank_code: req.bank_code,
        account_number_masked: mask(&req.account_number),
        expires_at: resolution.expires_at,
    })
}

/// Validates a recipient for a new payment: account holder name if known, plus amount bounds.
/// NGN bank recipients must carry a fresh `resolution_id` matching the same bank and account.
pub async fn verify_for_payment(
    state: &AppState,
    adapter: &dyn PayoutAdapter,
    recipient: &Recipient,
) -> AppResult<ResolvedRecipient> {
    match recipient {
        Recipient::NgBankTransfer {
            bank_code,
            account_number,
            resolution_id,
        } => {
            let id = resolution_id.as_deref().ok_or_else(|| {
                AppError::Validation("resolution_id is required; call /api/v1/recipients/resolve first".into())
            })?;
            let resolution = db::get_resolution(&state.pool, id)
                .await?
                .ok_or_else(|| AppError::Validation("unknown resolution_id".into()))?;
            if resolution.currency != Currency::Ngn
                || resolution.provider != adapter.provider()
                || resolution.bank_code != *bank_code
                || resolution.account_number_hash != hash_account(bank_code, account_number)
            {
                return Err(AppError::Validation(
                    "resolution_id does not match this recipient".into(),
                ));
            }
            if resolution.expires_at <= Utc::now() {
                return Err(AppError::Validation(
                    "account resolution has expired; resolve again".into(),
                ));
            }
            Ok(ResolvedRecipient {
                account_name: Some(resolution.account_name),
                ..Default::default()
            })
        }
        other => Ok(adapter.resolve_recipient(other).await?),
    }
}

//! Stand-in for an NGN virtual-account collection provider (Paystack/Flutterwave-style dedicated
//! accounts). Each payment gets its own 10-digit account number, derived from the payment ID.
//! Deposits are reported by `POST /api/v1/dev/payments/{id}/deposit` (demo) or by a signed
//! `POST /api/v1/webhooks/collections`.

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use super::{CollectionAdapter, DepositInstructions, DepositRequest};
use crate::{adapters::AdapterError, domain::Currency};

pub const MOCK_BANK: &str = "Sandbox Microfinance Bank";

pub struct MockNgnCollection {
    max_deposit_minor: i64,
}

impl MockNgnCollection {
    pub fn new(max_deposit_minor: i64) -> Self {
        Self { max_deposit_minor }
    }
}

/// Deterministic per-payment NUBAN, so repeated calls return the same account.
pub fn virtual_account(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    let n = u64::from_be_bytes(digest[..8].try_into().expect("8 bytes")) % 1_000_000_000;
    format!("9{n:09}")
}

#[async_trait]
impl CollectionAdapter for MockNgnCollection {
    fn provider(&self) -> &'static str {
        "ngn-collection-mock"
    }

    fn currency(&self) -> Currency {
        Currency::Ngn
    }

    fn max_deposit_minor(&self) -> Option<i64> {
        Some(self.max_deposit_minor)
    }

    async fn create_deposit(&self, request: DepositRequest<'_>) -> Result<DepositInstructions, AdapterError> {
        if request.currency != Currency::Ngn {
            return Err(AdapterError::Rejected("only NGN deposits are collected".into()));
        }
        if request.amount_minor > self.max_deposit_minor {
            return Err(AdapterError::Rejected("amount exceeds the NGN deposit limit".into()));
        }
        let account_number = virtual_account(request.idempotency_key);
        Ok(DepositInstructions {
            bank_name: MOCK_BANK.into(),
            account_name: format!("SATSBRIDGE / {}", &account_number[4..]),
            reference: account_number.clone(),
            account_number,
            provider_reference: Some(format!("mockva_{}", request.idempotency_key)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_accounts_are_stable_nubans() {
        let a = virtual_account("pay_1");
        assert_eq!(a, virtual_account("pay_1"));
        assert_ne!(a, virtual_account("pay_2"));
        assert_eq!(a.len(), 10);
        assert!(a.bytes().all(|b| b.is_ascii_digit()));
    }
}

//! Stand-in for an NGN payout provider over NIBSS NIP (docs/prd-addendum-ngn-payout.md).
//!
//! Demo behaviour: account `0000000000` does not resolve; every other valid NUBAN at a listed
//! institution resolves to `DEMO RECIPIENT <last4>`.

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;

use super::{Institution, InstitutionKind, PayoutAccepted, PayoutAdapter, PayoutRequest, ResolvedRecipient};
use crate::{
    adapters::AdapterError,
    domain::{Currency, Rail, Recipient},
};

/// Demo subset of NIP institution codes.
const INSTITUTIONS: &[(&str, &str, bool)] = &[
    ("044", "Access Bank", false),
    ("011", "First Bank of Nigeria", false),
    ("058", "Guaranty Trust Bank", false),
    ("033", "United Bank for Africa", false),
    ("057", "Zenith Bank", false),
    ("50211", "Kuda", true),
    ("50515", "Moniepoint", true),
    ("999992", "OPay", true),
    ("999991", "PalmPay", true),
];

pub struct MockNgnAdapter {
    max_payout_minor: i64,
    submissions: AtomicUsize,
}

impl MockNgnAdapter {
    pub fn new(max_payout_minor: i64) -> Self {
        Self {
            max_payout_minor,
            submissions: AtomicUsize::new(0),
        }
    }

    /// Number of `create_payout` calls, for asserting exactly-once payouts in tests.
    pub fn submissions(&self) -> usize {
        self.submissions.load(Ordering::SeqCst)
    }
}

pub fn is_valid_nuban(account_number: &str) -> bool {
    account_number.len() == 10 && account_number.bytes().all(|b| b.is_ascii_digit())
}

fn validate(recipient: &Recipient) -> Result<(&str, &str), AdapterError> {
    let Recipient::NgBankTransfer {
        bank_code,
        account_number,
        ..
    } = recipient
    else {
        return Err(AdapterError::InvalidRecipient(
            "expected an NG_BANK_TRANSFER recipient".into(),
        ));
    };
    if !INSTITUTIONS.iter().any(|(code, ..)| code == bank_code) {
        return Err(AdapterError::InvalidRecipient(format!("unknown bank code {bank_code}")));
    }
    if !is_valid_nuban(account_number) {
        return Err(AdapterError::InvalidRecipient(
            "account number must be 10 digits".into(),
        ));
    }
    Ok((bank_code, account_number))
}

#[async_trait]
impl PayoutAdapter for MockNgnAdapter {
    fn provider(&self) -> &'static str {
        "ngn-mock"
    }

    fn currency(&self) -> Currency {
        Currency::Ngn
    }

    fn rail(&self) -> Rail {
        Rail::NgBankTransfer
    }

    fn max_payout_minor(&self) -> Option<i64> {
        Some(self.max_payout_minor)
    }

    async fn institutions(&self) -> Result<Vec<Institution>, AdapterError> {
        Ok(INSTITUTIONS
            .iter()
            .map(|(code, name, wallet)| Institution {
                code: (*code).into(),
                name: (*name).into(),
                kind: if *wallet {
                    InstitutionKind::Wallet
                } else {
                    InstitutionKind::Bank
                },
            })
            .collect())
    }

    async fn resolve_recipient(&self, recipient: &Recipient) -> Result<ResolvedRecipient, AdapterError> {
        let (_, account_number) = validate(recipient)?;
        if account_number == "0000000000" {
            return Err(AdapterError::RecipientNotFound);
        }
        Ok(ResolvedRecipient {
            account_name: Some(format!("DEMO RECIPIENT {}", &account_number[6..])),
            ..Default::default()
        })
    }

    async fn create_payout(&self, request: PayoutRequest<'_>) -> Result<PayoutAccepted, AdapterError> {
        self.submissions.fetch_add(1, Ordering::SeqCst);
        validate(request.recipient)?;
        if request.amount_minor > self.max_payout_minor {
            return Err(AdapterError::Rejected("amount exceeds the NGN payout limit".into()));
        }
        Ok(PayoutAccepted {
            provider_reference: format!("mockngn_{}", request.idempotency_key),
            completed: false,
        })
    }
}

//! Stand-ins for collection providers, one per source currency:
//!
//! - NGN: Paystack/Flutterwave-style dedicated virtual bank account per payment.
//! - KES: M-Pesa Paybill with a per-payment account reference.
//! - GHS, XOF, XAF, GNF: mobile money merchant number with a per-payment reference.
//!
//! Deposits are reported by `POST /api/v1/dev/payments/{id}/deposit` (demo) or by a signed
//! `POST /api/v1/webhooks/collections`.

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use stellarbridge::adapters::collection::{CollectionAdapter, DepositInstructions, DepositMethod, DepositRequest};
use stellarbridge::{
    adapters::AdapterError,
    domain::{Currency, Recipient},
};

pub const MOCK_BANK: &str = "Sandbox Microfinance Bank";

/// Deterministic per-payment digits, so repeated calls return the same instructions.
fn digits(key: &str, n: u32) -> String {
    let digest = Sha256::digest(key.as_bytes());
    let v = u64::from_be_bytes(digest[..8].try_into().expect("8 bytes")) % 10u64.pow(n);
    format!("{v:0width$}", width = n as usize)
}

/// Deterministic per-payment NUBAN.
pub fn virtual_account(key: &str) -> String {
    format!("9{}", digits(key, 9))
}

pub struct MockCollection {
    currency: Currency,
    max_deposit_minor: i64,
}

impl MockCollection {
    pub fn new(currency: Currency, max_deposit_minor: i64) -> Self {
        Self {
            currency,
            max_deposit_minor,
        }
    }

    /// One mock rail per supported source currency. `ngn_max_deposit_minor` caps NGN; the
    /// mobile money rails take roughly $3,000.
    pub fn all(ngn_max_deposit_minor: i64) -> Vec<Self> {
        vec![
            Self::new(Currency::Ngn, ngn_max_deposit_minor),
            Self::new(Currency::Kes, 40_000_000),
            Self::new(Currency::Ghs, 5_000_000),
            Self::new(Currency::Xof, 2_000_000),
            Self::new(Currency::Xaf, 2_000_000),
            Self::new(Currency::Gnf, 30_000_000),
        ]
    }
}

/// The account a demo deposit "comes from", in the currency's own payout format, so that
/// refunds can go back to it.
pub fn demo_sender(currency: Currency) -> Recipient {
    let mobile = |country: &str, provider: &str, phone: &str, name: &str| Recipient::MobileMoney {
        country: country.into(),
        provider_code: provider.into(),
        phone_number: phone.into(),
        account_name: Some(name.into()),
    };
    match currency {
        Currency::Ngn => Recipient::NgBankTransfer {
            bank_code: "058".into(),
            account_number: "0123456789".into(),
            resolution_id: None,
        },
        Currency::Kes => Recipient::KeMobileMoney {
            phone_number: "0722000111".into(),
        },
        Currency::Ghs => mobile("GH", "MTN", "0244000111", "DEMO SENDER"),
        Currency::Xof => mobile("SN", "WAVE", "771000111", "DEMO SENDER"),
        Currency::Xaf => mobile("CM", "MTN", "677000111", "DEMO SENDER"),
        Currency::Gnf => mobile("GN", "ORANGE", "622000111", "DEMO SENDER"),
    }
}

#[async_trait]
impl CollectionAdapter for MockCollection {
    fn provider(&self) -> &'static str {
        match self.currency {
            Currency::Ngn => "ngn-collection-mock",
            Currency::Kes => "mpesa-collection-mock",
            _ => "momo-collection-mock",
        }
    }

    fn currency(&self) -> Currency {
        self.currency
    }

    fn max_deposit_minor(&self) -> Option<i64> {
        Some(self.max_deposit_minor)
    }

    async fn create_deposit(&self, request: DepositRequest<'_>) -> Result<DepositInstructions, AdapterError> {
        if request.currency != self.currency {
            return Err(AdapterError::Rejected(format!("this rail collects {}", self.currency)));
        }
        if request.amount_minor > self.max_deposit_minor {
            return Err(AdapterError::Rejected(format!(
                "amount exceeds the {} deposit limit",
                self.currency
            )));
        }
        let key = request.idempotency_key;
        let provider_reference = Some(format!("mockcol_{key}"));
        Ok(match self.currency {
            Currency::Ngn => {
                let account_number = virtual_account(key);
                DepositInstructions {
                    method: DepositMethod::BankTransfer,
                    bank_name: MOCK_BANK.into(),
                    account_name: format!("STELLARBRIDGE / {}", &account_number[4..]),
                    reference: account_number.clone(),
                    account_number,
                    provider_reference,
                }
            }
            Currency::Kes => DepositInstructions {
                method: DepositMethod::MobileMoney,
                bank_name: "M-Pesa Paybill".into(),
                account_number: "400200".into(),
                account_name: "STELLARBRIDGE".into(),
                reference: format!("SB{}", digits(key, 8)),
                provider_reference,
            },
            other => DepositInstructions {
                method: DepositMethod::MobileMoney,
                bank_name: match other {
                    Currency::Ghs => "MTN MoMo, Telecel Cash or AirtelTigo Money",
                    Currency::Gnf => "Orange Money or MTN MoMo",
                    _ => "Orange Money, MTN MoMo, Wave or Moov Money",
                }
                .into(),
                account_number: "700100".into(),
                account_name: "STELLARBRIDGE".into(),
                reference: format!("SB{}", digits(key, 8)),
                provider_reference,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn virtual_accounts_are_stable_nubans() {
        let a = virtual_account("pay_1");
        assert_eq!(a, virtual_account("pay_1"));
        assert_ne!(a, virtual_account("pay_2"));
        assert_eq!(a.len(), 10);
        assert!(a.bytes().all(|b| b.is_ascii_digit()));
    }

    #[tokio::test]
    async fn every_currency_has_a_rail_with_its_own_method() {
        for rail in MockCollection::all(1_000_000_000) {
            let c = rail.currency();
            let req = |key| DepositRequest {
                idempotency_key: key,
                currency: c,
                amount_minor: 1_000,
                expires_at: Utc::now(),
            };
            let a = rail.create_deposit(req("pay_1")).await.unwrap();
            assert_eq!(a.reference, rail.create_deposit(req("pay_1")).await.unwrap().reference);
            assert_ne!(a.reference, rail.create_deposit(req("pay_2")).await.unwrap().reference);
            let expected = if c == Currency::Ngn {
                DepositMethod::BankTransfer
            } else {
                DepositMethod::MobileMoney
            };
            assert_eq!(a.method, expected, "{c}");
        }
    }
}

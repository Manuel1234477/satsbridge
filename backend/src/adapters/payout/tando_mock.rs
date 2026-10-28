//! Stand-in for the Tando KES / M-Pesa payout integration.

use async_trait::async_trait;

use super::{Institution, InstitutionKind, PayoutAccepted, PayoutAdapter, PayoutRequest, ResolvedRecipient};
use crate::{
    adapters::AdapterError,
    domain::{Currency, Rail, Recipient},
};

pub struct MockTandoAdapter;

/// Accepts `+2547XXXXXXXX`, `2547XXXXXXXX`, `07XXXXXXXX` (and the `1` prefixes); returns `254XXXXXXXXX`.
pub fn normalize_ke_msisdn(input: &str) -> Option<String> {
    let digits: String = input.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    let rest = digits
        .strip_prefix("+254")
        .or_else(|| digits.strip_prefix("254"))
        .or_else(|| digits.strip_prefix('0'))?;
    let valid =
        rest.len() == 9 && rest.bytes().all(|b| b.is_ascii_digit()) && (rest.starts_with('7') || rest.starts_with('1'));
    valid.then(|| format!("254{rest}"))
}

fn phone_number(recipient: &Recipient) -> Result<String, AdapterError> {
    match recipient {
        Recipient::KeMobileMoney { phone_number } => normalize_ke_msisdn(phone_number)
            .ok_or_else(|| AdapterError::InvalidRecipient("invalid Kenyan mobile number".into())),
        _ => Err(AdapterError::InvalidRecipient(
            "expected a KE_MOBILE_MONEY recipient".into(),
        )),
    }
}

#[async_trait]
impl PayoutAdapter for MockTandoAdapter {
    fn provider(&self) -> &'static str {
        "tando-mock"
    }

    fn currency(&self) -> Currency {
        Currency::Kes
    }

    fn rail(&self) -> Rail {
        Rail::KeMobileMoney
    }

    async fn institutions(&self, _country: Option<&str>) -> Result<Vec<Institution>, AdapterError> {
        Ok(vec![Institution {
            code: "MPESA".into(),
            name: "M-Pesa".into(),
            kind: InstitutionKind::MobileMoney,
        }])
    }

    async fn resolve_recipient(&self, recipient: &Recipient) -> Result<ResolvedRecipient, AdapterError> {
        phone_number(recipient)?;
        Ok(ResolvedRecipient::default())
    }

    async fn create_payout(&self, request: PayoutRequest<'_>) -> Result<PayoutAccepted, AdapterError> {
        phone_number(request.recipient)?;
        Ok(PayoutAccepted {
            provider_reference: format!("mocktando_{}", request.idempotency_key),
            completed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_ke_msisdn;

    #[test]
    fn normalizes_kenyan_numbers() {
        assert_eq!(normalize_ke_msisdn("0712 345 678").as_deref(), Some("254712345678"));
        assert_eq!(normalize_ke_msisdn("+254112345678").as_deref(), Some("254112345678"));
        assert_eq!(normalize_ke_msisdn("0812345678"), None);
        assert_eq!(normalize_ke_msisdn("07123"), None);
    }
}

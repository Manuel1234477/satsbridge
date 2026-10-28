//! Offline stand-in for mobile money payouts (XOF, XAF), so those corridors can be demoed
//! without Bitnob. Accepts 8-12 digit numbers; uses the given name or a demo one.

use async_trait::async_trait;

use super::{Institution, InstitutionKind, PayoutAccepted, PayoutAdapter, PayoutRequest, ResolvedRecipient};
use crate::{
    adapters::AdapterError,
    domain::{Currency, Rail, Recipient, currency::currency_for_country, recipient::mask},
};

const PROVIDERS: &[(&str, &str)] = &[
    ("ORANGE", "Orange Money"),
    ("MTN", "MTN MoMo"),
    ("WAVE", "Wave"),
    ("MOOV", "Moov Money"),
];

pub struct MockMobileMoneyAdapter {
    currency: Currency,
}

impl MockMobileMoneyAdapter {
    pub fn new(currency: Currency) -> Self {
        Self { currency }
    }

    fn validate<'a>(&self, recipient: &'a Recipient) -> Result<(&'a str, Option<&'a str>), AdapterError> {
        let invalid = |m: &str| AdapterError::InvalidRecipient(m.into());
        let Recipient::MobileMoney {
            country,
            provider_code,
            phone_number,
            account_name,
        } = recipient
        else {
            return Err(invalid("expected a MOBILE_MONEY recipient"));
        };
        if currency_for_country(country) != Some(self.currency) {
            return Err(invalid(&format!("{country} doesn't pay out in {}", self.currency)));
        }
        if !PROVIDERS
            .iter()
            .any(|(code, _)| code.eq_ignore_ascii_case(provider_code))
        {
            return Err(invalid(&format!("unknown provider {provider_code}")));
        }
        if !(8..=12).contains(&phone_number.len()) || !phone_number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("phone number must be 8-12 digits"));
        }
        Ok((phone_number, account_name.as_deref()))
    }
}

#[async_trait]
impl PayoutAdapter for MockMobileMoneyAdapter {
    fn provider(&self) -> &'static str {
        "mobile-money-mock"
    }

    fn currency(&self) -> Currency {
        self.currency
    }

    fn rail(&self) -> Rail {
        Rail::MobileMoney
    }

    fn countries(&self) -> Vec<String> {
        let all = ["BJ", "BF", "CI", "GW", "ML", "NE", "SN", "TG", "CM", "CF", "TD", "CG", "GQ", "GA"];
        all.iter().filter(|c| currency_for_country(c) == Some(self.currency)).map(|c| c.to_string()).collect()
    }

    async fn institutions(&self, _country: Option<&str>) -> Result<Vec<Institution>, AdapterError> {
        Ok(PROVIDERS
            .iter()
            .map(|(code, name)| Institution {
                code: (*code).into(),
                name: (*name).into(),
                kind: InstitutionKind::MobileMoney,
            })
            .collect())
    }

    async fn resolve_recipient(&self, recipient: &Recipient) -> Result<ResolvedRecipient, AdapterError> {
        let (phone, name) = self.validate(recipient)?;
        let name = name
            .map(str::to_string)
            .unwrap_or_else(|| format!("DEMO RECIPIENT {}", mask(phone)));
        Ok(ResolvedRecipient {
            account_name: Some(name),
            ..Default::default()
        })
    }

    async fn create_payout(&self, request: PayoutRequest<'_>) -> Result<PayoutAccepted, AdapterError> {
        self.validate(request.recipient)?;
        Ok(PayoutAccepted {
            provider_reference: format!("mockmomo_{}", request.idempotency_key),
            completed: false,
        })
    }
}

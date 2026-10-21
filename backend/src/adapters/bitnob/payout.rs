//! Bitnob payouts: NG bank transfers (`NG_BANK_TRANSFER`) and mobile money (`MOBILE_MONEY`) in
//! KE, the XOF countries and the XAF countries.
//!
//! Per payout: quote -> initialize (beneficiary) -> finalize (irreversible). The reference is the
//! payment ID on the first attempt and `{payment_id}-r{n}` on retries, so a retry never reuses a
//! reference and reconciliation can find every attempt.
//!
//! Bitnob's spec documents bank beneficiaries only. For mobile money we read the field names and
//! number pattern from `GET /api/payouts/supported-countries/{country}` at startup; when Bitnob
//! doesn't name them we fall back to `bank_code` (operator) and `account_number` (phone), as for
//! banks. Run `cargo test --test bitnob_live` to see what Bitnob returns for your countries.

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use regex::Regex;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::{BitnobClient, Limit, final_status, never_paid};
use crate::{
    adapters::{
        AdapterError,
        payout::{Institution, InstitutionKind, PayoutAccepted, PayoutAdapter, PayoutRequest, ResolvedRecipient},
    },
    domain::{
        Currency, Rail, Recipient,
        currency::currency_for_country,
        money::{major_to_minor_ceil, major_to_minor_floor, minor_to_major},
    },
    services::payments::PayoutEventStatus,
};

/// How many recent Bitnob payouts reconciliation scans for earlier attempts.
const RECONCILE_SCAN: u32 = 200;
const DIRECTORY_TTL: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone)]
pub struct CountryRules {
    pub country: String,
    pub min_minor: i64,
    pub max_minor: i64,
    /// Beneficiary key for the operator or bank code.
    pub provider_key: String,
    /// Beneficiary key for the account or phone number.
    pub number_key: String,
    pub number_pattern: Option<Regex>,
}

pub struct BitnobPayoutAdapter {
    client: Arc<BitnobClient>,
    currency: Currency,
    rail: Rail,
    countries: Vec<CountryRules>,
    directory: Mutex<HashMap<String, (std::time::Instant, Vec<Institution>)>>,
}

fn destination_type(rail: Rail) -> &'static str {
    match rail {
        Rail::NgBankTransfer => "bank",
        _ => "mobile_money",
    }
}

/// Reads `destination_types.<type>.fields` from a country requirements response.
fn parse_rules(requirements: &Value, destination: &str) -> (String, String, Option<Regex>, Vec<String>) {
    let fields = requirements
        .pointer(&format!("/destination_types/{destination}/fields"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let key_of = |f: &Value| f.get("key").and_then(Value::as_str).unwrap_or_default().to_string();
    let keys: Vec<String> = fields.iter().map(key_of).collect();
    let pick = |candidates: &[&str], default: &str| {
        candidates
            .iter()
            .find(|c| keys.iter().any(|k| k == *c))
            .map(|c| c.to_string())
            .unwrap_or(default.into())
    };
    let provider_key = pick(
        &["bank_code", "provider", "network", "operator", "mobile_operator"],
        "bank_code",
    );
    let number_key = pick(
        &["account_number", "phone_number", "mobile_number", "msisdn"],
        "account_number",
    );
    let pattern = fields
        .iter()
        .find(|f| key_of(f) == number_key)
        .and_then(|f| f.get("pattern").and_then(Value::as_str))
        .and_then(|p| Regex::new(p).ok());
    let handled = [
        provider_key.as_str(),
        number_key.as_str(),
        "account_name",
        "country",
        "destination_type",
    ];
    let unhandled = fields
        .iter()
        .filter(|f| f.get("required").and_then(Value::as_bool).unwrap_or(false))
        .map(key_of)
        .filter(|k| !handled.contains(&k.as_str()))
        .collect();
    (provider_key, number_key, pattern, unhandled)
}

fn major(s: &str) -> Option<Decimal> {
    s.trim().parse().ok()
}

impl BitnobPayoutAdapter {
    /// Loads limits and field rules for each country. Fails if Bitnob has no limits for a
    /// country, which means the corridor isn't enabled on the workspace.
    pub async fn connect(
        client: Arc<BitnobClient>,
        currency: Currency,
        rail: Rail,
        countries: &[String],
        limits: &[Limit],
    ) -> Result<Self, AdapterError> {
        let mut rules = Vec::new();
        for country in countries {
            let country = country.to_ascii_uppercase();
            let limit = limits
                .iter()
                .find(|l| l.country.eq_ignore_ascii_case(&country) && l.currency.eq_ignore_ascii_case(currency.code()))
                .ok_or_else(|| {
                    AdapterError::Rejected(format!("Bitnob has no {currency} payout limits for {country}"))
                })?;
            let (min, max) = match (major(&limit.min_amount), major(&limit.max_amount)) {
                (Some(min), Some(max)) => (min, max),
                _ => {
                    return Err(AdapterError::Unavailable(format!(
                        "unreadable Bitnob limits for {country}"
                    )));
                }
            };
            let requirements = client.requirements(&country).await?;
            let (provider_key, number_key, number_pattern, unhandled) =
                parse_rules(&requirements, destination_type(rail));
            if !unhandled.is_empty() {
                tracing::warn!(%country, fields = ?unhandled, "Bitnob requires beneficiary fields SatsBridge doesn't collect");
            }
            tracing::info!(%country, %provider_key, %number_key, pattern = ?number_pattern.as_ref().map(Regex::as_str), "Bitnob corridor");
            rules.push(CountryRules {
                country,
                min_minor: major_to_minor_ceil(min, currency).unwrap_or(0),
                max_minor: major_to_minor_floor(max, currency).unwrap_or(i64::MAX),
                provider_key,
                number_key,
                number_pattern,
            });
        }
        Ok(Self {
            client,
            currency,
            rail,
            countries: rules,
            directory: Mutex::new(HashMap::new()),
        })
    }

    fn rules_for(&self, country: &str) -> Result<&CountryRules, AdapterError> {
        self.countries
            .iter()
            .find(|r| r.country.eq_ignore_ascii_case(country))
            .ok_or_else(|| {
                AdapterError::InvalidRecipient(format!("{} payouts to {country} are not enabled", self.currency))
            })
    }

    async fn directory(&self, country: &str) -> Result<Vec<Institution>, AdapterError> {
        let mut cache = self.directory.lock().await;
        if let Some((at, list)) = cache.get(country)
            && at.elapsed() < DIRECTORY_TTL
        {
            return Ok(list.clone());
        }
        let kind = if self.rail == Rail::NgBankTransfer {
            InstitutionKind::Bank
        } else {
            InstitutionKind::MobileMoney
        };
        let list: Vec<Institution> = self
            .client
            .banks(country)
            .await?
            .into_iter()
            .map(|b| Institution {
                code: b.bank_code,
                name: b.bank_name.trim().to_string(),
                kind: kind.clone(),
            })
            .collect();
        cache.insert(country.to_string(), (std::time::Instant::now(), list.clone()));
        Ok(list)
    }

    /// (country, provider code, number, name given by the sender)
    fn destination<'a>(
        &self,
        recipient: &'a Recipient,
    ) -> Result<(&'a str, &'a str, String, Option<&'a str>), AdapterError> {
        let invalid = |m: String| AdapterError::InvalidRecipient(m);
        let (country, code, number, name) = match (self.rail, recipient) {
            (
                Rail::NgBankTransfer,
                Recipient::NgBankTransfer {
                    bank_code,
                    account_number,
                    ..
                },
            ) => ("NG", bank_code.as_str(), account_number.trim().to_string(), None),
            (
                Rail::MobileMoney,
                Recipient::MobileMoney {
                    country,
                    provider_code,
                    phone_number,
                    account_name,
                },
            ) => {
                let digits: String = phone_number.chars().filter(char::is_ascii_digit).collect();
                (
                    country.as_str(),
                    provider_code.as_str(),
                    digits,
                    account_name.as_deref(),
                )
            }
            _ => return Err(invalid(format!("expected a {} recipient", self.rail.as_str()))),
        };
        if currency_for_country(country) != Some(self.currency) {
            return Err(invalid(format!("{country} doesn't pay out in {}", self.currency)));
        }
        let rules = self.rules_for(country)?;
        let valid = match &rules.number_pattern {
            Some(pattern) => pattern.is_match(&number),
            None => (6..=15).contains(&number.len()) && number.bytes().all(|b| b.is_ascii_digit()),
        };
        if !valid {
            return Err(invalid(format!("{} is not a valid number for {country}", number)));
        }
        Ok((country, code, number, name))
    }

    fn reference(key: &str, attempt: i64) -> String {
        if attempt <= 1 {
            key.to_string()
        } else {
            format!("{key}-r{attempt}")
        }
    }
}

#[async_trait]
impl PayoutAdapter for BitnobPayoutAdapter {
    fn provider(&self) -> &'static str {
        "bitnob"
    }

    fn currency(&self) -> Currency {
        self.currency
    }

    fn rail(&self) -> Rail {
        self.rail
    }

    fn countries(&self) -> Vec<String> {
        self.countries.iter().map(|r| r.country.clone()).collect()
    }

    fn min_payout_minor(&self) -> Option<i64> {
        self.countries.iter().map(|r| r.min_minor).min()
    }

    fn max_payout_minor(&self) -> Option<i64> {
        self.countries.iter().map(|r| r.max_minor).max()
    }

    async fn institutions(&self, country: Option<&str>) -> Result<Vec<Institution>, AdapterError> {
        let country = match (self.rail, country) {
            (Rail::NgBankTransfer, _) => "NG".to_string(),
            (_, Some(c)) => c.to_ascii_uppercase(),
            (_, None) => {
                return Err(AdapterError::InvalidRecipient(
                    "country is required for mobile money operators".into(),
                ));
            }
        };
        self.rules_for(&country)?;
        self.directory(&country).await
    }

    async fn resolve_recipient(&self, recipient: &Recipient) -> Result<ResolvedRecipient, AdapterError> {
        let (country, code, number, given_name) = self.destination(recipient)?;
        if !self.directory(country).await?.iter().any(|i| i.code == code) {
            return Err(AdapterError::InvalidRecipient(format!(
                "unknown bank or operator code {code} for {country}"
            )));
        }
        let looked_up = self.client.account_lookup(country, code, &number).await?;
        let account_name = match (self.rail, looked_up, given_name) {
            (_, Some(name), _) => name,
            (Rail::NgBankTransfer, None, _) => return Err(AdapterError::RecipientNotFound),
            (_, None, Some(name)) if !name.trim().is_empty() => name.trim().to_string(),
            (_, None, _) => {
                return Err(AdapterError::InvalidRecipient(
                    "account_name is required: Bitnob can't look up this account".into(),
                ));
            }
        };
        let rules = self.rules_for(country)?;
        Ok(ResolvedRecipient {
            account_name: Some(account_name),
            min_minor: Some(rules.min_minor),
            max_minor: Some(rules.max_minor),
            ..Default::default()
        })
    }

    async fn create_payout(&self, request: PayoutRequest<'_>) -> Result<PayoutAccepted, AdapterError> {
        let (country, code, number, _) = self.destination(request.recipient)?;
        let rules = self.rules_for(country)?;
        let account_name = request.account_name.ok_or_else(|| {
            AdapterError::InvalidRecipient("account name is required; resolve the account first".into())
        })?;
        let settlement_amount = minor_to_major(request.amount_minor, self.currency)
            .normalize()
            .to_string();
        let reference = Self::reference(request.idempotency_key, request.attempt);

        let quote = self
            .client
            .quote(
                self.currency.code(),
                country,
                &settlement_amount,
                &request.narration,
                &reference,
            )
            .await?;
        let quote_id = quote.quote_id.clone().unwrap_or_else(|| quote.id.clone());
        if let Some(needed) = quote.sats()
            && needed > request.amount_sats
        {
            // Customer was promised this amount; pay it and absorb the difference.
            tracing::warn!(
                payment_id = request.idempotency_key,
                needed,
                budget = request.amount_sats,
                "Bitnob quote exceeds sat budget"
            );
        }

        let mut beneficiary = json!({
            "destination_type": destination_type(self.rail),
            "country": country,
            "account_name": account_name,
        });
        beneficiary[&rules.provider_key] = json!(code);
        beneficiary[&rules.number_key] = json!(number);

        let init = self
            .client
            .initialize(&quote_id, beneficiary, &reference, &request.narration)
            .await?;
        self.client.finalize(&quote_id).await?;
        Ok(PayoutAccepted {
            provider_reference: init.id,
            completed: false,
        })
    }

    async fn find_payout(&self, idempotency_key: &str) -> Result<Option<PayoutAccepted>, AdapterError> {
        let retry_prefix = format!("{idempotency_key}-r");
        let found = self.client.recent_payouts(RECONCILE_SCAN).await?.into_iter().find(|p| {
            let ours = p
                .reference
                .as_deref()
                .is_some_and(|r| r == idempotency_key || r.starts_with(&retry_prefix));
            ours && !p.status.as_deref().is_some_and(never_paid)
        });
        Ok(found.map(|p| PayoutAccepted {
            provider_reference: p.id,
            completed: false,
        }))
    }

    async fn payout_status(&self, provider_reference: &str) -> Result<Option<PayoutEventStatus>, AdapterError> {
        Ok(self
            .client
            .get_payout(provider_reference)
            .await?
            .status
            .as_deref()
            .and_then(final_status))
    }
}

/// Probe amount used by the rate source: a typical payout, kept inside the country's limits.
pub fn probe_amount(currency: Currency, rules: &CountryRules) -> Decimal {
    let typical: i64 = match currency {
        Currency::Ngn => 100_000,
        Currency::Kes => 5_000,
        _ => 50_000,
    };
    let typical = major_to_minor_floor(Decimal::from(typical), currency).unwrap_or(rules.min_minor);
    minor_to_major(typical.clamp(rules.min_minor, rules.max_minor), currency)
}

impl BitnobPayoutAdapter {
    /// Countries and probe amounts for [`super::rates::BitnobRateSource`].
    pub fn rate_probes(&self) -> Vec<(Currency, String, Decimal)> {
        self.countries
            .iter()
            .take(1)
            .map(|r| (self.currency, r.country.clone(), probe_amount(self.currency, r)))
            .collect()
    }

    pub fn client(&self) -> &Arc<BitnobClient> {
        &self.client
    }
}

#[cfg(test)]
mod tests {
    use super::{super::fake::*, *};

    async fn adapter(rail: Rail, currency: Currency, countries: &[&str], fake: Fake) -> BitnobPayoutAdapter {
        let base = serve(fake).await;
        let client = Arc::new(BitnobClient::new(
            reqwest::Client::new(),
            config(base, SECRET, countries),
        ));
        let limits = client.limits().await.unwrap();
        let countries: Vec<String> = countries.iter().map(|c| c.to_string()).collect();
        BitnobPayoutAdapter::connect(client, currency, rail, &countries, &limits)
            .await
            .unwrap()
    }

    fn bank(account: &str) -> Recipient {
        Recipient::NgBankTransfer {
            bank_code: "000014".into(),
            account_number: account.into(),
            resolution_id: None,
        }
    }

    fn momo(country: &str, phone: &str, name: Option<&str>) -> Recipient {
        Recipient::MobileMoney {
            country: country.into(),
            provider_code: "OM".into(),
            phone_number: phone.into(),
            account_name: name.map(Into::into),
        }
    }

    fn request<'a>(recipient: &'a Recipient, attempt: i64, amount_minor: i64) -> PayoutRequest<'a> {
        PayoutRequest {
            idempotency_key: "pay_1",
            attempt,
            recipient,
            account_name: Some("ADA OBI"),
            amount_minor,
            amount_sats: 1_000_000,
            currency: Currency::Ngn,
            narration: "SatsBridge pay_1".into(),
        }
    }

    #[tokio::test]
    async fn loads_limits_and_rules_per_country() {
        let ng = adapter(Rail::NgBankTransfer, Currency::Ngn, &["NG"], Fake::default()).await;
        assert_eq!(
            (ng.min_payout_minor(), ng.max_payout_minor()),
            (Some(50_000), Some(5_000_000_000))
        );
        let xof = adapter(Rail::MobileMoney, Currency::Xof, &["SN", "CI"], Fake::default()).await;
        assert_eq!(
            (xof.min_payout_minor(), xof.max_payout_minor()),
            (Some(100), Some(2_000_000))
        );
    }

    #[tokio::test]
    async fn resolves_bank_accounts_and_mobile_money() {
        let ng = adapter(Rail::NgBankTransfer, Currency::Ngn, &["NG"], Fake::default()).await;
        assert_eq!(
            ng.resolve_recipient(&bank("1421795566"))
                .await
                .unwrap()
                .account_name
                .as_deref(),
            Some("ADA OBI")
        );
        assert!(matches!(
            ng.resolve_recipient(&bank("0000000000")).await,
            Err(AdapterError::RecipientNotFound)
        ));
        assert!(matches!(
            ng.resolve_recipient(&bank("12345")).await,
            Err(AdapterError::InvalidRecipient(_))
        ));

        let xof = adapter(Rail::MobileMoney, Currency::Xof, &["SN"], Fake::default()).await;
        // No lookup for mobile money: the sender's name is used, and required.
        let ok = xof
            .resolve_recipient(&momo("SN", "77 123 45 67", Some("Awa Diop")))
            .await
            .unwrap();
        assert_eq!(
            (ok.account_name.as_deref(), ok.max_minor),
            (Some("Awa Diop"), Some(200_000))
        );
        assert!(xof.resolve_recipient(&momo("SN", "771234567", None)).await.is_err());
        assert!(
            xof.resolve_recipient(&momo("SN", "1234", Some("A"))).await.is_err(),
            "pattern from Bitnob"
        );
        assert!(
            xof.resolve_recipient(&momo("CI", "771234567", Some("A")))
                .await
                .is_err(),
            "CI not enabled"
        );
        assert!(
            xof.resolve_recipient(&momo("CM", "771234567", Some("A")))
                .await
                .is_err(),
            "XAF country"
        );
    }

    #[tokio::test]
    async fn pays_out_with_per_attempt_references() {
        let fake = Fake::default();
        let ng = adapter(Rail::NgBankTransfer, Currency::Ngn, &["NG"], fake.clone()).await;
        let r = bank("1421795566");
        assert_eq!(
            ng.create_payout(request(&r, 1, 24_749_850))
                .await
                .unwrap()
                .provider_reference,
            "bn-payout-1"
        );
        ng.create_payout(request(&r, 2, 24_749_850)).await.unwrap();

        let calls = fake.calls.lock().unwrap();
        let paths: Vec<&str> = calls.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths[..3], ["quotes", "QT2_1/initialize", "QT2_1/finalize"]);
        assert_eq!(calls[0].1["settlement_amount"], "247498.5");
        assert_eq!(calls[0].1["reference"], "pay_1");
        assert_eq!(calls[3].1["reference"], "pay_1-r2");
        assert_eq!(calls[1].1["beneficiary"]["account_number"], "1421795566");
        assert_eq!(calls[1].1["beneficiary"]["destination_type"], "bank");
    }

    #[tokio::test]
    async fn mobile_money_beneficiary_uses_bitnob_field_names() {
        let fake = Fake::default();
        let xof = adapter(Rail::MobileMoney, Currency::Xof, &["SN"], fake.clone()).await;
        let r = momo("SN", "77 123 45 67", Some("Awa Diop"));
        xof.create_payout(request(&r, 1, 10_000)).await.unwrap();
        let calls = fake.calls.lock().unwrap();
        let beneficiary = &calls[1].1["beneficiary"];
        assert_eq!(beneficiary["destination_type"], "mobile_money");
        assert_eq!(
            (
                beneficiary["bank_code"].as_str(),
                beneficiary["account_number"].as_str()
            ),
            (Some("OM"), Some("771234567"))
        );
    }

    #[tokio::test]
    async fn reconciliation_finds_only_paid_attempts() {
        let fake = Fake::default();
        let ng = adapter(Rail::NgBankTransfer, Currency::Ngn, &["NG"], fake.clone()).await;
        assert!(ng.find_payout("pay_1").await.unwrap().is_none());

        fake.existing.lock().unwrap().extend([
            json!({"id": "a", "reference": "pay_1", "status": "INITIATED"}),
            json!({"id": "b", "reference": "pay_10", "status": "processing"}),
        ]);
        assert!(
            ng.find_payout("pay_1").await.unwrap().is_none(),
            "never finalized; other payment"
        );

        fake.existing
            .lock()
            .unwrap()
            .push(json!({"id": "c", "reference": "pay_1-r2", "status": "processing"}));
        assert_eq!(ng.find_payout("pay_1").await.unwrap().unwrap().provider_reference, "c");
        assert!(matches!(
            ng.payout_status("c").await.unwrap(),
            Some(PayoutEventStatus::Success)
        ));
    }
}

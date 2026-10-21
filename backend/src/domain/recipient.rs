use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Rail {
    /// Kenyan mobile money (M-Pesa) via Tando.
    KeMobileMoney,
    /// Nigerian NIP interbank transfer to a NUBAN (banks and fintech wallets).
    NgBankTransfer,
    /// LNURL-pay Lightning Address whose owner off-ramps to local currency (e.g. Tapnob).
    LightningAddress,
    /// Mobile money wallet in a given country (M-Pesa, Orange Money, MTN MoMo, Wave, ...).
    MobileMoney,
}

impl Rail {
    pub fn as_str(self) -> &'static str {
        match self {
            Rail::KeMobileMoney => "KE_MOBILE_MONEY",
            Rail::NgBankTransfer => "NG_BANK_TRANSFER",
            Rail::LightningAddress => "LIGHTNING_ADDRESS",
            Rail::MobileMoney => "MOBILE_MONEY",
        }
    }
}

/// Destination account details. Serialized with the rail as the tag, e.g.
/// `{"rail":"NG_BANK_TRANSFER","bank_code":"058","account_number":"0123456789","resolution_id":"res_..."}`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "rail", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Recipient {
    KeMobileMoney {
        phone_number: String,
    },
    NgBankTransfer {
        bank_code: String,
        account_number: String,
        /// From `POST /api/v1/recipients/resolve`; required when creating a payment.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolution_id: Option<String>,
    },
    LightningAddress {
        /// `name@domain`, e.g. `ada@tapnob.io`.
        address: String,
    },
    MobileMoney {
        /// ISO 3166 alpha-2, e.g. `SN`.
        country: String,
        /// Operator code from `GET /payout-institutions`.
        provider_code: String,
        phone_number: String,
        /// Needed where the provider can't look the name up.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_name: Option<String>,
    },
}

impl Recipient {
    pub fn rail(&self) -> Rail {
        match self {
            Recipient::KeMobileMoney { .. } => Rail::KeMobileMoney,
            Recipient::NgBankTransfer { .. } => Rail::NgBankTransfer,
            Recipient::LightningAddress { .. } => Rail::LightningAddress,
            Recipient::MobileMoney { .. } => Rail::MobileMoney,
        }
    }

    pub fn masked_reference(&self) -> String {
        match self {
            Recipient::KeMobileMoney { phone_number } => mask(phone_number),
            Recipient::NgBankTransfer {
                bank_code,
                account_number,
                ..
            } => {
                format!("{bank_code}:{}", mask(account_number))
            }
            Recipient::LightningAddress { address } => address.trim().to_ascii_lowercase(),
            Recipient::MobileMoney {
                country,
                provider_code,
                phone_number,
                ..
            } => {
                format!(
                    "{}:{provider_code}:{}",
                    country.to_ascii_uppercase(),
                    mask(phone_number)
                )
            }
        }
    }
}

/// Keeps the last four characters: `0123456789` -> `******6789`.
pub fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let keep = chars.len().min(4);
    let hidden = chars.len() - keep;
    "*".repeat(hidden) + &chars[hidden..].iter().collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_all_but_last_four() {
        assert_eq!(mask("0123456789"), "******6789");
        assert_eq!(mask("123"), "123");
    }

    #[test]
    fn deserializes_tagged_recipient() {
        let r: Recipient =
            serde_json::from_str(r#"{"rail":"NG_BANK_TRANSFER","bank_code":"058","account_number":"0123456789"}"#)
                .unwrap();
        assert_eq!(r.rail(), Rail::NgBankTransfer);
        assert_eq!(r.masked_reference(), "058:******6789");
    }
}

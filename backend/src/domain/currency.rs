use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "UPPERCASE")]
#[sqlx(rename_all = "UPPERCASE")]
pub enum Currency {
    Xof,
    Xaf,
    Gnf,
    Kes,
    Ngn,
}

impl Currency {
    pub const ALL: [Currency; 5] = [
        Currency::Xof,
        Currency::Xaf,
        Currency::Gnf,
        Currency::Kes,
        Currency::Ngn,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Currency::Xof => "XOF",
            Currency::Xaf => "XAF",
            Currency::Gnf => "GNF",
            Currency::Kes => "KES",
            Currency::Ngn => "NGN",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Currency::Xof => "West African CFA franc",
            Currency::Xaf => "Central African CFA franc",
            Currency::Gnf => "Guinean franc",
            Currency::Kes => "Kenyan shilling",
            Currency::Ngn => "Nigerian naira",
        }
    }

    /// Decimal places of the minor unit (ISO 4217). All amounts are stored as integer minor units.
    pub fn exponent(self) -> u32 {
        match self {
            Currency::Xof | Currency::Xaf | Currency::Gnf => 0,
            Currency::Kes | Currency::Ngn => 2,
        }
    }

    /// NGN is payout-only for now (docs/prd-addendum-ngn-payout.md).
    pub fn can_send(self) -> bool {
        !matches!(self, Currency::Ngn)
    }
}

/// Currency used for payouts in a country (ISO 3166 alpha-2), for the countries we serve.
pub fn currency_for_country(country: &str) -> Option<Currency> {
    match country.to_ascii_uppercase().as_str() {
        "NG" => Some(Currency::Ngn),
        "KE" => Some(Currency::Kes),
        "GN" => Some(Currency::Gnf),
        // UEMOA
        "BJ" | "BF" | "CI" | "GW" | "ML" | "NE" | "SN" | "TG" => Some(Currency::Xof),
        // CEMAC
        "CM" | "CF" | "TD" | "CG" | "GQ" | "GA" => Some(Currency::Xaf),
        _ => None,
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl FromStr for Currency {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Currency::ALL
            .into_iter()
            .find(|c| c.code().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| format!("unsupported currency: {s}"))
    }
}

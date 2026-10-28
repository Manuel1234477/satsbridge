use rust_decimal::{Decimal, prelude::ToPrimitive};

use super::Currency;

pub const SATS_PER_BTC: i64 = 100_000_000;

pub fn minor_to_major(minor: i64, currency: Currency) -> Decimal {
    Decimal::new(minor, currency.exponent())
}

pub fn major_to_minor_floor(major: Decimal, currency: Currency) -> Option<i64> {
    (major * minor_scale(currency)).floor().to_i64()
}

pub fn major_to_minor_ceil(major: Decimal, currency: Currency) -> Option<i64> {
    (major * minor_scale(currency)).ceil().to_i64()
}

fn minor_scale(currency: Currency) -> Decimal {
    Decimal::from(10i64.pow(currency.exponent()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_between_minor_and_major_units() {
        assert_eq!(minor_to_major(12_345, Currency::Ngn).to_string(), "123.45");
        assert_eq!(minor_to_major(12_345, Currency::Xof).to_string(), "12345");
        assert_eq!(
            major_to_minor_floor(Decimal::new(12_3459, 4), Currency::Kes),
            Some(1234)
        );
        assert_eq!(major_to_minor_ceil(Decimal::new(12_3451, 4), Currency::Kes), Some(1235));
    }
}

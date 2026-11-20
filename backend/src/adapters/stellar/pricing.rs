//! Stellar amount and price helpers.

use rust_decimal::{Decimal, prelude::ToPrimitive};

use super::STROOPS_PER_UNIT;
use crate::domain::Currency;

/// `1234567` stroops -> `"0.1234567"` (Horizon's amount format).
pub fn format_amount(stroops: i64) -> String {
    Decimal::new(stroops, 7).to_string()
}

/// `"0.1234567"` -> `1234567` stroops. Rejects more than 7 decimal places and negatives.
pub fn parse_amount(raw: &str) -> Option<i64> {
    let d: Decimal = raw.trim().parse().ok()?;
    if d.is_sign_negative() || d.scale() > 7 {
        return None;
    }
    (d * Decimal::from(STROOPS_PER_UNIT)).to_i64()
}

/// Currency minor units -> stroops. Exact, since every currency has at most 7 decimals.
pub fn minor_to_stroops(minor: i64, currency: Currency) -> Option<i64> {
    minor.checked_mul(10i64.pow(7 - currency.exponent()))
}

/// Stroops -> currency minor units, rounded down.
pub fn stroops_to_minor_floor(stroops: i64, currency: Currency) -> i64 {
    stroops.div_euclid(10i64.pow(7 - currency.exponent()))
}

/// Stroops -> currency minor units, rounded up.
pub fn stroops_to_minor_ceil(stroops: i64, currency: Currency) -> i64 {
    let unit = 10i64.pow(7 - currency.exponent());
    stroops.div_euclid(unit) + i64::from(stroops.rem_euclid(unit) != 0)
}

/// Best rational approximation `n/d` of a positive price with both terms in `i32`, as Stellar
/// offers need (continued fractions, like the Stellar SDKs' `best_r`).
pub fn best_rational(price: Decimal) -> Option<(i32, i32)> {
    if price <= Decimal::ZERO {
        return None;
    }
    let max = Decimal::from(i32::MAX);
    let (mut h0, mut h1) = (Decimal::ZERO, Decimal::ONE);
    let (mut k0, mut k1) = (Decimal::ONE, Decimal::ZERO);
    let mut x = price;
    let mut best = None;
    for _ in 0..40 {
        let a = x.floor();
        let h = a * h1 + h0;
        let k = a * k1 + k0;
        if h > max || k > max {
            break;
        }
        best = Some((h.to_i32()?, k.to_i32()?));
        let frac = x - a;
        if frac.is_zero() {
            break;
        }
        (h0, h1, k0, k1) = (h1, h, k1, k);
        x = Decimal::ONE.checked_div(frac)?;
    }
    best.filter(|(n, d)| *n > 0 && *d > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_round_trip() {
        assert_eq!(format_amount(1_234_567), "0.1234567");
        assert_eq!(format_amount(150_000_000), "15.0000000");
        assert_eq!(parse_amount("15.0000000"), Some(150_000_000));
        assert_eq!(parse_amount("0.00000001"), None);
        assert_eq!(parse_amount("-1"), None);
    }

    #[test]
    fn converts_minor_units() {
        assert_eq!(minor_to_stroops(12_345, Currency::Ngn), Some(1_234_500_000));
        assert_eq!(minor_to_stroops(500, Currency::Xof), Some(5_000_000_000));
        assert_eq!(stroops_to_minor_floor(1_234_599_999, Currency::Ngn), 12_345);
        assert_eq!(stroops_to_minor_ceil(1_234_500_001, Currency::Ngn), 12_346);
        assert_eq!(stroops_to_minor_ceil(1_234_500_000, Currency::Ngn), 12_345);
    }

    #[test]
    fn approximates_prices() {
        assert_eq!(best_rational(Decimal::new(25, 1)), Some((5, 2)));
        let (n, d) = best_rational(Decimal::new(116, 6)).unwrap(); // USD per GNF
        assert_eq!(Decimal::from(n) / Decimal::from(d), Decimal::new(116, 6));
        let (n, d) = best_rational(Decimal::from(1550)).unwrap();
        assert_eq!((n, d), (1550, 1));
        assert!(best_rational(Decimal::ZERO).is_none());
    }
}

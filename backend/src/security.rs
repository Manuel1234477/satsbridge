use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};

type HmacSha256 = Hmac<Sha256>;

pub const SIGNATURE_HEADER: &str = "x-satsbridge-signature";

/// Hex HMAC-SHA256 of the raw request body.
pub fn sign(secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// Constant-time check of a `<hex>` or `sha256=<hex>` signature.
pub fn verify_signature(secret: &str, body: &[u8], signature: Option<&str>) -> bool {
    let Some(signature) = signature else { return false };
    let Ok(expected) = hex::decode(signature.trim().trim_start_matches("sha256=")) else {
        return false;
    };
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

/// Constant-time check of a hex HMAC-SHA512 signature (Bitnob webhooks).
pub fn verify_hmac_sha512(secret: &str, body: &[u8], signature: Option<&str>) -> bool {
    let Some(Ok(expected)) = signature.map(|s| hex::decode(s.trim())) else {
        return false;
    };
    let mut mac = Hmac::<Sha512>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Lets a resolution be matched to a recipient without storing the account number.
/// TODO: key this with a dedicated secret; a plain hash of a 10-digit NUBAN is brute-forceable.
pub fn hash_account(bank_code: &str, account_number: &str) -> String {
    hex::encode(Sha256::digest(format!("{bank_code}:{account_number}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_round_trip() {
        let sig = sign("secret", b"{}");
        assert!(verify_signature("secret", b"{}", Some(&sig)));
        assert!(verify_signature("secret", b"{}", Some(&format!("sha256={sig}"))));
        assert!(!verify_signature("other", b"{}", Some(&sig)));
        assert!(!verify_signature("secret", b"{ }", Some(&sig)));
        assert!(!verify_signature("secret", b"{}", None));
    }
}

use ed25519_dalek::{Signer, SigningKey};
use stellar_strkey::ed25519;

/// An account keypair. Deliberately not `Debug`: it holds a secret key.
#[derive(Clone)]
pub struct Keypair {
    signing: SigningKey,
}

impl Keypair {
    /// From an `S...` secret seed.
    pub fn from_secret(secret: &str) -> anyhow::Result<Self> {
        let key = ed25519::PrivateKey::from_string(secret.trim())
            .map_err(|_| anyhow::anyhow!("not a valid Stellar secret seed (S...)"))?;
        Ok(Self {
            signing: SigningKey::from_bytes(&key.0),
        })
    }

    pub fn random() -> anyhow::Result<Self> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
        Ok(Self {
            signing: SigningKey::from_bytes(&seed),
        })
    }

    pub fn public_bytes(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    /// `G...` account ID.
    pub fn public_key(&self) -> String {
        ed25519::PublicKey(self.public_bytes()).to_string().as_str().to_owned()
    }

    /// `S...` secret seed. Only for printing freshly generated testnet keys.
    pub fn secret(&self) -> String {
        ed25519::PrivateKey(self.signing.to_bytes())
            .as_unredacted()
            .to_string()
            .as_str()
            .to_owned()
    }

    /// Last four bytes of the public key, as Stellar decorated signatures want.
    pub fn hint(&self) -> [u8; 4] {
        let pk = self.public_bytes();
        [pk[28], pk[29], pk[30], pk[31]]
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing.sign(message).to_bytes()
    }
}

/// Decodes a `G...` account ID.
pub fn decode_account(account: &str) -> anyhow::Result<[u8; 32]> {
    ed25519::PublicKey::from_string(account.trim())
        .map(|k| k.0)
        .map_err(|_| anyhow::anyhow!("not a valid Stellar account ID (G...): {account}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_round_trips_and_derives_account() {
        let kp = Keypair::random().unwrap();
        let again = Keypair::from_secret(&kp.secret()).unwrap();
        assert_eq!(kp.public_key(), again.public_key());
        assert!(kp.public_key().starts_with('G'));
        assert_eq!(decode_account(&kp.public_key()).unwrap(), kp.public_bytes());
        assert!(Keypair::from_secret("SNOTAKEY").is_err());
    }
}

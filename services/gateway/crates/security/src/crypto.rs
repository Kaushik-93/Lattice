//! Encryption of connection credentials at rest (AES-256-GCM).

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{AeadCore, Aes256Gcm, Nonce};
use argon2::password_hash::rand_core::OsRng;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error(
        "LATTICE_SECRET_KEY must be 32 bytes of base64 (generate one with `openssl rand -base64 32`)"
    )]
    BadKey,
    #[error("secret could not be decrypted (wrong key, or the ciphertext was altered or moved)")]
    Decrypt,
}

/// Seals secrets so that only this deployment's key can open them, and only for the record
/// they were sealed for: the `context` (e.g. the source id) is bound in as associated data.
#[derive(Clone)]
pub struct SecretBox {
    cipher: Aes256Gcm,
}

impl SecretBox {
    pub fn from_base64_key(key: &str) -> Result<Self, CryptoError> {
        let bytes = STANDARD
            .decode(key.trim())
            .map_err(|_| CryptoError::BadKey)?;
        let cipher = Aes256Gcm::new_from_slice(&bytes).map_err(|_| CryptoError::BadKey)?;
        Ok(Self { cipher })
    }

    /// Returns `nonce || ciphertext || tag`, with a fresh random nonce per call.
    pub fn seal(&self, plaintext: &[u8], context: &[u8]) -> Vec<u8> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        // Encryption with a valid key and in-memory buffers cannot fail.
        let ciphertext = self
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: context,
                },
            )
            .unwrap_or_default();
        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        out
    }

    pub fn open(&self, sealed: &[u8], context: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if sealed.len() < NONCE_LEN {
            return Err(CryptoError::Decrypt);
        }
        let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| CryptoError::Decrypt)?;
        self.cipher
            .decrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: ciphertext,
                    aad: context,
                },
            )
            .map_err(|_| CryptoError::Decrypt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

    #[test]
    fn round_trip() {
        let sb = SecretBox::from_base64_key(KEY).unwrap();
        let sealed = sb.seal(b"hunter2", b"source:1");
        assert_ne!(&sealed[NONCE_LEN..], b"hunter2");
        assert_eq!(sb.open(&sealed, b"source:1").unwrap(), b"hunter2");
    }

    #[test]
    fn nonces_are_fresh() {
        let sb = SecretBox::from_base64_key(KEY).unwrap();
        assert_ne!(sb.seal(b"x", b"c"), sb.seal(b"x", b"c"));
    }

    #[test]
    fn tampering_moving_or_wrong_key_fails() {
        let sb = SecretBox::from_base64_key(KEY).unwrap();
        let mut sealed = sb.seal(b"hunter2", b"source:1");
        assert!(
            sb.open(&sealed, b"source:2").is_err(),
            "bound to its record"
        );
        let other = SecretBox::from_base64_key(&STANDARD.encode([7u8; 32])).unwrap();
        assert!(
            other.open(&sealed, b"source:1").is_err(),
            "bound to its key"
        );
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert!(
            sb.open(&sealed, b"source:1").is_err(),
            "integrity protected"
        );
        assert!(sb.open(b"short", b"source:1").is_err());
    }

    #[test]
    fn rejects_bad_keys() {
        assert!(SecretBox::from_base64_key("not base64!").is_err());
        assert!(SecretBox::from_base64_key(&STANDARD.encode([1u8; 16])).is_err());
    }
}

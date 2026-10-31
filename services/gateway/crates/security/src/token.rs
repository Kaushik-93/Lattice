use std::fmt;

use argon2::password_hash::rand_core::{OsRng, RngCore as _};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest as _, Sha256};

/// Length of an encoded token: 32 random bytes in unpadded base64url.
pub const SESSION_TOKEN_LEN: usize = 43;

/// An opaque bearer token stored in the session cookie. Only its SHA-256 is persisted.
pub struct SessionToken(String);

impl SessionToken {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn hash(&self) -> [u8; 32] {
        hash_token(&self.0)
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionToken(<redacted>)")
    }
}

/// A plain SHA-256 is enough here: tokens carry 256 bits of entropy, so there is nothing
/// for a slow hash to protect, and lookup sits on the hot path of every request.
pub fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_url_safe_and_fixed_length() {
        let a = SessionToken::generate();
        let b = SessionToken::generate();
        assert_ne!(a.as_str(), b.as_str());
        assert_eq!(a.as_str().len(), SESSION_TOKEN_LEN);
        assert!(
            a.as_str()
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        );
    }

    #[test]
    fn hash_is_stable() {
        let t = SessionToken::generate();
        assert_eq!(t.hash(), hash_token(t.as_str()));
    }

    #[test]
    fn debug_does_not_leak_the_token() {
        let t = SessionToken::generate();
        assert!(!format!("{t:?}").contains(t.as_str()));
    }
}

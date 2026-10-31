//! Password hashing. Argon2id is deliberately slow (~tens of ms), so callers on the async
//! runtime must run these functions under `spawn_blocking`.

use std::sync::LazyLock;

use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHash, PasswordHasher as _, PasswordVerifier as _};

#[derive(Debug, thiserror::Error)]
#[error("password hashing failed: {0}")]
pub struct PasswordError(String);

/// Argon2id with the crate defaults (m = 19 MiB, t = 2, p = 1), the OWASP baseline.
pub fn hash_password(password: &str) -> Result<String, PasswordError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| PasswordError(e.to_string()))
}

/// Returns false for a wrong password and for a malformed stored hash alike.
pub fn verify_password(password: &str, phc: &str) -> bool {
    PasswordHash::new(phc)
        .map(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
        .unwrap_or(false)
}

/// Spends the same CPU time as a real verification, so a login for an unknown email
/// cannot be told apart from a wrong password by its latency.
pub fn burn_verification_time(password: &str) {
    static DUMMY: LazyLock<String> =
        LazyLock::new(|| hash_password("timing-equaliser").unwrap_or_default());
    verify_password(password, &DUMMY);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery", &hash));
        assert!(!verify_password("wrong horse battery", &hash));
    }

    #[test]
    fn same_password_gets_a_fresh_salt() {
        assert_ne!(
            hash_password("pw-123456").unwrap(),
            hash_password("pw-123456").unwrap()
        );
    }

    #[test]
    fn malformed_hash_never_verifies() {
        assert!(!verify_password("anything", "not-a-phc-string"));
    }
}

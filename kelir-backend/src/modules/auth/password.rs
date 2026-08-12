use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;

use crate::error::AppError;

/// Hashes a password with Argon2id and default parameters.
///
/// Argon2id only (coding standard §2.8, NFR-SEC-001). The returned PHC string
/// carries the algorithm, parameters and salt, so verification needs nothing
/// else stored and the cost can be raised later without invalidating old hashes.
///
/// Hashing is deliberately expensive, so callers must run it on a blocking
/// thread rather than the async runtime (coding standard §2.4).
pub fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);

    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| AppError::Internal {
            source: anyhow::anyhow!("failed to hash password: {error}"),
        })
}

/// Verifies a password against a stored PHC hash.
///
/// A malformed stored hash is a failed verification, not an error: it means the
/// row is corrupt, and reporting that difference to the caller would let an
/// attacker distinguish it from a wrong password.
pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored_hash) else {
        tracing::error!("stored password hash is not a valid PHC string");
        return false;
    };

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_correct_password() {
        let hash = hash_password("correct horse battery staple").expect("hashes");

        assert!(verify_password("correct horse battery staple", &hash));
    }

    #[test]
    fn rejects_the_wrong_password() {
        let hash = hash_password("correct horse battery staple").expect("hashes");

        assert!(!verify_password("Correct horse battery staple", &hash));
        assert!(!verify_password("", &hash));
    }

    #[test]
    fn produces_a_different_hash_each_time() {
        // A per-password salt is what stops identical passwords sharing a hash,
        // which would otherwise leak that two accounts use the same one.
        let first = hash_password("same password").expect("hashes");
        let second = hash_password("same password").expect("hashes");

        assert_ne!(first, second);
        assert!(verify_password("same password", &first));
        assert!(verify_password("same password", &second));
    }

    #[test]
    fn stores_an_argon2id_phc_string() {
        let hash = hash_password("whatever").expect("hashes");

        assert!(
            hash.starts_with("$argon2id$"),
            "expected argon2id, got {hash}"
        );
    }

    #[test]
    fn treats_a_corrupt_stored_hash_as_a_failure() {
        assert!(!verify_password("whatever", "not-a-phc-string"));
        assert!(!verify_password("whatever", ""));
    }
}

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};
use rand::rngs::OsRng;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("invalid PIN format")]
    InvalidPin,
    #[error("password hashing failure")]
    HashFailure,
}

const PIN_MEMORY_KIB: u32 = 19_456;
const PIN_ITERATIONS: u32 = 2;
const PIN_PARALLELISM: u32 = 1;
const CURRENT_PIN_HASH_PREFIX: &str = "$argon2id$v=19$m=19456,t=2,p=1$";

fn pin_argon2() -> Argon2<'static> {
    let params = Params::new(
        PIN_MEMORY_KIB,
        PIN_ITERATIONS,
        PIN_PARALLELISM,
        None,
    )
    .expect("valid audited Argon2id PIN parameters");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn validate_pin(pin: &str) -> Result<(), AuthError> {
    if (4..=12).contains(&pin.len()) && pin.chars().all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err(AuthError::InvalidPin)
    }
}

pub fn hash_pin(pin: &str) -> Result<String, AuthError> {
    validate_pin(pin)?;
    let salt = SaltString::generate(&mut OsRng);
    pin_argon2()
        .hash_password(pin.as_bytes(), &salt)
        .map(|p| p.to_string())
        .map_err(|_| AuthError::HashFailure)
}

pub fn verify_pin(hash: &str, pin: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(v) => v,
        Err(_) => return false,
    };
    pin_argon2()
        .verify_password(pin.as_bytes(), &parsed)
        .is_ok()
}

pub fn pin_hash_needs_upgrade(hash: &str) -> bool {
    !hash.starts_with(CURRENT_PIN_HASH_PREFIX)
}

pub fn hash_secret(secret: &str) -> Result<String, AuthError> {
    if secret.len() < 16 {
        return Err(AuthError::HashFailure);
    }
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map(|p| p.to_string())
        .map_err(|_| AuthError::HashFailure)
}

pub fn verify_secret(hash: &str, secret: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(v) => v,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(secret.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pin_round_trip() {
        let h = hash_pin("123456").unwrap();
        assert!(verify_pin(&h, "123456"));
        assert!(!verify_pin(&h, "123457"));
        assert!(!pin_hash_needs_upgrade(&h));
        assert!(pin_hash_needs_upgrade("$argon2id$v=19$m=4096,t=1,p=1$old"));
    }
    #[test]
    fn secret_round_trip() {
        let h = hash_secret("device-secret-0123456789").unwrap();
        assert!(verify_secret(&h, "device-secret-0123456789"));
        assert!(!verify_secret(&h, "wrong-secret-0123456789"));
    }
}

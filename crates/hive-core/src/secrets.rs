//! Credential handling.
//!
//! HiveMind Chat never stores an API key. A provider entry holds only the *name*
//! of an environment variable; the value is read at request time and dropped
//! immediately afterwards. That keeps keys out of the config file, out of the
//! SQLite database and out of any backup of either.

use crate::error::{HiveError, Result};

/// A reference to a credential, not the credential itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRef(String);

impl SecretRef {
    /// Environment variable names are restricted to the POSIX-portable set so a
    /// config file cannot smuggle shell syntax into a variable lookup.
    pub fn new(var_name: impl Into<String>) -> Result<Self> {
        let name = var_name.into();
        let valid = !name.is_empty()
            && name.len() <= 128
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit());
        if !valid {
            return Err(HiveError::Config(format!(
                "'{name}' is not a valid environment variable name (expected A-Z, 0-9 and underscore)"
            )));
        }
        Ok(Self(name))
    }

    pub fn var_name(&self) -> &str {
        &self.0
    }

    /// Reads the credential. The returned value is the secret itself, so it must
    /// never be logged, serialised or returned over the API.
    pub fn resolve(&self) -> Result<String> {
        match std::env::var(&self.0) {
            Ok(value) if !value.trim().is_empty() => Ok(value),
            _ => Err(HiveError::MissingCredential(self.0.clone())),
        }
    }

    /// Whether the credential is currently available, for status displays that
    /// must not reveal the value.
    pub fn is_available(&self) -> bool {
        self.resolve().is_ok()
    }
}

/// Renders a secret as a short fingerprint for logs and status output.
///
/// Only the last four characters survive, which is enough to tell two keys apart
/// during setup without putting a usable secret into a log file.
pub fn redact(secret: &str) -> String {
    let visible: String = secret
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if secret.chars().count() <= 8 {
        return "****".to_string();
    }
    format!("****{visible}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_names_outside_the_portable_character_set() {
        assert!(SecretRef::new("HIVEMIND_KEY_MAIN").is_ok());
        assert!(SecretRef::new("hivemind key").is_err());
        assert!(SecretRef::new("PATH; rm -rf /").is_err());
        assert!(SecretRef::new("1KEY").is_err());
        assert!(SecretRef::new("").is_err());
    }

    #[test]
    fn unset_variable_reports_missing_credential() {
        let secret = SecretRef::new("HIVEMIND_TEST_DEFINITELY_UNSET").unwrap();
        assert!(matches!(
            secret.resolve(),
            Err(HiveError::MissingCredential(_))
        ));
        assert!(!secret.is_available());
    }

    #[test]
    fn redaction_keeps_only_the_last_four_characters() {
        assert_eq!(redact("example-credential-0123456789abcd"), "****abcd");
        assert_eq!(redact("short"), "****");
    }
}

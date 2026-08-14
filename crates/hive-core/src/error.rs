use std::fmt;

/// Every fallible operation in `hive-core` returns this.
pub type Result<T> = std::result::Result<T, HiveError>;

#[derive(Debug, thiserror::Error)]
pub enum HiveError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error(
        "credential '{0}' is not available: the referenced environment variable is unset or empty"
    )]
    MissingCredential(String),

    #[error("provider '{provider}' failed: {message}")]
    Provider { provider: String, message: String },

    #[error("provider '{0}' is not registered")]
    UnknownProvider(String),

    #[error("agent '{0}' is not a member of this room")]
    UnknownAgent(String),

    #[error("room '{0}' does not exist")]
    UnknownRoom(String),

    #[error("invalid input: {0}")]
    Validation(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("network error: {0}")]
    Network(String),
}

impl HiveError {
    pub fn provider(provider: impl Into<String>, message: impl fmt::Display) -> Self {
        Self::Provider {
            provider: provider.into(),
            message: message.to_string(),
        }
    }
}

impl From<rusqlite::Error> for HiveError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Storage(err.to_string())
    }
}

impl From<serde_json::Error> for HiveError {
    fn from(err: serde_json::Error) -> Self {
        Self::Validation(format!("malformed JSON: {err}"))
    }
}

/// Network failures carry URLs that may embed query parameters, so the message is
/// reduced to the transport-level cause instead of the full request context.
impl From<reqwest::Error> for HiveError {
    fn from(err: reqwest::Error) -> Self {
        let kind = if err.is_timeout() {
            "request timed out"
        } else if err.is_connect() {
            "connection refused"
        } else if err.is_decode() {
            "malformed response body"
        } else {
            "request failed"
        };
        Self::Network(kind.to_string())
    }
}

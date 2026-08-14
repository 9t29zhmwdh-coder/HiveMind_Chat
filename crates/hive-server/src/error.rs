use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use hive_core::HiveError;
use serde::Serialize;

/// The error shape every failing endpoint returns.
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
    #[serde(skip)]
    pub status: StatusCode,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
            status,
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "a valid access token is required")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self)).into_response()
    }
}

/// Maps core errors onto status codes.
///
/// Provider and storage failures deliberately keep their message: it names the
/// provider or the constraint, never the credential, because `HiveError`
/// already strips those.
impl From<HiveError> for ApiError {
    fn from(error: HiveError) -> Self {
        let status = match &error {
            HiveError::Validation(_) | HiveError::Config(_) => StatusCode::BAD_REQUEST,
            HiveError::UnknownRoom(_)
            | HiveError::UnknownAgent(_)
            | HiveError::UnknownProvider(_) => StatusCode::NOT_FOUND,
            HiveError::MissingCredential(_) => StatusCode::FAILED_DEPENDENCY,
            HiveError::Provider { .. } | HiveError::Network(_) => StatusCode::BAD_GATEWAY,
            HiveError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, error.to_string())
    }
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_errors_map_onto_meaningful_status_codes() {
        let cases = [
            (HiveError::Validation("bad".into()), StatusCode::BAD_REQUEST),
            (HiveError::UnknownRoom("r".into()), StatusCode::NOT_FOUND),
            (
                HiveError::MissingCredential("K".into()),
                StatusCode::FAILED_DEPENDENCY,
            ),
            (
                HiveError::Network("timeout".into()),
                StatusCode::BAD_GATEWAY,
            ),
            (
                HiveError::Storage("disk".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(ApiError::from(error).status, expected);
        }
    }

    #[test]
    fn missing_credential_errors_do_not_leak_the_value() {
        let message = ApiError::from(HiveError::MissingCredential("HIVEMIND_KEY_X".into())).error;
        assert!(message.contains("HIVEMIND_KEY_X"));
        assert!(!message.contains("sk-"));
    }
}

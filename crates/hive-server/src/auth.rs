use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Rejects API calls that do not carry the configured access token.
///
/// The token is optional: an instance bound to loopback for a single user does
/// not need one, while an instance reachable from the rest of the network does.
pub async fn require_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> ApiResult<Response> {
    if state.access_token.is_none() {
        return Ok(next.run(request).await);
    }
    let presented = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(bearer_token);

    match state.token_matches(presented) {
        true => Ok(next.run(request).await),
        false => Err(ApiError::unauthorized()),
    }
}

/// Extracts the credential from an `Authorization` header, case-insensitively
/// on the scheme as required by RFC 7235.
pub fn bearer_token(header: &str) -> Option<&str> {
    let (scheme, token) = header.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_tokens_are_extracted_regardless_of_scheme_case() {
        assert_eq!(bearer_token("Bearer abc123"), Some("abc123"));
        assert_eq!(bearer_token("bearer  abc123 "), Some("abc123"));
        assert_eq!(bearer_token("BEARER abc123"), Some("abc123"));
    }

    #[test]
    fn other_schemes_are_ignored() {
        assert_eq!(bearer_token("Basic abc123"), None);
        assert_eq!(bearer_token("abc123"), None);
    }
}

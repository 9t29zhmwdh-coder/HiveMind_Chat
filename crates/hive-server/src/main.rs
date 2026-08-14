//! HiveMind Chat server: REST API, live conversation socket and the web UI.

mod api;
mod auth;
mod error;
mod state;
mod ws;

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use axum::routing::{delete, get, post, put};
use axum::Router;
use clap::Parser;
use hive_core::{HiveConfig, Store, VERSION};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::state::AppState;

/// Upper bound on a request body. Room definitions are small; anything larger
/// is either a mistake or an attempt to exhaust memory.
const MAX_BODY_BYTES: usize = 256 * 1024;

#[derive(Parser, Debug)]
#[command(
    name = "hivemind-server",
    version,
    about = "Serves HiveMind Chat over HTTP and WebSocket"
)]
struct Args {
    /// Path to the TOML configuration file.
    #[arg(long, default_value = "hivemind.toml", env = "HIVEMIND_CONFIG")]
    config: PathBuf,

    /// Address to bind to, overriding the configuration file.
    #[arg(long, env = "HIVEMIND_BIND")]
    bind: Option<String>,

    /// SQLite database file, overriding the configuration file.
    #[arg(long, env = "HIVEMIND_DATABASE")]
    database: Option<PathBuf>,

    /// Directory containing the built web UI.
    #[arg(long, default_value = "frontend/dist", env = "HIVEMIND_WEB_ROOT")]
    web_root: PathBuf,

    /// Shared access token. When set, every API call and socket must present it.
    #[arg(long, env = "HIVEMIND_ACCESS_TOKEN", hide_env_values = true)]
    access_token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let config = HiveConfig::load_or_default(&args.config)
        .with_context(|| format!("cannot load {}", args.config.display()))?;

    let bind: SocketAddr = args
        .bind
        .unwrap_or_else(|| config.server.bind.clone())
        .parse()
        .context("the bind address is not a valid host:port")?;
    let database = args
        .database
        .unwrap_or_else(|| PathBuf::from(&config.server.database));

    let store =
        Store::open(&database).with_context(|| format!("cannot open {}", database.display()))?;
    let state = AppState::new(store, config, args.access_token)?;

    warn_if_exposed(&bind, &state);
    report_providers(&state);

    let app = build_router(state, &args.web_root);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, version = VERSION, "HiveMind Chat is listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("the server stopped unexpectedly")
}

/// An instance reachable from the network without a token is open to everyone
/// who can route to it, including anything else on the same LAN.
fn warn_if_exposed(bind: &SocketAddr, state: &AppState) {
    if !bind.ip().is_loopback() && state.access_token.is_none() {
        tracing::warn!(
            "bound to {bind} without an access token: set HIVEMIND_ACCESS_TOKEN before exposing this instance"
        );
    }
}

fn report_providers(state: &AppState) {
    for provider in &state.config.providers {
        let credential = match provider.secret_ref().ok().flatten() {
            Some(secret) if secret.is_available() => "credential resolved",
            Some(_) => "credential MISSING",
            None => "no credential needed",
        };
        tracing::info!(id = %provider.id, url = %provider.resolved_base_url(), "{credential}");
    }
}

fn build_router(state: AppState, web_root: &PathBuf) -> Router {
    // The socket authenticates itself in its first frame, so it must stay
    // outside the header-based middleware.
    let open = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/rooms/{room_id}/ws", get(ws::upgrade));

    let protected = Router::new()
        .route("/api/providers", get(api::list_providers))
        .route(
            "/api/providers/{provider_id}/models",
            get(api::provider_models),
        )
        .route("/api/policies", get(api::list_policies))
        .route("/api/rooms", get(api::list_rooms).post(api::create_room))
        .route("/api/rooms/{room_id}", get(api::get_room))
        .route("/api/rooms/{room_id}", put(api::update_room))
        .route("/api/rooms/{room_id}", delete(api::delete_room))
        .route("/api/rooms/{room_id}/duplicate", post(api::duplicate_room))
        .route(
            "/api/rooms/{room_id}/transcript",
            get(api::export_transcript),
        )
        .route(
            "/api/rooms/{room_id}/transcript",
            delete(api::clear_transcript),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_token,
        ));

    open.merge(protected)
        .fallback_service(web_ui(web_root))
        .layer(cors_layer(&state))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Serves the built UI, falling back to `index.html` so client-side routes work.
fn web_ui(web_root: &PathBuf) -> ServeDir<ServeFile> {
    ServeDir::new(web_root).fallback(ServeFile::new(web_root.join("index.html")))
}

/// Same-origin only unless the configuration names other origins, because the
/// server ships the UI it serves.
fn cors_layer(state: &AppState) -> CorsLayer {
    let origins = &state.config.server.allowed_origins;
    if origins.is_empty() {
        return CorsLayer::new();
    }
    let parsed: Vec<_> = origins
        .iter()
        .filter_map(|origin| origin.parse().ok())
        .collect();
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(parsed))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any)
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app(token: Option<&str>) -> Router {
        let state = AppState::new(
            Store::in_memory().unwrap(),
            HiveConfig::local_default(),
            token.map(str::to_string),
        )
        .unwrap();
        build_router(state, &PathBuf::from("frontend/dist"))
    }

    /// Sends a request and parses the JSON body.
    async fn json_call(
        app: Router,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let mut request = Request::builder().method(method).uri(uri);
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let payload = body
            .map(|value| Body::from(value.to_string()))
            .unwrap_or_else(Body::empty);
        let response = app.oneshot(request.body(payload).unwrap()).await.unwrap();
        assert!(
            response.status().is_success(),
            "{method} {uri} returned {}",
            response.status()
        );
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn status(app: Router, uri: &str, token: Option<&str>) -> StatusCode {
        let mut request = Request::builder().uri(uri);
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        app.oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn health_needs_no_token() {
        assert_eq!(
            status(app(Some("s3cret")), "/api/health", None).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn protected_routes_reject_a_missing_or_wrong_token() {
        assert_eq!(
            status(app(Some("s3cret")), "/api/rooms", None).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(app(Some("s3cret")), "/api/rooms", Some("nope")).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(app(Some("s3cret")), "/api/rooms", Some("s3cret")).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn an_instance_without_a_token_serves_the_api_openly() {
        assert_eq!(status(app(None), "/api/rooms", None).await, StatusCode::OK);
        assert_eq!(
            status(app(None), "/api/policies", None).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn unknown_rooms_return_not_found() {
        assert_eq!(
            status(app(None), "/api/rooms/does-not-exist", None).await,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn a_room_survives_create_and_fetch() {
        let app = app(None);
        let body = serde_json::json!({
            "name": "Lab",
            "policy": "round_robin",
            "agents": [{"name": "Scout", "provider_id": "local", "model": "llama3:8b"}],
        });
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rooms")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let room_id = created["id"].as_str().unwrap();

        assert_eq!(
            status(app, &format!("/api/rooms/{room_id}"), None).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn duplicating_a_room_creates_a_second_one_with_its_own_agents() {
        let app = app(None);
        let body = serde_json::json!({
            "name": "Lab",
            "policy": "parallel",
            "agents": [{"name": "Scout", "provider_id": "local", "model": "llama3:8b"}],
        });
        let created: serde_json::Value =
            json_call(app.clone(), "POST", "/api/rooms", Some(body)).await;
        let room_id = created["id"].as_str().unwrap().to_string();

        let copy: serde_json::Value = json_call(
            app.clone(),
            "POST",
            &format!("/api/rooms/{room_id}/duplicate"),
            None,
        )
        .await;

        assert_eq!(copy["name"], "Lab (copy)");
        assert_ne!(copy["id"], created["id"]);
        assert_ne!(copy["agents"][0]["id"], created["agents"][0]["id"]);

        let rooms: serde_json::Value = json_call(app, "GET", "/api/rooms", None).await;
        assert_eq!(rooms.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn duplicating_an_unknown_room_is_not_found() {
        let response = app(None)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rooms/does-not-exist/duplicate")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn rooms_with_unknown_providers_are_rejected() {
        let body = serde_json::json!({
            "name": "Lab",
            "policy": "round_robin",
            "agents": [{"name": "Scout", "provider_id": "ghost", "model": "x"}],
        });
        let response = app(None)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rooms")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}

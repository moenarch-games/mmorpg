//! The HTTP face of the studio. It edits branches and starts programs, so it only ever listens
//! on, and answers to, the loopback address.
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use tower_http::services::{ServeDir, ServeFile};

use crate::studio::{Refusal, Selection, Studio};

pub const DEFAULT_PORT: u16 = 8737;

const MAX_DOCUMENT_BYTES: usize = 1 << 30;
/// The layer images of an export that the app may hand out.
const LAYERS: &[&str] = &[
    "beauty",
    "ground",
    "height",
    "hillshade",
    "slope",
    "biome",
    "roads",
    "water",
    "locks",
];

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND.into_response(),
            Self::Invalid(message) => (StatusCode::UNPROCESSABLE_ENTITY, message).into_response(),
        }
    }
}

/// Whether a `Host` or `Origin` authority names this machine's loopback.
fn is_loopback(authority: &str) -> bool {
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host);
    matches!(host, "127.0.0.1" | "localhost")
}

/// Keeps other websites out: a page elsewhere must not start jobs through the visitor's browser.
async fn loopback_only(headers: HeaderMap, request: Request, next: Next) -> Response {
    let text = |name| headers.get(name).and_then(|value| value.to_str().ok());
    let host_ok = text(header::HOST).is_some_and(is_loopback);
    let origin_ok = text(header::ORIGIN)
        .is_none_or(|origin| origin.strip_prefix("http://").is_some_and(is_loopback));
    if host_ok && origin_ok {
        next.run(request).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

async fn state(State(studio): State<Arc<Studio>>) -> Json<serde_json::Value> {
    Json(studio.snapshot())
}

async fn refresh_tiles(State(studio): State<Arc<Studio>>) -> StatusCode {
    studio.refresh_tiles();
    StatusCode::ACCEPTED
}

async fn create_export(
    State(studio): State<Arc<Studio>>,
    Json(selection): Json<Selection>,
) -> Result<Json<serde_json::Value>, Refusal> {
    let id = studio.export(selection)?;
    Ok(Json(serde_json::json!({ "id": id })))
}

async fn file(path: PathBuf, content_type: &'static str, disposition: Option<String>) -> Response {
    let Ok(bytes) = tokio::fs::read(path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut response = Response::new(Body::from(bytes));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(content_type),
    );
    if let Some(value) = disposition.and_then(|value| value.parse().ok()) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

async fn document(State(studio): State<Arc<Studio>>, Path(id): Path<String>) -> Response {
    let Some(folder) = studio.export_folder(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    file(
        folder.join("selection.ora"),
        "image/openraster",
        Some(format!("attachment; filename=\"greyhaven-{id}.ora\"")),
    )
    .await
}

async fn layer(
    State(studio): State<Arc<Studio>>,
    Path((id, layer)): Path<(String, String)>,
) -> Response {
    let Some(folder) = studio.export_folder(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !LAYERS.contains(&layer.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    file(
        folder.join("selection").join(format!("{layer}.png")),
        "image/png",
        None,
    )
    .await
}

async fn upload(
    State(studio): State<Arc<Studio>>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, Refusal> {
    let edit = studio.upload(&id, &body)?;
    Ok(Json(serde_json::json!({ "id": edit })))
}

async fn apply(
    State(studio): State<Arc<Studio>>,
    Path(id): Path<String>,
) -> Result<StatusCode, Refusal> {
    studio.apply(&id)?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
struct Direction {
    direction: String,
}

async fn reconcile(
    State(studio): State<Arc<Studio>>,
    Path(id): Path<String>,
    Json(body): Json<Direction>,
) -> Result<StatusCode, Refusal> {
    studio.reconcile(&id, &body.direction)?;
    Ok(StatusCode::ACCEPTED)
}

/// Runs the web app until it fails.
pub async fn serve(repo: PathBuf, port: u16) -> Result<(), String> {
    let agent = std::env::var_os("WORLD_STUDIO_AGENT")
        .map_or_else(|| PathBuf::from("claude"), PathBuf::from);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|error| format!("could not listen on port {port}: {error}"))?;
    println!("World Studio: http://127.0.0.1:{port}/");
    axum::serve(listener, app(Studio::open(repo, agent)))
        .await
        .map_err(|error| format!("server stopped: {error}"))
}

fn app(studio: Arc<Studio>) -> Router {
    let frontend = studio
        .repo()
        .join("crates/mmorpg-world-studio/frontend/dist");
    let tiles = ServeDir::new(studio.tiles_root());
    Router::new()
        .route("/api/state", get(state))
        .route("/api/tiles", post(refresh_tiles))
        .route("/api/exports", post(create_export))
        .route("/api/exports/{id}/document", get(document))
        .route("/api/exports/{id}/layers/{layer}", get(layer))
        .route("/api/exports/{id}/import", post(upload))
        .route("/api/edits/{id}/apply", post(apply))
        .route("/api/edits/{id}/reconcile", post(reconcile))
        .nest_service("/tiles", tiles)
        .fallback_service(
            ServeDir::new(&frontend).fallback(ServeFile::new(frontend.join("index.html"))),
        )
        .layer(DefaultBodyLimit::max(MAX_DOCUMENT_BYTES))
        .layer(middleware::from_fn(loopback_only))
        .with_state(studio)
}

#[cfg(test)]
mod tests {
    use super::is_loopback;

    #[test]
    fn only_this_machine_may_talk_to_the_studio() {
        for good in ["127.0.0.1:8737", "localhost:8737", "localhost"] {
            assert!(is_loopback(good), "{good}");
        }
        for bad in ["evil.example:8737", "127.0.0.1.evil.example", "example.com"] {
            assert!(!is_loopback(bad), "{bad}");
        }
    }
}

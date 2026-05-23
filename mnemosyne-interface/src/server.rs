use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures::Stream;
use serde::Deserialize;
use tower_http::cors::CorsLayer;

use mnemosyne_inference_engine::InferenceEngine;

static INDEX_HTML: &str = include_str!("../assets/index.html");

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    engine: Arc<InferenceEngine>,
}

// ── Config ────────────────────────────────────────────────────────────────────

pub struct ServerConfig {
    pub bind: SocketAddr,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:3000".parse().unwrap(),
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run_server(engine: Arc<InferenceEngine>, config: ServerConfig) -> anyhow::Result<()> {
    let state = AppState { engine };
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/chat", post(chat_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!("listening on http://{}", config.bind);
    axum::serve(listener, app).await?;
    Ok(())
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn index_handler() -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(INDEX_HTML.to_owned())
        .unwrap()
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    /// Passed through from the client but not yet used for session routing.
    #[allow(dead_code)]
    session_id: Option<String>,
}

async fn chat_handler(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let engine = state.engine.clone();
    let message = req.message;

    let stream = async_stream::stream! {
        match engine.run(message).await {
            Ok(response) => {
                let payload = serde_json::json!({ "text": response }).to_string();
                yield Ok(Event::default().event("token").data(payload));
                yield Ok(Event::default().event("done").data("{}"));
            }
            Err(e) => {
                let payload = serde_json::json!({ "error": e.to_string() }).to_string();
                yield Ok(Event::default().event("error").data(payload));
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

pub mod api;
pub mod db;
pub mod events;
pub mod spotify_api;

pub use db::Db;
pub use events::{PlayerCommand, WebEvent};
pub use spotify_api::SpotifyApi;

use crate::auth::AuthManager;
use crate::state::StateManager;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use rust_embed::Embed;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tower_http::cors::CorsLayer;

#[derive(Embed)]
#[folder = "frontend/build/"]
struct FrontendAssets;

const INDEX_HTML: &str = "index.html";

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub auth_manager: Arc<AuthManager>,
    pub spotify_api: SpotifyApi,
    pub event_tx: broadcast::Sender<WebEvent>,
    pub player_cmd_tx: mpsc::Sender<PlayerCommand>,
    pub state_manager: StateManager,
}

pub fn create_router(state: AppState) -> Router {
    let api_routes = Router::new()
        .route("/auth/url", get(api::get_auth_url))
        .route("/auth/callback", get(api::auth_callback))
        .route("/auth/status", get(api::get_auth_status))
        .route("/auth/logout", post(api::auth_logout))
        .route("/cards", get(api::list_cards).post(api::add_card))
        .route("/cards/:id", axum::routing::delete(api::delete_card))
        .route("/playlists", get(api::get_playlists))
        .route("/now-playing", get(api::get_now_playing))
        .route("/state", get(api::get_state))
        .route("/last-card", get(api::get_last_card))
        .route("/player/play", post(api::player_play))
        .route("/player/pause", post(api::player_pause))
        .route("/player/next", post(api::player_next))
        .route("/player/previous", post(api::player_previous))
        .with_state(state.clone());

    Router::new()
        .nest("/api", api_routes)
        .route("/ws", axum::routing::any(ws_handler))
        .with_state(state)
        .fallback(static_handler)
        .layer(CorsLayer::very_permissive())
}

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if path.is_empty() || path == INDEX_HTML {
        return index_html().await;
    }

    match FrontendAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data,
            )
                .into_response()
        }
        None => {
            if path.contains('.') {
                return (
                    StatusCode::NOT_FOUND,
                    "404 Not Found",
                )
                    .into_response();
            }
            index_html().await
        }
    }
}

async fn index_html() -> Response {
    match FrontendAssets::get(INDEX_HTML) {
        Some(content) => Html(content.data).into_response(),
        None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    let mut rx = state.event_tx.subscribe();
    ws.on_upgrade(|mut socket: WebSocket| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let json = match serde_json::to_string(&event) {
                        Ok(j) => j,
                        Err(_) => continue,
                    };
                    if socket.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    })
}

pub async fn start_web_server(state: AppState, addr: &str) -> anyhow::Result<()> {
    let router = create_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    log::info!("Web server listening on {}", addr);
    println!("Web UI: http://{}", addr);
    axum::serve(listener, router).await?;
    Ok(())
}

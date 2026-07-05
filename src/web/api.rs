use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

use super::AppState;
use crate::web::events::PlayerCommand;

#[derive(Debug)]
pub enum ApiError {
    Internal(String),
    Unauthorized,
    BadRequest(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
        };
        (
            status,
            Json(serde_json::json!({ "error": message })),
        )
            .into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e.to_string())
    }
}

#[derive(Debug, Deserialize)]
pub struct AuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CardMappingRequest {
    pub card_id: String,
    pub playlist_uri: String,
    pub playlist_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthUrlResponse {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct AuthStatusResponse {
    pub authenticated: bool,
}

#[derive(Debug, Serialize)]
pub struct LastCardResponse {
    pub card_id: String,
}

pub async fn get_auth_url(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AuthUrlResponse>, ApiError> {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost:8080");
    let origin = format!("http://{}", host);

    let csrf = uuid::Uuid::new_v4().to_string();
    let state_data = serde_json::json!({
        "csrf": csrf,
        "origin": origin,
    });
    let state_encoded = URL_SAFE_NO_PAD.encode(state_data.to_string());

    let url = state.auth_manager.get_auth_url_with_state(&state_encoded);
    Ok(Json(AuthUrlResponse { url }))
}

pub async fn auth_callback(
    State(state): State<AppState>,
    Query(query): Query<AuthCallbackQuery>,
) -> Result<Redirect, ApiError> {
    if let Some(err) = query.error {
        return Err(ApiError::BadRequest(format!("Auth error: {}", err)));
    }

    let code = query.code.ok_or_else(|| {
        ApiError::BadRequest("Missing 'code' query parameter".to_string())
    })?;
    let state_param = query.state.unwrap_or_default();

    let fake_url = format!("http://localhost?code={}&state={}", code, state_param);
    state
        .auth_manager
        .authenticate_with_redirect_url(&fake_url)
        .await
        .map_err(|e| ApiError::Internal(format!("Authentication failed: {}", e)))?;

    Ok(Redirect::to("/"))
}

pub async fn get_auth_status(
    State(state): State<AppState>,
) -> Result<Json<AuthStatusResponse>, ApiError> {
    let authenticated = state.auth_manager.has_valid_token().await;
    Ok(Json(AuthStatusResponse { authenticated }))
}

pub async fn auth_logout(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    state.auth_manager.clear_token().await;
    Ok(Json(serde_json::json!({})))
}

pub async fn list_cards(State(state): State<AppState>) -> Result<Json<Vec<crate::web::db::CardMapping>>, ApiError> {
    let mappings = state.db.get_all_mappings()?;
    Ok(Json(mappings))
}

pub async fn add_card(
    State(state): State<AppState>,
    Json(body): Json<CardMappingRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    state
        .db
        .add_card_mapping(&body.card_id, &body.playlist_uri, body.playlist_name.as_deref())
        .map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({}))))
}

pub async fn delete_card(
    State(state): State<AppState>,
    Path(card_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .db
        .remove_card_mapping(&card_id)
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({})))
}

pub async fn get_playlists(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::web::spotify_api::PlaylistInfo>>, ApiError> {
    let token = state
        .auth_manager
        .ensure_valid_token()
        .await
        .map_err(|_| ApiError::Unauthorized)?;
    let playlists = state
        .spotify_api
        .get_user_playlists(&token.access_token)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(playlists))
}

pub async fn get_now_playing(
    State(state): State<AppState>,
) -> Result<Json<Option<crate::web::spotify_api::CurrentlyPlaying>>, ApiError> {
    let token = state
        .auth_manager
        .ensure_valid_token()
        .await
        .map_err(|_| ApiError::Unauthorized)?;
    let playing = state
        .spotify_api
        .get_currently_playing(&token.access_token)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(playing))
}

#[derive(Debug, Serialize)]
pub struct StateResponse {
    pub is_playing: bool,
    pub current_card: Option<String>,
    pub context_uri: Option<String>,
}

pub async fn get_state(State(state): State<AppState>) -> Result<Json<StateResponse>, ApiError> {
    let (is_playing, current_card, context_uri) = state
        .state_manager
        .with_state(|s| {
            (s.is_playing, s.current_id.clone(), s.context_uri.clone())
        })
        .map_err(ApiError::from)?;
    Ok(Json(StateResponse {
        is_playing,
        current_card,
        context_uri,
    }))
}

pub async fn get_last_card(
    State(state): State<AppState>,
) -> Result<Json<Option<LastCardResponse>>, ApiError> {
    let last = state.db.get_last_card().map_err(ApiError::from)?;
    Ok(Json(last.map(|card_id| LastCardResponse { card_id })))
}

pub async fn player_play(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _ = state.player_cmd_tx.send(PlayerCommand::Play).await;
    Ok(Json(serde_json::json!({})))
}

pub async fn player_pause(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _ = state.player_cmd_tx.send(PlayerCommand::Pause).await;
    Ok(Json(serde_json::json!({})))
}

pub async fn player_next(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _ = state.player_cmd_tx.send(PlayerCommand::Next).await;
    Ok(Json(serde_json::json!({})))
}

pub async fn player_previous(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _ = state
        .player_cmd_tx
        .send(PlayerCommand::Previous)
        .await;
    Ok(Json(serde_json::json!({})))
}

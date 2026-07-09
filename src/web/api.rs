use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

use super::AppState;
use crate::web::events::PlayerCommand;
use super::MAX_PENDING_AUTH_STATES;

#[derive(Debug, Serialize, Deserialize)]
struct OAuthStateData {
    csrf: String,
    origin: String,
}

#[derive(Debug)]
pub enum ApiError {
    Internal(String),
    Unauthorized,
    BadRequest(String),
    RateLimited,
    ServiceUnavailable,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "Rate limited".to_string()),
            ApiError::ServiceUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "Service unavailable".to_string()),
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

impl From<crate::web::spotify_api::SpotifyApiError> for ApiError {
    fn from(e: crate::web::spotify_api::SpotifyApiError) -> Self {
        match e {
            crate::web::spotify_api::SpotifyApiError::Unauthorized => ApiError::Unauthorized,
            crate::web::spotify_api::SpotifyApiError::RateLimited => ApiError::RateLimited,
            other => ApiError::Internal(other.to_string()),
        }
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
    let origin = if let Some(ref configured) = state.web_origin {
        configured.clone()
    } else {
        let host = headers
            .get("host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("localhost:8080");
        format!("http://{}", host)
    };

    let csrf = uuid::Uuid::new_v4().to_string();
    {
        let mut states = state.pending_auth_states.lock().await;
        if states.len() >= MAX_PENDING_AUTH_STATES {
            // Remove oldest entries rather than nuking all — keep the most
            // recent ones so concurrent auth flows don't invalidate each other.
            let to_remove: Vec<String> = states.iter().take(states.len() / 2).cloned().collect();
            for key in to_remove {
                states.remove(&key);
            }
        }
        states.insert(csrf.clone());
    }
    let state_data = OAuthStateData {
        csrf: csrf.clone(),
        origin: origin.clone(),
    };
    let state_json = serde_json::to_string(&state_data)
        .map_err(|e| ApiError::Internal(format!("Failed to encode state: {}", e)))?;
    let state_encoded = URL_SAFE_NO_PAD.encode(state_json);

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

    // The auth proxy passes the original base64url-encoded state JSON
    // through unchanged. Decode it to extract the CSRF token.
    let state_json = URL_SAFE_NO_PAD
        .decode(&state_param)
        .map_err(|_| ApiError::BadRequest("Invalid state parameter".to_string()))?;
    let state_data: OAuthStateData = serde_json::from_slice(&state_json)
        .map_err(|_| ApiError::BadRequest("Malformed state parameter".to_string()))?;
    let csrf = &state_data.csrf;

    // Check CSRF token against pending set
    let valid = {
        let mut states = state.pending_auth_states.lock().await;
        states.remove(csrf)
    };
    if !valid {
        return Err(ApiError::BadRequest("Invalid or expired CSRF state".to_string()));
    }

    state
        .auth_manager
        .exchange_code_for_token(&code)
        .await
        .map_err(|e| ApiError::Internal(format!("Authentication failed: {}", e)))?;

    Ok(Redirect::to("/"))
}

pub async fn get_auth_status(
    State(state): State<AppState>,
) -> Result<Json<AuthStatusResponse>, ApiError> {
    let authenticated = state.auth_manager.has_valid_token().map_err(ApiError::from)?;
    Ok(Json(AuthStatusResponse { authenticated }))
}

pub async fn auth_logout(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    state.auth_manager.clear_token().await.map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({})))
}

pub async fn list_cards(State(state): State<AppState>) -> Result<Json<Vec<crate::web::db::CardMapping>>, ApiError> {
    let mappings = state.db.get_all_mappings().await?;
    Ok(Json(mappings))
}

pub async fn add_card(
    State(state): State<AppState>,
    Json(body): Json<CardMappingRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    state
        .db
        .add_card_mapping(&body.card_id, &body.playlist_uri, body.playlist_name.as_deref())
        .await
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
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({})))
}

pub async fn get_playlists(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::web::spotify_api::PlaylistInfo>>, ApiError> {
    let playlists = with_spotify_token(&state, |api, token| {
        Box::pin(api.get_user_playlists(token))
    })
    .await?;

    let uri_to_name: std::collections::HashMap<String, String> = playlists
        .iter()
        .map(|p| (p.uri.clone(), p.name.clone()))
        .collect();
    if let Err(e) = state.db.backfill_playlist_names(&uri_to_name).await {
        log::warn!("Failed to backfill playlist names: {}", e);
    }
    Ok(Json(playlists))
}

pub async fn get_now_playing(
    State(state): State<AppState>,
) -> Result<Json<Option<crate::web::spotify_api::CurrentlyPlaying>>, ApiError> {
    let playing = with_spotify_token(&state, |api, token| {
        Box::pin(api.get_currently_playing(token))
    })
    .await?;
    Ok(Json(playing))
}

async fn with_spotify_token<T>(
    state: &AppState,
    f: impl for<'a> Fn(
        &'a crate::web::spotify_api::SpotifyApi,
        &'a str,
    ) -> BoxFuture<'a, Result<T, crate::web::spotify_api::SpotifyApiError>>,
) -> Result<T, ApiError> {
    let token = state
        .auth_manager
        .ensure_valid_token()
        .await
        .map_err(|_| ApiError::Unauthorized)?;

    match f(&state.spotify_api, &token.access_token).await {
        Ok(result) => Ok(result),
        Err(crate::web::spotify_api::SpotifyApiError::Unauthorized) => {
            let token = state
                .auth_manager
                .refresh_token()
                .await
                .map_err(|_| ApiError::Unauthorized)?;
            f(&state.spotify_api, &token.access_token)
                .await
                .map_err(ApiError::from)
        }
        Err(e) => Err(ApiError::from(e)),
    }
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
    let last = state.db.get_last_card().await.map_err(ApiError::from)?;
    Ok(Json(last.map(|card_id| LastCardResponse { card_id })))
}

pub async fn player_play(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    send_player_command(&state, PlayerCommand::Play).await
}

pub async fn player_pause(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    send_player_command(&state, PlayerCommand::Pause).await
}

pub async fn player_next(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    send_player_command(&state, PlayerCommand::Next).await
}

pub async fn player_previous(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    send_player_command(&state, PlayerCommand::Previous).await
}

async fn send_player_command(
    state: &AppState,
    cmd: PlayerCommand,
) -> Result<Json<serde_json::Value>, ApiError> {
    if state.player_cmd_tx.send(cmd).await.is_err() {
        log::warn!("Failed to send player command: channel closed");
        return Err(ApiError::ServiceUnavailable);
    }
    Ok(Json(serde_json::json!({})))
}

#[derive(Debug, Serialize)]
pub struct VolumeResponse {
    pub volume: u8,
}

#[derive(Debug, Deserialize)]
pub struct VolumeRequest {
    pub volume: u8,
}

pub async fn get_volume(State(state): State<AppState>) -> Result<Json<VolumeResponse>, ApiError> {
    let volume = state.db.get_volume().await?.unwrap_or(state.default_volume);
    Ok(Json(VolumeResponse { volume }))
}

pub async fn set_volume(
    State(state): State<AppState>,
    Json(body): Json<VolumeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if body.volume > 100 {
        return Err(ApiError::BadRequest("Volume must be 0-100".to_string()));
    }
    if state
        .player_cmd_tx
        .send(PlayerCommand::SetVolume(body.volume))
        .await
        .is_err()
    {
        log::warn!("Failed to send set_volume command: channel closed");
        return Err(ApiError::ServiceUnavailable);
    }
    Ok(Json(serde_json::json!({})))
}

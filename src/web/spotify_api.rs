use serde::{Deserialize, Serialize};

const API_BASE: &str = "https://api.spotify.com/v1";

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistInfo {
    pub id: String,
    pub uri: String,
    pub name: String,
    pub images: Vec<String>,
    pub owner: String,
    pub track_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CurrentlyPlaying {
    pub track_name: String,
    pub artist: String,
    pub album: String,
    pub album_art: Option<String>,
    pub is_playing: bool,
    pub progress_ms: u32,
    pub duration_ms: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum SpotifyApiError {
    #[error("Unauthorized (401)")]
    Unauthorized,
    #[error("HTTP error: {0}")]
    HttpError(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Rate limited (429)")]
    RateLimited,
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Deserialize)]
struct PlaylistsResponse {
    items: Vec<PlaylistItem>,
}

#[derive(Debug, Deserialize)]
struct PlaylistItem {
    id: String,
    uri: String,
    name: Option<String>,
    images: Vec<PlaylistImage>,
    owner: PlaylistOwner,
    tracks: PlaylistTracks,
}

#[derive(Debug, Deserialize, Default)]
struct PlaylistImage {
    url: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct PlaylistOwner {
    display_name: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct PlaylistTracks {
    total: u64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct CurrentlyPlayingResponse {
    is_playing: bool,
    progress_ms: u64,
    item: Option<CurrentlyPlayingItem>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct CurrentlyPlayingItem {
    name: String,
    duration_ms: u64,
    artists: Vec<CurrentlyPlayingArtist>,
    album: CurrentlyPlayingAlbum,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct CurrentlyPlayingArtist {
    name: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct CurrentlyPlayingAlbum {
    name: String,
    images: Vec<PlaylistImage>,
}

#[derive(Clone)]
pub struct SpotifyApi {
    http_client: reqwest::Client,
}

impl SpotifyApi {
    pub fn new() -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("default reqwest client must build");
        Self { http_client }
    }

    pub async fn get_user_playlists(&self, access_token: &str) -> Result<Vec<PlaylistInfo>, SpotifyApiError> {
        let response = self
            .http_client
            .get(format!("{}{}", API_BASE, "/me/playlists?limit=50"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| SpotifyApiError::Other(format!("Failed to fetch playlists: {}", e)))?;

        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(Vec::new());
        }

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(SpotifyApiError::RateLimited);
        }

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(SpotifyApiError::Unauthorized);
        }

        if !response.status().is_success() {
            let status = response.status();
            // unwrap_or_default: only for the error message body, not the success path
            let body = response.text().await.unwrap_or_default();
            return Err(SpotifyApiError::HttpError(format!("Spotify API error ({}): {}", status, body)));
        }

        let parsed: PlaylistsResponse = response
            .json()
            .await
            .map_err(|e| SpotifyApiError::ParseError(format!("Failed to parse playlists: {}", e)))?;

        let playlists = parsed.items.into_iter().map(|item| {
            let images: Vec<String> = item.images.into_iter().map(|img| img.url).filter(|u| !u.is_empty()).collect();
            PlaylistInfo {
                id: item.id,
                uri: item.uri,
                name: item.name.unwrap_or_default(),
                images,
                owner: item.owner.display_name,
                track_count: item.tracks.total as u32,
            }
        }).collect();

        Ok(playlists)
    }

    pub async fn get_currently_playing(&self, access_token: &str) -> Result<Option<CurrentlyPlaying>, SpotifyApiError> {
        let response = self
            .http_client
            .get(format!("{}{}", API_BASE, "/me/player/currently-playing"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| SpotifyApiError::Other(format!("Failed to fetch currently playing: {}", e)))?;

        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(SpotifyApiError::RateLimited);
        }

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(SpotifyApiError::Unauthorized);
        }

        if !response.status().is_success() {
            let status = response.status();
            // unwrap_or_default: only for the error message body, not the success path
            let body = response.text().await.unwrap_or_default();
            return Err(SpotifyApiError::HttpError(format!("Spotify API error ({}): {}", status, body)));
        }

        let parsed: CurrentlyPlayingResponse = response
            .json()
            .await
            .map_err(|e| SpotifyApiError::ParseError(format!("Failed to parse currently playing: {}", e)))?;

        match parsed.item {
            Some(item) => {
                let artist = item.artists.first().map(|a| a.name.clone()).unwrap_or_default();
                let album_art = item.album.images.first().map(|img| img.url.clone()).filter(|u| !u.is_empty());
                Ok(Some(CurrentlyPlaying {
                    track_name: item.name,
                    artist,
                    album: item.album.name,
                    album_art,
                    is_playing: parsed.is_playing,
                    progress_ms: parsed.progress_ms as u32,
                    duration_ms: item.duration_ms as u32,
                }))
            }
            None => Ok(None),
        }
    }
}

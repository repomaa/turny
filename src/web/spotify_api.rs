use anyhow::{Context, Result};
use serde::Serialize;

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

#[derive(Clone)]
pub struct SpotifyApi {
    http_client: reqwest::Client,
}

impl SpotifyApi {
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::new(),
        }
    }

    pub async fn get_user_playlists(&self, access_token: &str) -> Result<Vec<PlaylistInfo>> {
        let response = self
            .http_client
            .get("https://api.spotify.com/v1/me/playlists?limit=50")
            .bearer_auth(access_token)
            .send()
            .await
            .context("Failed to fetch user playlists")?;

        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(Vec::new());
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Spotify API error ({}): {}",
                status,
                body
            ));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse playlists response")?;

        let items = json["items"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or(&[]);

        let playlists = items
            .iter()
            .map(|item| {
                let id = item["id"].as_str().unwrap_or("").to_string();
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                let name = item["name"].as_str().unwrap_or("").to_string();
                let images: Vec<String> = item["images"]
                    .as_array()
                    .map(|imgs| {
                        imgs.iter()
                            .filter_map(|i| i["url"].as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let owner = item["owner"]["display_name"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let track_count = item["tracks"]["total"].as_u64().unwrap_or(0) as u32;

                PlaylistInfo {
                    id,
                    uri,
                    name,
                    images,
                    owner,
                    track_count,
                }
            })
            .collect();

        Ok(playlists)
    }

    pub async fn get_currently_playing(&self, access_token: &str) -> Result<Option<CurrentlyPlaying>> {
        let response = self
            .http_client
            .get("https://api.spotify.com/v1/me/player/currently-playing")
            .bearer_auth(access_token)
            .send()
            .await
            .context("Failed to fetch currently playing")?;

        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Spotify API error ({}): {}",
                status,
                body
            ));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse currently playing response")?;

        let is_playing = json["is_playing"].as_bool().unwrap_or(false);
        let progress_ms = json["progress_ms"].as_u64().unwrap_or(0) as u32;

        let item = &json["item"];
        let track_name = item["name"].as_str().unwrap_or("").to_string();
        let duration_ms = item["duration_ms"].as_u64().unwrap_or(0) as u32;

        let artist = item["artists"]
            .as_array()
            .and_then(|artists| artists.first())
            .and_then(|a| a["name"].as_str())
            .unwrap_or("")
            .to_string();

        let album = item["album"]["name"].as_str().unwrap_or("").to_string();
        let album_art = item["album"]["images"]
            .as_array()
            .and_then(|imgs| imgs.first())
            .and_then(|i| i["url"].as_str())
            .map(|s| s.to_string());

        Ok(Some(CurrentlyPlaying {
            track_name,
            artist,
            album,
            album_art,
            is_playing,
            progress_ms,
            duration_ms,
        }))
    }
}

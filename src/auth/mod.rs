use anyhow::{Context, Result};
use log::{info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use std::sync::{Arc, Mutex};

use crate::web::Db;

/// OAuth token information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_in: Option<u64>,
    pub scope: Option<String>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl TokenInfo {
    #[allow(dead_code)]
    pub fn new(access_token: String, refresh_token: Option<String>) -> Self {
        Self {
            access_token,
            refresh_token,
            token_type: "Bearer".to_string(),
            expires_in: None,
            scope: None,
            expires_at: None,
        }
    }

    /// Check if the token is expired
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(expires_at) => chrono::Utc::now() > expires_at,
            None => false, // If no expiration time, assume it's valid
        }
    }

    /// Check if the token will expire soon (within the given duration)
    pub fn will_expire_soon(&self, duration: chrono::Duration) -> bool {
        match self.expires_at {
            Some(expires_at) => chrono::Utc::now() + duration > expires_at,
            None => false,
        }
    }

    /// Update expiration time based on expires_in seconds
    pub fn update_expiration(&mut self) {
        if let Some(expires_in) = self.expires_in {
            self.expires_at =
                Some(chrono::Utc::now() + chrono::Duration::seconds(expires_in as i64));
        }
    }
}

/// OAuth authentication manager.
///
/// Uses `std::sync::Mutex` for interior mutability. This is safe because no
/// `.await` is ever called while holding the lock — all network I/O happens
/// after the lock is dropped. Do not add `.await` calls inside locked sections.
pub struct AuthManager {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    scopes: Vec<String>,
    token_info: Arc<Mutex<Option<TokenInfo>>>,
    http_client: Client,
    db: Option<Arc<Db>>,
}

impl AuthManager {
    /// Create a new authentication manager
    pub async fn new(
        client_id: String,
        client_secret: String,
        redirect_uri: String,
        scopes: Vec<String>,
        db: Option<Arc<Db>>,
    ) -> Self {
        let auth_manager = Self {
            client_id,
            client_secret,
            redirect_uri,
            scopes,
            token_info: Arc::new(Mutex::new(None)),
            http_client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to build auth HTTP client"),
            db,
        };

        // Try to load existing token from DB, with file migration fallback
        if let Err(e) = auth_manager.load_token().await {
            info!("No existing token found or failed to load: {}", e);
        }

        auth_manager
    }

    /// Generate OAuth authorization URL
    pub fn get_auth_url(&self) -> String {
        let state = uuid::Uuid::new_v4().to_string();
        self.get_auth_url_with_state(&state)
    }

    /// Generate OAuth authorization URL with a custom state parameter
    pub fn get_auth_url_with_state(&self, state: &str) -> String {
        let scope_string = self.scopes.join(" ");

        format!(
            "https://accounts.spotify.com/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}",
            urlencoding::encode(&self.client_id),
            urlencoding::encode(&self.redirect_uri),
            urlencoding::encode(&scope_string),
            state
        )
    }

    fn lock_token(&self) -> Result<std::sync::MutexGuard<'_, Option<TokenInfo>>> {
        self.token_info.lock().map_err(|e| anyhow::anyhow!("AuthManager mutex lock error: {}", e))
    }

    /// Send a token request to the Spotify OAuth endpoint and store the result.
    async fn perform_token_request(
        &self,
        params: &[(&str, &str)],
        default_refresh_token: Option<String>,
    ) -> Result<TokenInfo> {
        let token_url = "https://accounts.spotify.com/api/token";
        let response = self
            .http_client
            .post(token_url)
            .form(params)
            .send()
            .await
            .context("Failed to send token request")?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!("Token request failed: {}", error_text));
        }

        let token_response: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse token response")?;

        let token_info = parse_token_response(token_response, default_refresh_token)?;

        {
            let mut token_guard = self.lock_token()?;
            *token_guard = Some(token_info.clone());
        }

        self.save_token(&token_info).await.context("Failed to save token")?;

        Ok(token_info)
    }

    /// Exchange authorization code for access token
    pub async fn exchange_code_for_token(&self, code: &str) -> Result<TokenInfo> {
        info!("Exchanging authorization code for access token");

        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &self.redirect_uri),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
        ];

        let token_info = self.perform_token_request(&params, None).await?;

        info!("Successfully exchanged authorization code for access token");
        Ok(token_info)
    }

    /// Refresh access token using refresh token
    pub async fn refresh_token(&self) -> Result<TokenInfo> {
        let current_token = {
            let token_guard = self.lock_token()?;
            token_guard.clone()
        };

        let refresh_token = match current_token {
            Some(token) => token.refresh_token.context("No refresh token available")?,
            None => return Err(anyhow::anyhow!("No current token to refresh")),
        };

        info!("Refreshing access token");

        let refresh_token_for_params = refresh_token.clone();
        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh_token_for_params),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
        ];

        let token_info = self.perform_token_request(&params, Some(refresh_token)).await?;

        info!("Successfully refreshed access token");
        Ok(token_info)
    }

    /// Get current token info
    pub fn get_token_info(&self) -> Result<Option<TokenInfo>> {
        let guard = self.lock_token()?;
        Ok(guard.clone())
    }

    /// Set token information
    pub async fn set_token_info(&self, token_info: TokenInfo) -> Result<()> {
        {
            let mut token = self.lock_token()?;
            *token = Some(token_info.clone());
        }
        self.save_token(&token_info).await.context("Failed to save token")?;
        Ok(())
    }

    /// Check if we have a valid token
    pub fn has_valid_token(&self) -> Result<bool> {
        let guard = self.lock_token()?;
        Ok(guard.as_ref().map_or(false, |t| !t.is_expired()))
    }

    /// Ensure we have a valid token, refreshing if necessary
    pub async fn ensure_valid_token(&self) -> Result<TokenInfo> {
        let token_info = self.get_token_info()?;

        match token_info {
            Some(token) => {
                if token.is_expired() || token.will_expire_soon(chrono::Duration::minutes(5)) {
                    warn!("Token is expired or will expire soon, refreshing...");
                    self.refresh_token().await
                } else {
                    Ok(token)
                }
            }
            None => Err(anyhow::anyhow!(
                "No token available. Please authenticate first."
            )),
        }
    }

    /// Clear stored token
    pub async fn clear_token(&self) -> Result<()> {
        {
            let mut token = self.lock_token()?;
            *token = None;
        }

        // Remove token from DB
        if let Some(db) = &self.db {
            db.clear_token().await.context("Failed to remove token from database")?;
        }
        Ok(())
    }

    /// Save token to database (with file migration fallback)
    async fn save_token(&self, token_info: &TokenInfo) -> Result<()> {
        let json = serde_json::to_string_pretty(token_info).context("Failed to serialize token")?;

        if let Some(db) = &self.db {
            db.save_token(&json).await.context("Failed to save token to database")?;
            info!("Token saved to database");
        } else {
            warn!("No database available, token will not be persisted");
        }
        Ok(())
    }

    /// Load token from database, with migration from legacy file
    async fn load_token(&self) -> Result<()> {
        if let Some(db) = &self.db {
            if let Some(json) = db.load_token().await? {
                let token_info: TokenInfo =
                    serde_json::from_str(&json).context("Failed to deserialize token")?;
                {
                    let mut token = self.lock_token()?;
                    *token = Some(token_info);
                }
                info!("Token loaded from database");
                return Ok(());
            }

            // No token in DB — try migrating from legacy file
            let file_path = match std::env::var("HOME") {
                Ok(h) => std::path::PathBuf::from(h).join("spotify_token.json"),
                Err(_) => return Err(anyhow::anyhow!("No token in database and HOME not set, cannot migrate from legacy file")),
            };

            if tokio::fs::try_exists(&file_path).await.unwrap_or(false) {
                info!("Found legacy token file, migrating to database...");
                let json = tokio::fs::read_to_string(&file_path)
                    .await
                    .with_context(|| format!("Failed to read legacy token file: {:?}", file_path))?;
                let token_info: TokenInfo =
                    serde_json::from_str(&json).context("Failed to deserialize legacy token")?;
                {
                    let mut token = self.lock_token()?;
                    *token = Some(token_info);
                }
                db.save_token(&json).await.context("Failed to migrate token to database")?;
                if let Err(e) = tokio::fs::remove_file(&file_path).await {
                    warn!("Failed to remove legacy token file after migration: {}", e);
                }
                info!("Token migrated from file to database");
                return Ok(());
            }

            return Err(anyhow::anyhow!("No token in database or legacy file"));
        }

        Err(anyhow::anyhow!("No database available"))
    }

    /// Simplified OAuth flow - print auth URL and wait for redirect URL input
    pub async fn authenticate_with_redirect_url(&self, redirect_url: &str) -> Result<TokenInfo> {
        // Extract code from redirect URL
        let url = url::Url::parse(redirect_url).context("Invalid redirect URL")?;

        let code = url
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.to_string())
            .context("No authorization code found in redirect URL")?;

        // Exchange code for token
        let token_info = self.exchange_code_for_token(&code).await?;

        // Save token
        self.set_token_info(token_info.clone()).await?;

        Ok(token_info)
    }
}

fn parse_token_response(
    token_response: serde_json::Value,
    default_refresh_token: Option<String>,
) -> Result<TokenInfo> {
    let access_token = token_response["access_token"]
        .as_str()
        .context("Missing access token in response")?
        .to_string();

    let refresh_token = token_response["refresh_token"]
        .as_str()
        .map(|s| s.to_string())
        .or(default_refresh_token);

    let expires_in = token_response["expires_in"].as_u64();

    let token_type = token_response["token_type"]
        .as_str()
        .unwrap_or("Bearer")
        .to_string();

    let scope = token_response["scope"].as_str().map(|s| s.to_string());

    let mut token_info = TokenInfo {
        access_token,
        refresh_token,
        token_type,
        expires_in,
        scope,
        expires_at: None,
    };
    token_info.update_expiration();
    Ok(token_info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_info_creation() {
        let token = TokenInfo::new(
            "access_token".to_string(),
            Some("refresh_token".to_string()),
        );

        assert_eq!(token.access_token, "access_token");
        assert_eq!(token.refresh_token, Some("refresh_token".to_string()));
        assert_eq!(token.token_type, "Bearer");
        assert!(!token.is_expired());
    }

    #[test]
    fn test_token_expiration() {
        let mut token = TokenInfo::new(
            "access_token".to_string(),
            Some("refresh_token".to_string()),
        );

        // Set expiration to 1 hour from now
        token.expires_in = Some(3600);
        token.update_expiration();

        assert!(!token.is_expired());
        assert!(!token.will_expire_soon(chrono::Duration::minutes(30)));
        assert!(token.will_expire_soon(chrono::Duration::hours(2)));
    }

    #[tokio::test]
    async fn test_auth_manager_creation() {
        let auth_manager = AuthManager::new(
            "client_id".to_string(),
            "client_secret".to_string(),
            "http://localhost:8080/callback".to_string(),
            vec!["scope1".to_string(), "scope2".to_string()],
            None,
        )
        .await;

        let auth_url = auth_manager.get_auth_url();
        assert!(auth_url.contains("client_id"));
        assert!(auth_url.contains("scope1%20scope2"));
    }

    #[tokio::test]
    async fn test_auth_manager_token_management() {
        let auth_manager = AuthManager::new(
            "client_id".to_string(),
            "client_secret".to_string(),
            "http://localhost:8080/callback".to_string(),
            vec!["scope1".to_string()],
            None,
        )
        .await;

        assert!(!auth_manager.has_valid_token().unwrap());
        assert!(auth_manager.get_token_info().unwrap().is_none());

        let token = TokenInfo::new("test_token".to_string(), Some("refresh_token".to_string()));

        auth_manager.set_token_info(token).await.unwrap();
        assert!(auth_manager.has_valid_token().unwrap());
        assert!(auth_manager.get_token_info().unwrap().is_some());

        auth_manager.clear_token().await.unwrap();
        assert!(!auth_manager.has_valid_token().unwrap());
    }
}

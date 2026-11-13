use anyhow::{Context, Result};
use log::{info, warn};
use reqwest::Client;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::auth::{parse_token_response, TokenInfo};
use crate::web::Db;

/// Spotify's desktop ("keymaster") client id. This is the client id
/// librespot's `SessionConfig::default()` uses on Linux, and one of the
/// few clients Spotify enables for the OAuth 2.0 device authorization flow.
///
/// Connect playback (login5) only accepts credentials minted for this
/// client — third-party Web API tokens are rejected with
/// `INVALID_CREDENTIALS` (librespot-org/librespot#1737), so playback needs
/// its own token, obtained through this flow.
const PLAYBACK_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";

const DEVICE_AUTHORIZE_URL: &str = "https://accounts.spotify.com/oauth2/device/authorize";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const PLAYBACK_SCOPE: &str = "streaming";

const DEVICE_AUTH_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// The code the user types in at `https://spotify.com/pair`, plus the
/// context needed to display and complete the pairing.
#[derive(Debug, Clone, Serialize)]
pub struct PairingInfo {
    pub user_code: String,
    pub verification_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_uri_complete: Option<String>,
    /// Seconds until the user code expires.
    pub expires_in_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PairingPhase {
    /// Not paired, no pairing attempt in progress.
    #[default]
    Idle,
    /// Waiting for the user to approve the code at spotify.com/pair.
    Pending,
    /// Pairing completed and credentials were stored.
    Granted,
    /// Pairing failed, expired, or was denied.
    Failed,
}

/// Snapshot of the pairing state for the web API.
#[derive(Debug, Clone, Serialize)]
pub struct PlaybackStatus {
    /// True when a refresh token is stored, i.e. playback can initialize.
    pub paired: bool,
    pub phase: PairingPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Internal, mutable pairing state. `device_code` identifies which polling
/// task owns the flow, so a stale poller can never clobber a newer pairing.
#[derive(Default)]
struct PairingState {
    phase: PairingPhase,
    info: Option<PairingInfo>,
    device_code: Option<String>,
    error: Option<String>,
}

impl PairingState {
    fn snapshot(&self, paired: bool) -> PlaybackStatus {
        PlaybackStatus {
            paired,
            phase: self.phase,
            user_code: self.info.as_ref().map(|i| i.user_code.clone()),
            verification_uri: self.info.as_ref().map(|i| i.verification_uri.clone()),
            error: self.error.clone(),
        }
    }
}

/// Spotify authentication for local playback, separate from the Web API
/// auth (`AuthManager`).
///
/// Spotify's device authorization grant (RFC 8628) needs no redirect and no
/// client secret: the user opens `https://spotify.com/pair` on any device
/// and enters a short code. The resulting refresh token is long-lived and
/// refreshed here without user interaction.
pub struct PlaybackAuth {
    token_info: Arc<Mutex<Option<TokenInfo>>>,
    pairing: Arc<Mutex<PairingState>>,
    http_client: Client,
    db: Option<Arc<Db>>,
}

impl PlaybackAuth {
    pub async fn new(db: Option<Arc<Db>>) -> Self {
        let this = Self {
            token_info: Arc::new(Mutex::new(None)),
            pairing: Arc::new(Mutex::new(PairingState::default())),
            http_client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to build playback auth HTTP client"),
            db,
        };

        if let Some(db) = &this.db {
            match db.load_playback_token().await {
                Ok(Some(json)) => match serde_json::from_str::<TokenInfo>(&json) {
                    Ok(token_info) => {
                        info!("Playback token loaded from database");
                        if let Ok(mut guard) = this.token_info.lock() {
                            *guard = Some(token_info);
                        }
                    }
                    Err(e) => warn!("Failed to parse stored playback token: {}", e),
                },
                Ok(None) => {}
                Err(e) => warn!("Failed to load playback token from database: {}", e),
            }
        }

        this
    }

    fn lock_token(&self) -> Result<std::sync::MutexGuard<'_, Option<TokenInfo>>> {
        self.token_info
            .lock()
            .map_err(|e| anyhow::anyhow!("PlaybackAuth token mutex lock error: {}", e))
    }

    fn lock_pairing(&self) -> Result<std::sync::MutexGuard<'_, PairingState>> {
        self.pairing
            .lock()
            .map_err(|e| anyhow::anyhow!("PlaybackAuth pairing mutex lock error: {}", e))
    }

    /// Fast check for the main loop: true when playback credentials exist,
    /// without any network I/O.
    pub fn has_refresh_token(&self) -> bool {
        self.token_info
            .lock()
            .map(|guard| {
                guard
                    .as_ref()
                    .map(|t| t.refresh_token.is_some())
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    /// Current pairing/token status for the web UI.
    pub fn pairing_status(&self) -> PlaybackStatus {
        let paired = self.has_refresh_token();
        match self.lock_pairing() {
            Ok(pairing) => pairing.snapshot(paired),
            Err(_) => PlaybackStatus {
                paired,
                phase: PairingPhase::Idle,
                user_code: None,
                verification_uri: None,
                error: None,
            },
        }
    }

    /// Start a new device pairing flow. Idempotent while a pairing is
    /// already pending: the in-progress code is returned instead.
    pub async fn start_pairing(&self) -> Result<PlaybackStatus> {
        if let Ok(pairing) = self.lock_pairing() {
            if pairing.phase == PairingPhase::Pending {
                return Ok(pairing.snapshot(self.has_refresh_token()));
            }
        }

        let response = self
            .http_client
            .post(DEVICE_AUTHORIZE_URL)
            .form(&[("client_id", PLAYBACK_CLIENT_ID), ("scope", PLAYBACK_SCOPE)])
            .send()
            .await
            .context("Failed to reach the Spotify device authorization endpoint")?;

        let status = response.status();
        let body: serde_json::Value = response.json().await.with_context(|| {
            format!(
                "Failed to parse device authorization response (HTTP {})",
                status
            )
        })?;

        if !status.is_success() {
            let error = body["error"].as_str().unwrap_or("unknown_error");
            anyhow::bail!(
                "Spotify rejected the pairing request: {} (HTTP {})",
                error,
                status
            );
        }

        let device_code = body["device_code"]
            .as_str()
            .context("Missing device_code in device authorization response")?
            .to_string();
        let user_code = body["user_code"]
            .as_str()
            .context("Missing user_code in device authorization response")?
            .to_string();
        let verification_uri = body["verification_uri"]
            .as_str()
            .unwrap_or("https://spotify.com/pair")
            .to_string();
        let verification_uri_complete =
            body["verification_uri_complete"].as_str().map(String::from);
        let expires_in = body["expires_in"].as_u64().unwrap_or(3599);
        let interval = body["interval"].as_u64().unwrap_or(5).max(1);

        {
            let mut pairing = self.lock_pairing()?;
            pairing.phase = PairingPhase::Pending;
            pairing.device_code = Some(device_code.clone());
            pairing.info = Some(PairingInfo {
                user_code: user_code.clone(),
                verification_uri: verification_uri.clone(),
                verification_uri_complete: verification_uri_complete.clone(),
                expires_in_secs: expires_in,
            });
            pairing.error = None;
        }

        info!(
            "Playback pairing started — enter code {} at {}",
            user_code, verification_uri
        );

        let token_info = self.token_info.clone();
        let pairing = self.pairing.clone();
        let http_client = self.http_client.clone();
        let db = self.db.clone();
        tokio::spawn(async move {
            Self::poll_until_granted(
                http_client,
                device_code,
                interval,
                expires_in,
                token_info,
                pairing,
                db,
            )
            .await;
        });

        Ok(self.pairing_status())
    }

    /// Poll the token endpoint until the user approves the code, it
    /// expires, or the flow fails.
    async fn poll_until_granted(
        http_client: Client,
        device_code: String,
        mut interval: u64,
        expires_in: u64,
        token_info: Arc<Mutex<Option<TokenInfo>>>,
        pairing: Arc<Mutex<PairingState>>,
        db: Option<Arc<Db>>,
    ) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(expires_in);

        loop {
            tokio::time::sleep(Duration::from_secs(interval)).await;

            if tokio::time::Instant::now() >= deadline {
                Self::fail_pairing(&pairing, &device_code, "Pairing code expired. Start again.")
                    .await;
                return;
            }

            let params = [
                ("grant_type", DEVICE_AUTH_GRANT_TYPE),
                ("device_code", device_code.as_str()),
                ("client_id", PLAYBACK_CLIENT_ID),
            ];
            let response = match http_client.post(TOKEN_URL).form(&params).send().await {
                Ok(response) => response,
                Err(e) => {
                    warn!("Playback pairing poll request failed, retrying: {}", e);
                    continue;
                }
            };

            let status = response.status();
            let body: serde_json::Value = match response.json().await {
                Ok(body) => body,
                Err(e) => {
                    warn!("Failed to parse pairing poll response, retrying: {}", e);
                    continue;
                }
            };

            if status.is_success() {
                match parse_token_response(body, None) {
                    Ok(info) => {
                        if let Err(e) =
                            Self::store_granted(&token_info, &pairing, &device_code, &db, info)
                                .await
                        {
                            warn!("Failed to store granted playback token: {}", e);
                        }
                    }
                    Err(e) => {
                        Self::fail_pairing(
                            &pairing,
                            &device_code,
                            &format!("Invalid token response: {}", e),
                        )
                        .await;
                    }
                }
                return;
            }

            match body["error"].as_str().unwrap_or("") {
                "authorization_pending" => continue,
                "slow_down" => {
                    interval += 5;
                    continue;
                }
                "access_denied" => {
                    Self::fail_pairing(&pairing, &device_code, "Pairing was denied.").await;
                    return;
                }
                "expired_token" => {
                    Self::fail_pairing(
                        &pairing,
                        &device_code,
                        "Pairing code expired. Start again.",
                    )
                    .await;
                    return;
                }
                other => {
                    Self::fail_pairing(
                        &pairing,
                        &device_code,
                        &format!("Pairing failed: {} (HTTP {})", other, status),
                    )
                    .await;
                    return;
                }
            }
        }
    }

    /// Store a granted token, but only if this poller still owns the flow.
    async fn store_granted(
        token_info: &Mutex<Option<TokenInfo>>,
        pairing: &Mutex<PairingState>,
        device_code: &str,
        db: &Option<Arc<Db>>,
        info: TokenInfo,
    ) -> Result<()> {
        {
            let mut guard = pairing
                .lock()
                .map_err(|e| anyhow::anyhow!("pairing mutex lock error: {}", e))?;
            if guard.device_code.as_deref() != Some(device_code) {
                // A newer pairing (or an unpair) superseded this flow.
                return Ok(());
            }
            guard.phase = PairingPhase::Granted;
            guard.error = None;
        }

        {
            let mut guard = token_info
                .lock()
                .map_err(|e| anyhow::anyhow!("token mutex lock error: {}", e))?;
            *guard = Some(info.clone());
        }

        if let Some(db) = db {
            let json =
                serde_json::to_string(&info).context("Failed to serialize playback token")?;
            db.save_playback_token(&json)
                .await
                .context("Failed to save playback token to database")?;
            info!("Playback token saved to database");
        } else {
            warn!("No database available, playback token will not be persisted");
        }

        info!("Playback pairing granted — Spotify Connect can initialize");
        Ok(())
    }

    /// Mark the flow failed, but only if this poller still owns it.
    async fn fail_pairing(pairing: &Mutex<PairingState>, device_code: &str, message: &str) {
        let mut guard = match pairing.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if guard.device_code.as_deref() != Some(device_code) {
            return;
        }
        warn!("Playback pairing failed: {}", message);
        guard.phase = PairingPhase::Failed;
        guard.error = Some(message.to_string());
    }

    /// Return a valid playback token, refreshing it when needed.
    pub async fn ensure_valid_token(&self) -> Result<TokenInfo> {
        let existing = self.lock_token()?.clone();

        if let Some(info) = &existing {
            if !info.is_expired() && !info.will_expire_soon(chrono::Duration::minutes(5)) {
                return Ok(info.clone());
            }
        }

        let refresh_token = existing
            .and_then(|info| info.refresh_token)
            .context("Playback not paired. Open the Turny web UI and pair with Spotify.")?;

        info!("Refreshing playback token");
        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
            ("client_id", PLAYBACK_CLIENT_ID),
        ];
        let response = self
            .http_client
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await
            .context("Failed to send playback token refresh request")?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!(
                "Playback token refresh failed: {}",
                error_text
            ));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse playback token refresh response")?;

        // The refresh response may omit a new refresh token; keep the old one.
        let token_info = parse_token_response(body, Some(refresh_token))
            .context("Failed to parse playback token refresh response")?;

        {
            let mut guard = self.lock_token()?;
            *guard = Some(token_info.clone());
        }
        self.save_token(&token_info).await?;

        info!("Successfully refreshed playback token");
        Ok(token_info)
    }

    /// Drop stored playback credentials (web UI "Unpair").
    pub async fn clear(&self) -> Result<()> {
        {
            let mut guard = self.lock_token()?;
            *guard = None;
        }
        {
            let mut pairing = self.lock_pairing()?;
            *pairing = PairingState::default();
        }
        if let Some(db) = &self.db {
            db.clear_playback_token()
                .await
                .context("Failed to clear playback token from database")?;
        }
        info!("Playback pairing cleared");
        Ok(())
    }

    async fn save_token(&self, token_info: &TokenInfo) -> Result<()> {
        if let Some(db) = &self.db {
            let json =
                serde_json::to_string(token_info).context("Failed to serialize playback token")?;
            db.save_playback_token(&json)
                .await
                .context("Failed to save playback token to database")?;
            info!("Playback token saved to database");
        } else {
            warn!("No database available, playback token will not be persisted");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_flow_token_response() {
        let body = serde_json::json!({
            "access_token": "BQabc",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "AQdef",
            "scope": "streaming"
        });

        let info = parse_token_response(body, None).unwrap();
        assert_eq!(info.access_token, "BQabc");
        assert_eq!(info.refresh_token.as_deref(), Some("AQdef"));
        assert!(info.expires_at.is_some());
    }

    #[test]
    fn refresh_response_keeps_old_refresh_token() {
        let body = serde_json::json!({
            "access_token": "BQnew",
            "token_type": "Bearer",
            "expires_in": 3600
        });

        let info = parse_token_response(body, Some("AQold".to_string())).unwrap();
        assert_eq!(info.access_token, "BQnew");
        assert_eq!(info.refresh_token.as_deref(), Some("AQold"));
    }

    #[test]
    fn pairing_status_serializes_phase_lowercase() {
        let status = PlaybackStatus {
            paired: false,
            phase: PairingPhase::Pending,
            user_code: Some("ABCDEF".to_string()),
            verification_uri: Some("https://spotify.com/pair".to_string()),
            error: None,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["phase"], "pending");
        assert_eq!(json["user_code"], "ABCDEF");
        assert!(json.get("error").is_none());
    }

    #[tokio::test]
    async fn test_playback_auth_starts_unpaired() {
        let auth = PlaybackAuth::new(None).await;
        assert!(!auth.has_refresh_token());
        let status = auth.pairing_status();
        assert!(!status.paired);
        assert_eq!(status.phase, PairingPhase::Idle);
        assert!(ensure_valid_token_error_mentions_pairing(&auth).await);
    }

    async fn ensure_valid_token_error_mentions_pairing(auth: &PlaybackAuth) -> bool {
        matches!(
            auth.ensure_valid_token().await,
            Err(e) if e.to_string().contains("Playback not paired")
        )
    }
}

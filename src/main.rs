use anyhow::{Context, Result};
use log::{error, info, warn};
use std::env;
use std::sync::Arc;
use tokio::signal;

mod app;
mod audio;
mod auth;
mod cli;
mod config;
mod hardware;
mod spotify_connect;
mod state;
mod web;

use app::TurnyApp;
use auth::AuthManager;
use config::TurnyConfig;
use state::StateManager;
use tokio::sync::{broadcast, mpsc};

const EVENT_CHANNEL_CAPACITY: usize = 100;
const CMD_CHANNEL_CAPACITY: usize = 100;

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls CryptoProvider");

    // Initialize logging
    env_logger::init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        if args[1] == "web" {
            return run_web_mode().await;
        }
        return cli::run_cli_mode(&args[1..]).await;
    }

    // Default hardware mode
    run_hardware_mode().await
}

/// Run the main hardware mode (default behavior)
async fn run_hardware_mode() -> Result<()> {
    info!("Starting Turny Music Player...");

    // Load configuration
    let config = cli::load_config().await?;

    // Create DB and migrate
    let db = Arc::new(web::Db::open("turny.db")?);
    db.migrate_from_config(&config.playlists).await?;

    // Create channels
    let (event_tx, _) = broadcast::channel::<web::WebEvent>(EVENT_CHANNEL_CAPACITY);
    let (player_cmd_tx, player_cmd_rx) = mpsc::channel::<web::PlayerCommand>(CMD_CHANNEL_CAPACITY);

    // Extract web config before config is moved into TurnyApp
    let web_addr = format!("{}:{}", config.web.host, config.web.port);

    // Create and initialize the application
    let mut app = TurnyApp::new(
        config.clone(),
        Some(db.clone()),
        Some(event_tx.clone()),
        Some(player_cmd_rx),
    )
    .await
    .context("Failed to create Turny application")?;

    // Check authentication - try refreshing existing token first
    if !app.is_authenticated()? {
        if let Err(e) = app.refresh_token().await {
            warn!(
                "No valid token: {}. Authenticate via web UI at http://localhost:8080",
                e
            );
        }
    }

    // Initialize Spotify services
    if app.is_authenticated()? {
        if let Err(e) = app.initialize_spotify().await {
            error!("Failed to initialize Spotify services: {}", e);
        }
    }

    // Get shared state for web server
    let auth_manager = app.get_auth_manager();
    let state_manager = app.get_state_manager();

    let web_state = build_app_state(
        db,
        auth_manager,
        &config,
        event_tx,
        player_cmd_tx,
        state_manager,
    );

    let shutdown_for_web = setup_shutdown_signal();
    let web_handle = tokio::spawn(async move {
        if let Err(e) = web::start_web_server(web_state, &web_addr, shutdown_for_web).await {
            error!("Web server error: {}", e);
        }
    });

    // Run the application — the web server handles the shutdown signal
    // and will gracefully close WebSocket connections.
    tokio::select! {
        result = app.run() => {
            match result {
                Ok(_) => info!("Application completed successfully"),
                Err(e) => error!("Application error: {}", e),
            }
        }
        _ = web_handle => {
            info!("Web server stopped");
        }
    }

    // Graceful shutdown
    info!("Shutting down...");
    app.shutdown().await?;

    info!("Turny Music Player stopped");
    Ok(())
}

/// Run web server only (no hardware required)
async fn run_web_mode() -> Result<()> {
    info!("Starting Turny web server (no hardware mode)...");

    let config = cli::load_config().await?;

    let db = Arc::new(web::Db::open("turny.db")?);
    db.migrate_from_config(&config.playlists).await?;

    let auth_manager = Arc::new(AuthManager::new(
        config.spotify.client_id.clone(),
        config.spotify.client_secret.clone(),
        config.spotify.redirect_uri.clone(),
        config.advanced.scopes.clone(),
        Some(db.clone()),
    ).await);

    let state_manager = StateManager::new();
    let (event_tx, _) = broadcast::channel::<web::WebEvent>(EVENT_CHANNEL_CAPACITY);
    let (player_cmd_tx, _player_cmd_rx) = mpsc::channel::<web::PlayerCommand>(CMD_CHANNEL_CAPACITY);

    let web_state = build_app_state(
        db,
        auth_manager,
        &config,
        event_tx,
        player_cmd_tx,
        state_manager,
    );

    let web_addr = format!("{}:{}", config.web.host, config.web.port);
    let shutdown_signal = setup_shutdown_signal();

    if let Err(e) = web::start_web_server(web_state, &web_addr, shutdown_signal).await {
        error!("Web server error: {}", e);
    }

    info!("Web server stopped");
    Ok(())
}

/// Build AppState for the web server, extracting config-derived fields
fn build_app_state(
    db: Arc<web::Db>,
    auth_manager: Arc<AuthManager>,
    config: &TurnyConfig,
    event_tx: broadcast::Sender<web::WebEvent>,
    player_cmd_tx: mpsc::Sender<web::PlayerCommand>,
    state_manager: StateManager,
) -> web::AppState {
    web::AppState {
        db,
        auth_manager,
        spotify_api: web::SpotifyApi::new(),
        event_tx,
        player_cmd_tx,
        state_manager,
        pending_auth_states: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
        web_origin: config.web.external_url.clone(),
        default_volume: config.settings.default_volume,
    }
}

/// Set up graceful shutdown signal handling
async fn setup_shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Ctrl+C received");
        },
        _ = terminate => {
            info!("SIGTERM received");
        },
    }
}

use anyhow::{Context, Result};
use log::{error, info, warn};
use std::env;
use std::io::{self, Write};
use tokio::signal;

mod app;
mod audio;
mod auth;
mod config;
mod hardware;
mod spotify_connect;
mod state;

use app::TurnyApp;
use config::TurnyConfig;

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider().install_default().expect("Failed to install rustls CryptoProvider");

    // Initialize logging
    env_logger::init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        // CLI mode
        return run_cli_mode(&args[1..]).await;
    }

    // Default hardware mode
    run_hardware_mode().await
}

/// Run the main hardware mode (default behavior)
async fn run_hardware_mode() -> Result<()> {
    info!("Starting Turny Music Player...");

    // Load configuration
    let config = load_config().await?;

    // Create and initialize the application
    let mut app = TurnyApp::new(config)
        .await
        .context("Failed to create Turny application")?;

    // Check authentication - try refreshing existing token first
    if !app.is_authenticated().await {
        match app.refresh_token().await {
            Ok(_) => {
                info!("Token refreshed successfully");
            }
            Err(e) => {
                warn!("No valid token and refresh failed: {}", e);
                println!("No valid authentication found!");
                println!("Please visit this URL to authenticate:");
                println!("{}", app.get_oauth_url());
                println!();
                print!("After authentication, paste the redirect URL here: ");
                io::stdout().flush().unwrap();

                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .context("Failed to read redirect URL")?;

                let redirect_url = input.trim();

                // Authenticate with the redirect URL
                match app.authenticate_with_redirect_url(redirect_url).await {
                    Ok(_) => {
                        info!("Authentication successful!");
                    }
                    Err(e) => {
                        error!("Authentication failed: {}", e);
                        return Err(e);
                    }
                }
            }
        }
    }

    // Initialize Spotify services
    if let Err(e) = app.initialize_spotify().await {
        error!("Failed to initialize Spotify services: {}", e);
        warn!("You may need to re-authenticate. Visit:");
        warn!("{}", app.get_oauth_url());
        return Err(e);
    }

    // Set up graceful shutdown
    let shutdown_signal = setup_shutdown_signal();

    // Run the application
    tokio::select! {
        result = app.run() => {
            match result {
                Ok(_) => info!("Application completed successfully"),
                Err(e) => error!("Application error: {}", e),
            }
        }
        _ = shutdown_signal => {
            info!("Shutdown signal received");
        }
    }

    // Graceful shutdown
    info!("Shutting down...");
    app.shutdown().await?;

    info!("Turny Music Player stopped");
    Ok(())
}

/// Handle authentication commands
async fn handle_auth_command(args: &[String]) -> Result<()> {
    let config = load_config().await?;

    // Create a minimal app for auth operations (may fail due to hardware)
    let app_result = TurnyApp::new(config.clone()).await;

    match args.get(0).map(|s| s.as_str()) {
        Some("login") => {
            // Use auth manager directly if app creation fails
            if let Ok(app) = app_result {
                if app.is_authenticated().await {
                    println!("Already authenticated!");
                    return Ok(());
                }

                println!("Please visit this URL to authenticate:");
                println!("{}", app.get_oauth_url());
                println!();
                print!("After authentication, paste the redirect URL here: ");
                io::stdout().flush().unwrap();

                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .context("Failed to read redirect URL")?;

                let redirect_url = input.trim();

                match app.authenticate_with_redirect_url(redirect_url).await {
                    Ok(_) => println!("Authentication successful!"),
                    Err(e) => println!("Authentication failed: {}", e),
                }
            } else {
                println!("Cannot initialize hardware for authentication.");
                println!("Please run on a Raspberry Pi or use the main application.");
            }
        }
        Some("logout") => {
            if let Ok(app) = app_result {
                println!("Logging out...");
                match app.clear_authentication().await {
                    Ok(_) => println!("Successfully logged out!"),
                    Err(e) => println!("Error during logout: {}", e),
                }
            } else {
                println!("Cannot access authentication without hardware.");
            }
        }
        Some("status") => {
            if let Ok(app) = app_result {
                let authenticated = app.is_authenticated().await;
                println!(
                    "Authentication status: {}",
                    if authenticated {
                        "Authenticated"
                    } else {
                        "Not authenticated"
                    }
                );
                if !authenticated {
                    println!("OAuth URL: {}", app.get_oauth_url());
                }
            } else {
                println!("Cannot check authentication status without hardware.");
            }
        }
        _ => {
            println!("Auth commands:");
            println!("  login   - Authenticate with Spotify");
            println!("  logout  - Clear authentication");
            println!("  status  - Show authentication status");
        }
    }

    Ok(())
}

/// Handle configuration commands
async fn handle_config_command(args: &[String]) -> Result<()> {
    let mut config = load_config().await?;

    match args.get(0).map(|s| s.as_str()) {
        Some("show") => {
            println!("Current configuration:");
            println!("Client ID: {}", config.spotify.client_id);
            println!("Redirect URI: {}", config.spotify.redirect_uri);
            println!("Card mappings: {} entries", config.playlists.len());
            for (card_id, playlist_uri) in &config.playlists {
                println!("  {}: {}", card_id, playlist_uri);
            }
        }
        Some("add-card") => {
            if args.len() < 3 {
                println!("Usage: turny config add-card <card_id> <playlist_uri>");
                return Ok(());
            }

            let card_id = args[1].clone();
            let playlist_uri = args[2].clone();

            config.add_card_mapping(card_id.clone(), playlist_uri.clone());
            config.save_to_file("config.toml")?;

            println!("Added card mapping: {} -> {}", card_id, playlist_uri);
        }
        Some("remove-card") => {
            if args.len() < 2 {
                println!("Usage: turny config remove-card <card_id>");
                return Ok(());
            }

            let card_id = &args[1];
            if let Some(playlist_uri) = config.remove_card_mapping(card_id) {
                config.save_to_file("config.toml")?;
                println!("Removed card mapping: {} -> {}", card_id, playlist_uri);
            } else {
                println!("Card not found: {}", card_id);
            }
        }
        _ => {
            println!("Config commands:");
            println!("  show                      - Show current configuration");
            println!("  add-card <id> <playlist>  - Add card mapping");
            println!("  remove-card <id>          - Remove card mapping");
        }
    }

    Ok(())
}

/// Handle status commands
async fn handle_status_command(_args: &[String]) -> Result<()> {
    let config = load_config().await?;

    match TurnyApp::new(config).await {
        Ok(app) => {
            let status = app.get_status().await?;
            println!("{}", status);
        }
        Err(e) => {
            println!("Cannot get full status without hardware: {}", e);
            println!(
                "Configuration file: {}",
                if std::path::Path::new("config.toml").exists() {
                    "Found"
                } else {
                    "Not found"
                }
            );
            println!(
                "Token file: {}",
                if std::path::Path::new("spotify_token.json").exists() {
                    "Found"
                } else {
                    "Not found"
                }
            );
        }
    }

    Ok(())
}

/// Handle Spotify commands
async fn handle_spotify_command(args: &[String]) -> Result<()> {
    let config = load_config().await?;

    match TurnyApp::new(config).await {
        Ok(mut app) => {
            if !app.is_authenticated().await {
                println!("Not authenticated. Run 'turny auth login' first.");
                return Ok(());
            }

            if let Err(e) = app.initialize_spotify().await {
                println!("Failed to initialize Spotify: {}", e);
                return Ok(());
            }

            match args.get(0).map(|s| s.as_str()) {
                Some("status") => {
                    println!(
                        "Spotify Connect status: {}",
                        if app.is_spotify_connect_initialized() {
                            "Connected"
                        } else {
                            "Disconnected"
                        }
                    );
                }
                _ => {
                    println!("Spotify commands:");
                    println!("  status  - Show Spotify Connect status");
                    println!();
                    println!("Note: Advanced Spotify controls are handled by the main application");
                    println!("when running in hardware mode with RFID cards and buttons.");
                }
            }
        }
        Err(e) => {
            println!("Cannot access Spotify without hardware: {}", e);
        }
    }

    Ok(())
}

/// Handle card management commands
async fn handle_cards_command(args: &[String]) -> Result<()> {
    let mut config = load_config().await?;

    match args.get(0).map(|s| s.as_str()) {
        Some("list") => {
            println!("Card mappings:");
            if config.playlists.is_empty() {
                println!("  No cards configured");
            } else {
                for (card_id, playlist_uri) in &config.playlists {
                    println!("  {}: {}", card_id, playlist_uri);
                }
            }
        }
        Some("add") => {
            if args.len() < 3 {
                println!("Usage: turny cards add <card_id> <playlist_uri>");
                return Ok(());
            }

            let card_id = args[1].clone();
            let playlist_uri = args[2].clone();

            config.add_card_mapping(card_id.clone(), playlist_uri.clone());
            config.save_to_file("config.toml")?;

            println!("Added card: {} -> {}", card_id, playlist_uri);
        }
        Some("remove") => {
            if args.len() < 2 {
                println!("Usage: turny cards remove <card_id>");
                return Ok(());
            }

            let card_id = &args[1];
            if let Some(playlist_uri) = config.remove_card_mapping(card_id) {
                config.save_to_file("config.toml")?;
                println!("Removed card: {} -> {}", card_id, playlist_uri);
            } else {
                println!("Card not found: {}", card_id);
            }
        }
        _ => {
            println!("Card commands:");
            println!("  list           - List all card mappings");
            println!("  add <id> <uri> - Add card mapping");
            println!("  remove <id>    - Remove card mapping");
        }
    }

    Ok(())
}

/// Run CLI mode with various commands
async fn run_cli_mode(args: &[String]) -> Result<()> {
    if args.is_empty() {
        print_help();
        return Ok(());
    }

    match args[0].as_str() {
        "auth" => handle_auth_command(&args[1..]).await,
        "config" => handle_config_command(&args[1..]).await,
        "status" => handle_status_command(&args[1..]).await,
        "spotify" => handle_spotify_command(&args[1..]).await,
        "cards" => handle_cards_command(&args[1..]).await,
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        _ => {
            println!("Unknown command: {}", args[0]);
            print_help();
            Ok(())
        }
    }
}

/// Print help information
fn print_help() {
    println!("Turny Music Player - CLI Mode");
    println!("Usage: turny [COMMAND] [OPTIONS]");
    println!();
    println!("Commands:");
    println!("  auth      Authentication management");
    println!("  config    Configuration management");
    println!("  status    Show application status");
    println!("  spotify   Spotify operations");
    println!("  cards     RFID card management");
    println!("  help      Show this help message");
    println!();
    println!("Run without arguments to start hardware mode.");
    println!();
    println!("Examples:");
    println!("  turny auth login                 # Authenticate with Spotify");
    println!("  turny auth logout                # Clear authentication");
    println!("  turny status                     # Show current status");
    println!("  turny config show                # Show current configuration");
    println!("  turny spotify devices            # List available devices");
    println!("  turny cards add <id> <playlist>  # Add card mapping");
}

/// Load configuration from file or environment variables
async fn load_config() -> Result<TurnyConfig> {
    // Try to load from config file first
    if let Ok(config) = TurnyConfig::from_file("config.toml") {
        info!("Loaded configuration from config.toml");
        return Ok(config);
    }

    // Fall back to environment variables or defaults
    let config = TurnyConfig::from_env_or_default();
    info!("Using configuration from environment variables or defaults");

    // Validate configuration
    config.validate().context("Invalid configuration")?;

    Ok(config)
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

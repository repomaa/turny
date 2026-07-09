use anyhow::{Context, Result};
use log::info;
use std::io::{self, Write};
use std::sync::Arc;

use crate::auth::AuthManager;
use crate::config::TurnyConfig;
use crate::web::Db;

/// Load configuration from file or environment variables
pub async fn load_config() -> Result<TurnyConfig> {
    if let Ok(config) = TurnyConfig::from_file("config.toml").await {
        info!("Loaded configuration from config.toml");
        config.validate().context("Invalid configuration in config.toml")?;
        return Ok(config);
    }

    let config = TurnyConfig::from_env_or_default();
    info!("Using configuration from environment variables");
    config.validate().context("Invalid configuration")?;
    Ok(config)
}

/// Handle authentication commands
pub async fn handle_auth_command(args: &[String]) -> Result<()> {
    let config = load_config().await?;

    let db = Db::open("turny.db").ok().map(Arc::new);
    let auth_manager = AuthManager::new(
        config.spotify.client_id.clone(),
        config.spotify.client_secret.clone(),
        config.spotify.redirect_uri.clone(),
        config.advanced.scopes.clone(),
        db,
    )
    .await;

    match args.first().map(String::as_str) {
        Some("login") => {
            if auth_manager.has_valid_token()? {
                println!("Already authenticated!");
                return Ok(());
            }

            println!("Please visit this URL to authenticate:");
            println!("{}", auth_manager.get_auth_url());
            println!();
            print!("After authentication, paste the redirect URL here: ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .context("Failed to read redirect URL")?;

            let redirect_url = input.trim();

            match auth_manager.authenticate_with_redirect_url(redirect_url).await {
                Ok(_) => println!("Authentication successful!"),
                Err(e) => println!("Authentication failed: {}", e),
            }
        }
        Some("logout") => {
            println!("Logging out...");
            match auth_manager.clear_token().await {
                Ok(_) => println!("Successfully logged out!"),
                Err(e) => println!("Error during logout: {}", e),
            }
        }
        Some("status") => {
            let authenticated = auth_manager.has_valid_token()?;
            println!(
                "Authentication status: {}",
                if authenticated {
                    "Authenticated"
                } else {
                    "Not authenticated"
                }
            );
            if !authenticated {
                println!("OAuth URL: {}", auth_manager.get_auth_url());
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
pub async fn handle_config_command(args: &[String]) -> Result<()> {
    let mut config = load_config().await?;

    match args.first().map(String::as_str) {
        Some("show") | Some("list") => {
            println!("Current configuration:");
            println!("Client ID: {}", config.spotify.client_id);
            println!("Redirect URI: {}", config.spotify.redirect_uri);

            match Db::open("turny.db") {
                Ok(db) => {
                    match db.get_all_mappings().await {
                        Ok(mappings) => {
                            println!("Card mappings: {} entries", mappings.len());
                            for m in &mappings {
                                println!(
                                    "  {}: {} ({})",
                                    m.card_id,
                                    m.playlist_uri,
                                    m.playlist_name.as_deref().unwrap_or("unnamed")
                                );
                            }
                        }
                        Err(e) => println!("Card mappings: failed to read DB: {}", e),
                    }
                }
                Err(_) => {
                    println!(
                        "Card mappings: {} entries (config only)",
                        config.playlists.len()
                    );
                    for (card_id, playlist_uri) in &config.playlists {
                        println!("  {}: {}", card_id, playlist_uri);
                    }
                }
            }
        }
        Some("add-card") | Some("add") => {
            if args.len() < 3 {
                println!("Usage: turny config add-card <card_id> <playlist_uri>");
                return Ok(());
            }

            let card_id = args[1].clone();
            let playlist_uri = args[2].clone();

            let db = Db::open("turny.db")?;
            db.add_card_mapping(&card_id, &playlist_uri, None).await?;

            config.add_card_mapping(card_id.clone(), playlist_uri.clone());
            config.save_to_file("config.toml").await?;

            println!("Added card mapping: {} -> {}", card_id, playlist_uri);
        }
        Some("remove-card") | Some("remove") => {
            if args.len() < 2 {
                println!("Usage: turny config remove-card <card_id>");
                return Ok(());
            }

            let card_id = &args[1];

            let db = Db::open("turny.db")?;
            db.remove_card_mapping(card_id).await?;

            if let Some(playlist_uri) = config.remove_card_mapping(card_id) {
                config.save_to_file("config.toml").await?;
                println!("Removed card mapping: {} -> {}", card_id, playlist_uri);
            } else {
                println!("Card not found in config: {}", card_id);
            }
        }
        _ => {
            println!("Config commands:");
            println!("  show | list                - Show current configuration");
            println!("  add-card <id> <playlist>   - Add card mapping");
            println!("  remove-card <id>           - Remove card mapping");
        }
    }

    Ok(())
}

/// Handle cards commands
pub async fn handle_cards_command(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("list") => {
            let db = Db::open("turny.db")?;
            match db.get_all_mappings().await {
                Ok(mappings) => {
                    println!("Card mappings: {} entries", mappings.len());
                    for m in &mappings {
                        println!(
                            "  {}: {} ({})",
                            m.card_id,
                            m.playlist_uri,
                            m.playlist_name.as_deref().unwrap_or("unnamed")
                        );
                    }
                }
                Err(e) => println!("Failed to read DB: {}", e),
            }
        }
        Some("add") => {
            if args.len() < 3 {
                println!("Usage: turny cards add <card_id> <playlist_uri>");
                return Ok(());
            }

            let card_id = args[1].clone();
            let playlist_uri = args[2].clone();

            let db = Db::open("turny.db")?;
            db.add_card_mapping(&card_id, &playlist_uri, None).await?;

            let mut config = load_config().await?;
            config.add_card_mapping(card_id.clone(), playlist_uri.clone());
            config.save_to_file("config.toml").await?;

            println!("Added card mapping: {} -> {}", card_id, playlist_uri);
        }
        Some("remove") => {
            if args.len() < 2 {
                println!("Usage: turny cards remove <card_id>");
                return Ok(());
            }

            let card_id = &args[1];

            let db = Db::open("turny.db")?;
            db.remove_card_mapping(card_id).await?;

            let mut config = load_config().await?;
            if let Some(playlist_uri) = config.remove_card_mapping(card_id) {
                config.save_to_file("config.toml").await?;
                println!("Removed card mapping: {} -> {}", card_id, playlist_uri);
            } else {
                println!("Removed from DB (not in config): {}", card_id);
            }
        }
        _ => {
            println!("Cards commands:");
            println!("  list                  - List all card mappings");
            println!("  add <id> <playlist>   - Add card mapping");
            println!("  remove <id>           - Remove card mapping");
        }
    }

    Ok(())
}

/// Handle status commands
pub async fn handle_status_command(_args: &[String]) -> Result<()> {
    let config = load_config().await?;

    match crate::app::TurnyApp::new(config, None, None, None).await {
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
                "Token in database: {}",
                if std::path::Path::new("turny.db").exists() {
                    "Database found"
                } else {
                    "Database not found"
                }
            );
        }
    }

    Ok(())
}

/// Handle Spotify commands
pub async fn handle_spotify_command(args: &[String]) -> Result<()> {
    let config = load_config().await?;

    match crate::app::TurnyApp::new(config, None, None, None).await {
        Ok(mut app) => {
            if !app.is_authenticated()? {
                println!("Not authenticated. Run 'turny auth login' first.");
                return Ok(());
            }

            if let Err(e) = app.initialize_spotify().await {
                println!("Failed to initialize Spotify: {}", e);
                return Ok(());
            }

            match args.first().map(String::as_str) {
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
                    println!(
                        "Note: Advanced Spotify controls are handled by the main application"
                    );
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

/// Run CLI mode with various commands
pub async fn run_cli_mode(args: &[String]) -> Result<()> {
    if args.is_empty() {
        print_help();
        return Ok(());
    }

    match args[0].as_str() {
        "auth" => handle_auth_command(&args[1..]).await,
        "config" => handle_config_command(&args[1..]).await,
        "cards" => handle_cards_command(&args[1..]).await,
        "status" => handle_status_command(&args[1..]).await,
        "spotify" => handle_spotify_command(&args[1..]).await,
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
    println!("  config    Configuration and card management");
    println!("  cards     Card mapping management (list, add, remove)");
    println!("  status    Show application status");
    println!("  spotify   Spotify operations");
    println!("  web       Start web server only (no hardware required)");
    println!("  help      Show this help message");
    println!();
    println!("Run without arguments to start hardware mode.");
    println!();
    println!("Examples:");
    println!("  turny auth login                 # Authenticate with Spotify");
    println!("  turny auth logout                # Clear authentication");
    println!("  turny status                     # Show current status");
    println!("  turny config show                # Show current configuration");
    println!("  turny config add-card <id> <uri> # Add card mapping");
    println!("  turny cards list                 # List all card mappings");
    println!("  turny spotify status             # Show Spotify Connect status");
}

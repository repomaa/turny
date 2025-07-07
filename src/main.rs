use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use hashbrown::HashMap;
use log::{error, info, warn};
use rppal::gpio::{Gpio, InputPin, Level, OutputPin};
use rspotify::{
    AuthCodeSpotify, Credentials, OAuth,
    model::{RepeatState, PlaylistId, PlayContextId},
    clients::OAuthClient,
};
use std::{
    collections::HashSet,
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::time::sleep;

// RFID reader imports
use linux_embedded_hal as hal;
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use hal::spidev::{SpiModeFlags, SpidevOptions};
use hal::{Delay, SpidevBus, SysfsPin};
use mfrc522::comm::blocking::spi::SpiInterface;
use mfrc522::Mfrc522;

// Spotify Connect integration
mod spotify_connect;
use spotify_connect::SpotifyConnect;

const BUTTON_PIN: u8 = 27;
const LED_PIN: u8 = 22;  // Matches reference.py exactly - same as MFRC522 RST pin
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const DEVICE_ID: &str = "d295ff8dc55fa0b2ec7f612119675301d38f802c";

#[derive(Debug, Clone)]
pub enum ButtonEvent {
    Pressed,
    Released(Duration),
}

pub trait RfidReader {
    fn read_card_id(&mut self) -> Option<String>;
    fn is_available(&mut self) -> bool;
}

pub struct Mfrc522RfidReader {
    spi: ExclusiveDevice<SpidevBus, SysfsPin, Delay>,
}

impl Mfrc522RfidReader {
    pub fn new() -> Result<Self> {
        let mut delay = Delay;
        
        // Initialize SPI
        let mut spi = SpidevBus::open("/dev/spidev0.0")
            .context("Failed to open SPI device")?;
        let options = SpidevOptions::new()
            .max_speed_hz(1_000_000)
            .mode(SpiModeFlags::SPI_MODE_0 | SpiModeFlags::SPI_NO_CS)
            .build();
        spi.configure(&options)
            .context("Failed to configure SPI")?;

        // Setup chip select pin (GPIO8 - CE0, matching SimpleMFRC522 standard)
        let cs_pin = SysfsPin::new(8);
        cs_pin.export().context("Failed to export RFID CS pin")?;
        
        // Wait for pin to be exported
        while !cs_pin.is_exported() {}
        delay.delay_ms(500u32);
        
        let cs_pin = cs_pin.into_output_pin(embedded_hal::digital::PinState::High)
            .context("Failed to set RFID CS pin as output")?;

        // Create SPI device
        let spi = ExclusiveDevice::new(spi, cs_pin, Delay)?;
        
        info!("MFRC522 SPI interface initialized (RST shared with LED on GPIO 22)");
        
        Ok(Self { spi })
    }
}

impl RfidReader for Mfrc522RfidReader {
    fn read_card_id(&mut self) -> Option<String> {
        // Create a simplified MFRC522 interface for each read
        let itf = SpiInterface::new(&mut self.spi);
        match Mfrc522::new(itf).init() {
            Ok(mut mfrc522) => {
                match mfrc522.reqa() {
                    Ok(atqa) => {
                        match mfrc522.select(&atqa) {
                            Ok(uid) => {
                                // Convert UID bytes to string
                                let uid_bytes = uid.as_bytes();
                                let uid_string = uid_bytes.iter()
                                    .map(|b| format!("{:02x}", b))
                                    .collect::<Vec<_>>()
                                    .join("");
                                
                                info!("RFID card detected: {}", uid_string);
                                
                                // Halt the card to prevent repeated reads
                                let _ = mfrc522.hlta();
                                
                                Some(uid_string)
                            }
                            Err(_) => None,
                        }
                    }
                    Err(_) => None,
                }
            }
            Err(_) => None,
        }
    }
    
    fn is_available(&mut self) -> bool {
        // Try to create and initialize the MFRC522 interface
        let itf = SpiInterface::new(&mut self.spi);
        match Mfrc522::new(itf).init() {
            Ok(mut mfrc522) => {
                // Check if we can read the version register
                mfrc522.version().is_ok()
            }
            Err(_) => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TurnyConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub device_id: String,
    pub playlist_map: HashMap<String, String>,
}

impl Default for TurnyConfig {
    fn default() -> Self {
        let mut playlist_map = HashMap::new();
        playlist_map.insert(
            "383951559086".to_string(),
            "spotify:playlist:4Y6ZFtrQX7vuKVGLbNQ5sN".to_string(),
        );

        Self {
            client_id: "6408760457ed45538740a3f13f369722".to_string(),
            client_secret: "72ad08a2fe204c8894bdb1a7a8c9a866".to_string(),
            redirect_uri: "https://jokke.space/callback".to_string(),
            device_id: DEVICE_ID.to_string(),
            playlist_map,
        }
    }
}

#[derive(Debug)]
pub struct TurnyState {
    pub current_id: Option<String>,
    pub context_uri: Option<String>,
    pub is_playing: bool,
    pub last_heartbeat: DateTime<Utc>,
    pub button_press_start: Option<Instant>,
    pub button_action_handled: bool,
    pub absence_count: u32,
}

impl Default for TurnyState {
    fn default() -> Self {
        Self {
            current_id: None,
            context_uri: None,
            is_playing: false,
            last_heartbeat: Utc::now(),
            button_press_start: None,
            button_action_handled: false,
            absence_count: 0,
        }
    }
}

pub struct Turny {
    config: TurnyConfig,
    state: Arc<Mutex<TurnyState>>,
    spotify: AuthCodeSpotify,
    spotify_connect: SpotifyConnect,
    button: InputPin,
    led: OutputPin,
    rfid_reader: Box<dyn RfidReader + Send>,
}

impl Turny {
    pub async fn new(config: TurnyConfig) -> Result<Self> {
        // Initialize Spotify client
        let creds = Credentials::new(&config.client_id, &config.client_secret);
        let oauth = OAuth {
            redirect_uri: config.redirect_uri.clone(),
            scopes: [
                "user-read-playback-state",
                "user-modify-playback-state",
            ].iter().map(|s| s.to_string()).collect::<HashSet<String>>(),
            ..Default::default()
        };
        let spotify = AuthCodeSpotify::new(creds, oauth);

        // Initialize GPIO
        let gpio = Gpio::new().context("Failed to initialize GPIO")?;
        let button = gpio
            .get(BUTTON_PIN)
            .context("Failed to get button pin")?
            .into_input_pullup();
        let led = gpio
            .get(LED_PIN)
            .context("Failed to get LED pin")?
            .into_output();

        // Initialize RFID reader
        let rfid_reader: Box<dyn RfidReader + Send> = Box::new(
            Mfrc522RfidReader::new()
                .context("Failed to initialize MFRC522 RFID reader")?
        );

        // Initialize Spotify Connect
        let spotify_connect = SpotifyConnect::new(
            "Turny Speaker".to_string(),
            config.device_id.clone(),
            config.client_id.clone(),
            config.client_secret.clone(),
            config.redirect_uri.clone(),
        );

        Ok(Self {
            config,
            state: Arc::new(Mutex::new(TurnyState::default())),
            spotify,
            spotify_connect,
            button,
            led,
            rfid_reader,
        })
    }

    pub async fn initialize_spotify(&mut self) -> Result<()> {
        // Check if we have valid authentication
        if !self.spotify_connect.has_valid_token() {
            warn!("No valid Spotify authentication found!");
            warn!("Please authenticate using OAuth. Visit this URL:");
            warn!("{}", self.get_oauth_url());
            warn!("After authentication, call authenticate_with_code() with the authorization code");
            return Err(anyhow::anyhow!("Authentication required before initializing Spotify"));
        }
        
        // Initialize Spotify Connect (librespot) with existing token
        info!("Initializing Spotify Connect with existing authentication...");
        self.spotify_connect.initialize().await?;
        self.spotify_connect.start().await?;
        
        // Initialize Spotify Web API client
        info!("Initializing Spotify Web API client...");
        
        // Set initial playback state
        if let Err(e) = self.spotify.pause_playback(Some(&self.config.device_id)).await {
            error!("Failed to pause playback: {}", e);
        }

        if let Err(e) = self.spotify.repeat(RepeatState::Context, Some(&self.config.device_id)).await {
            error!("Failed to set repeat mode: {}", e);
        }

        if let Err(e) = self.spotify.volume(70, Some(&self.config.device_id)).await {
            error!("Failed to set volume: {}", e);
        }

        Ok(())
    }

    fn read_rfid_id(&mut self) -> Option<String> {
        self.rfid_reader.read_card_id()
    }

    fn reset_state(&self) {
        let mut state = self.state.lock().unwrap();
        state.current_id = None;
        state.context_uri = None;
        state.is_playing = false;
        state.absence_count = 0;
    }

    async fn handle_button_press(&mut self, duration: Duration) -> Result<()> {
        if duration >= Duration::from_secs(5) {
            // Manual reset
            self.manual_reset().await?;
        } else if duration >= Duration::from_secs(1) {
            // Previous track
            info!("Previous track");
            if let Err(e) = self.spotify.previous_track(Some(&self.config.device_id)).await {
                error!("Error going to previous track: {}", e);
            }
        } else {
            // Next track
            info!("Next track");
            if let Err(e) = self.spotify.next_track(Some(&self.config.device_id)).await {
                error!("Error going to next track: {}", e);
            }
        }
        Ok(())
    }

    async fn check_heartbeat(&self) -> bool {
        let mut retries = 0;
        while retries < 10 {
            let retry_delay = Duration::from_millis(1000 + retries * 500);
            
            match self.spotify.device().await {
                Ok(devices) => {
                    for device in devices {
                        if device.id.as_ref() == Some(&self.config.device_id) && device.is_active {
                            return true;
                        }
                    }
                    error!("Heartbeat check ({}/10) failed: device missing or inactive. Next retry in {:?}", retries + 1, retry_delay);
                }
                Err(e) => {
                    error!("Heartbeat check ({}/10) failed: {}. Next retry in {:?}", retries + 1, e, retry_delay);
                }
            }
            
            retries += 1;
            sleep(retry_delay).await;
        }
        false
    }

    async fn restart_spotify_connect(&mut self) -> bool {
        info!("Restarting Spotify Connect...");
        
        match self.spotify_connect.restart().await {
            Ok(()) => {
                info!("Spotify Connect restarted successfully");
                true
            }
            Err(e) => {
                error!("Failed to restart Spotify Connect: {}", e);
                false
            }
        }
    }

    pub fn get_oauth_url(&self) -> String {
        self.spotify_connect.get_oauth_url()
    }

    pub async fn authenticate_with_code(&mut self, code: &str) -> Result<()> {
        info!("Authenticating with OAuth code...");
        
        let (access_token, refresh_token) = self.spotify_connect.exchange_code_for_token(code).await
            .context("Failed to exchange code for token")?;
        
        info!("OAuth tokens obtained successfully");
        
        // Initialize SpotifyConnect with the tokens
        self.spotify_connect.initialize_with_token(access_token, Some(refresh_token)).await
            .context("Failed to initialize Spotify Connect with tokens")?;
        
        // Start the Spotify Connect service
        self.spotify_connect.start().await
            .context("Failed to start Spotify Connect service")?;
        
        info!("Spotify Connect authenticated and started");
        Ok(())
    }

    pub async fn refresh_authentication(&mut self) -> Result<()> {
        info!("Refreshing authentication...");
        
        let _new_access_token = self.spotify_connect.refresh_access_token().await
            .context("Failed to refresh access token")?;
        
        info!("Access token refreshed successfully");
        
        // Restart SpotifyConnect with new token
        self.restart_spotify_connect().await;
        
        Ok(())
    }

    pub fn is_authenticated(&self) -> bool {
        self.spotify_connect.has_valid_token() && self.spotify_connect.is_initialized()
    }

    pub async fn ensure_authenticated(&mut self) -> Result<()> {
        if !self.is_authenticated() {
            let oauth_url = self.get_oauth_url();
            error!("Authentication required! Please visit: {}", oauth_url);
            return Err(anyhow::anyhow!("Authentication required. Visit the OAuth URL to authenticate."));
        }
        Ok(())
    }

    /// Helper function for development/testing - starts a simple HTTP server to handle OAuth callback
    pub async fn start_oauth_server(&mut self) -> Result<()> {
        use std::net::TcpListener;
        use std::thread;
        use std::sync::mpsc;

        info!("Starting OAuth authentication flow...");
        info!("Visit this URL to authenticate: {}", self.get_oauth_url());

        let (tx, rx) = mpsc::channel();
        
        // Start HTTP server in a separate thread
        thread::spawn(move || {
            let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
            info!("OAuth callback server listening on http://127.0.0.1:8080");
            
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        if let Some(code) = handle_oauth_callback(stream) {
                            tx.send(code).unwrap();
                            break;
                        }
                    }
                    Err(e) => {
                        error!("OAuth server error: {}", e);
                    }
                }
            }
        });

        // Wait for OAuth callback
        info!("Waiting for OAuth callback...");
        let code = rx.recv().map_err(|e| anyhow::anyhow!("Failed to receive OAuth code: {}", e))?;
        
        // Exchange code for token
        self.authenticate_with_code(&code).await?;
        
        info!("OAuth authentication completed successfully!");
        Ok(())
    }

    async fn manual_reset(&mut self) -> Result<()> {
        info!("Manual reset triggered");

        // Visual confirmation - blink rapidly
        self.blink_led(Duration::from_millis(100), Duration::from_millis(100), 10).await;

        // Reset internal state
        self.reset_state();

        // Try to pause any current playback
        if let Err(e) = self.spotify.pause_playback(Some(&self.config.device_id)).await {
            error!("Error pausing during reset: {}", e);
        }

        // Restart Spotify Connect
        if self.restart_spotify_connect().await {
            // Success confirmation - slow blink
            self.blink_led(Duration::from_millis(500), Duration::from_millis(500), 3).await;
        } else {
            // Error confirmation - fast blink
            self.blink_led(Duration::from_millis(50), Duration::from_millis(50), 20).await;
        }

        self.led.set_low();
        Ok(())
    }

    async fn blink_led(&mut self, on_time: Duration, off_time: Duration, count: u32) {
        for _ in 0..count {
            self.led.set_high();
            sleep(on_time).await;
            self.led.set_low();
            sleep(off_time).await;
        }
    }

    async fn play_startup_sound(&self) {
        if let Err(e) = Command::new("aplay").arg("startup.wav").output() {
            error!("Failed to play startup sound: {}", e);
        }
    }

    async fn start_playback(&mut self, context_uri: &str) -> Result<()> {
        info!("Starting playback of: {}", context_uri);
        
        self.led.set_high();
        
        match self.spotify.start_context_playback(
            PlayContextId::Playlist(PlaylistId::from_id(context_uri).unwrap()),
            Some(&self.config.device_id),
            None,
            None,
        ).await {
            Ok(_) => {
                let mut state = self.state.lock().unwrap();
                state.is_playing = true;
                Ok(())
            }
            Err(e) => {
                error!("Error starting playback: {}", e);
                self.led.set_low();
                let mut state = self.state.lock().unwrap();
                state.is_playing = false;
                Err(e.into())
            }
        }
    }

    async fn pause_playback(&mut self) -> Result<()> {
        info!("Pausing playback");
        
        self.led.set_low();
        
        match self.spotify.pause_playback(Some(&self.config.device_id)).await {
            Ok(_) => {
                let mut state = self.state.lock().unwrap();
                state.is_playing = false;
                Ok(())
            }
            Err(e) => {
                error!("Error pausing playback: {}", e);
                // Still set is_playing to false to prevent stuck state
                let mut state = self.state.lock().unwrap();
                state.is_playing = false;
                Err(e.into())
            }
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        // Check authentication status
        if !self.is_authenticated() {
            warn!("Spotify authentication required!");
            warn!("Please visit: {}", self.get_oauth_url());
            warn!("After authentication, restart the application");
            
            // Show authentication required pattern - fast blinking
            self.blink_led(Duration::from_millis(100), Duration::from_millis(100), 20).await;
            
            return Err(anyhow::anyhow!("Authentication required. Please authenticate via OAuth and restart."));
        }

        // Initialize Spotify
        if let Err(e) = self.initialize_spotify().await {
            error!("Failed to initialize Spotify: {}", e);
            // Show error pattern - slow blinking
            self.blink_led(Duration::from_millis(1000), Duration::from_millis(1000), 5).await;
            return Err(e);
        }

        // Startup indication
        sleep(Duration::from_secs(1)).await;
        self.blink_led(Duration::from_millis(500), Duration::from_millis(500), 3).await;
        self.play_startup_sound().await;

        // Button monitoring state
        let mut button_press_start: Option<Instant> = None;
        let mut last_button_level = self.button.read();

        // Main loop
        loop {
            // Button monitoring
            let current_button_level = self.button.read();
            if current_button_level != last_button_level {
                match current_button_level {
                    Level::Low => {
                        // Button pressed
                        button_press_start = Some(Instant::now());
                        let mut state = self.state.lock().unwrap();
                        state.button_press_start = button_press_start;
                        state.button_action_handled = false;
                    }
                    Level::High => {
                        // Button released
                        if let Some(press_start) = button_press_start {
                            let duration = press_start.elapsed();
                            if let Err(e) = self.handle_button_press(duration).await {
                                error!("Error handling button press: {}", e);
                            }
                        }
                        button_press_start = None;
                        let mut state = self.state.lock().unwrap();
                        state.button_press_start = None;
                    }
                }
            }
            last_button_level = current_button_level;
            
            // Periodic heartbeat check
            {
                let mut state = self.state.lock().unwrap();
                if Utc::now().signed_duration_since(state.last_heartbeat).to_std().unwrap_or(Duration::ZERO) >= HEARTBEAT_INTERVAL {
                    state.last_heartbeat = Utc::now();
                    drop(state);
                    
                    if !self.check_heartbeat().await {
                        error!("Heartbeat failed, attempting to restart Spotify Connect...");
                        if self.restart_spotify_connect().await {
                            self.reset_state();
                            self.led.set_low();
                        } else {
                            error!("Failed to restart Spotify Connect, continuing...");
                        }
                    }
                }
            }

            // RFID reading logic
            if let Some(id) = self.read_rfid_id() {
                let mut state = self.state.lock().unwrap();
                state.absence_count = 0;

                if state.current_id.as_ref() != Some(&id) {
                    state.current_id = Some(id.clone());
                    state.context_uri = self.config.playlist_map.get(&id).cloned();
                    state.is_playing = false;
                    info!("New chip detected: {}", id);
                }

                // Start playback if we have a valid playlist and aren't already playing
                if let Some(context_uri) = &state.context_uri {
                    if !state.is_playing {
                        let context_uri = context_uri.clone();
                        drop(state);
                        if let Err(e) = self.start_playback(&context_uri).await {
                            error!("Failed to start playback: {}", e);
                        }
                    }
                } else if state.context_uri.is_none() {
                    warn!("Unknown chip: {}", id);
                }
            } else {
                let mut state = self.state.lock().unwrap();
                state.absence_count += 1;
                
                if state.absence_count > 3 && state.is_playing {
                    drop(state);
                    if let Err(e) = self.pause_playback().await {
                        error!("Failed to pause playback: {}", e);
                    }
                }
            }

            sleep(POLL_INTERVAL).await;
        }
    }


}

fn handle_oauth_callback(mut stream: std::net::TcpStream) -> Option<String> {
    use std::io::{Read, Write};
    
    let mut buffer = [0; 1024];
    if let Err(e) = stream.read(&mut buffer) {
        error!("Failed to read OAuth callback: {}", e);
        return None;
    }
    
    let request = String::from_utf8_lossy(&buffer[..]);
    info!("OAuth callback received: {}", request.lines().next().unwrap_or(""));
    
    // Extract authorization code from URL
    let code = if let Some(line) = request.lines().next() {
        if line.starts_with("GET /?code=") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let url_part = parts[1];
                if let Some(code_start) = url_part.find("code=") {
                    let code_part = &url_part[code_start + 5..];
                    let code_end = code_part.find('&').unwrap_or(code_part.len());
                    Some(code_part[..code_end].to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    
    // Send response
    let response = if code.is_some() {
        "HTTP/1.1 200 OK\r\n\r\n<html><body><h1>Authorization successful!</h1><p>You can close this window and return to the application.</p></body></html>"
    } else {
        "HTTP/1.1 400 Bad Request\r\n\r\n<html><body><h1>Authorization failed!</h1><p>No authorization code received.</p></body></html>"
    };
    
    if let Err(e) = stream.write(response.as_bytes()) {
        error!("Failed to write OAuth response: {}", e);
    }
    
    if let Err(e) = stream.flush() {
        error!("Failed to flush OAuth response: {}", e);
    }
    
    code
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    info!("Starting Turny Spotify RFID controller");
    
    let config = TurnyConfig::default();
    let mut turny = Turny::new(config).await?;
    
    // Check if authentication is required
    if !turny.is_authenticated() {
        info!("Authentication required. Starting OAuth flow...");
        if let Err(e) = turny.start_oauth_server().await {
            error!("OAuth authentication failed: {}", e);
            return Err(e);
        }
    }
    
    // Run the main loop
    turny.run().await?;
    
    Ok(())
}
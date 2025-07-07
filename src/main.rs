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

        Ok(Self {
            config,
            state: Arc::new(Mutex::new(TurnyState::default())),
            spotify,
            button,
            led,
            rfid_reader,
        })
    }

    pub async fn initialize_spotify(&mut self) -> Result<()> {
        // This would need proper OAuth flow implementation
        // For now, assume we have a valid token
        info!("Initializing Spotify client...");
        
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

    async fn restart_spotifyd(&self) -> bool {
        info!("Restarting spotifyd...");
        
        match Command::new("systemctl")
            .args(&["--user", "restart", "spotifyd"])
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    sleep(Duration::from_secs(5)).await;
                    info!("Spotifyd restarted");
                    true
                } else {
                    error!("Failed to restart spotifyd: {}", String::from_utf8_lossy(&output.stderr));
                    false
                }
            }
            Err(e) => {
                error!("Failed to restart spotifyd: {}", e);
                false
            }
        }
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

        // Restart spotifyd
        if self.restart_spotifyd().await {
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
        // Initialize Spotify
        self.initialize_spotify().await?;

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
                        error!("Heartbeat failed, attempting to restart spotifyd...");
                        if self.restart_spotifyd().await {
                            self.reset_state();
                            self.led.set_low();
                        } else {
                            error!("Failed to restart spotifyd, continuing...");
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

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    info!("Starting Turny Spotify RFID controller");
    
    let config = TurnyConfig::default();
    let mut turny = Turny::new(config).await?;
    
    // Run the main loop
    turny.run().await?;
    
    Ok(())
}
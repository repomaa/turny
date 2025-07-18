use anyhow::{Context, Result};
use log::{info, warn};
use rodio::{Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

/// Audio manager for handling sound playback
pub struct AudioManager {
    _stream: OutputStream,
    sink: Sink,
}

impl AudioManager {
    /// Create a new audio manager
    pub fn new() -> Result<Self> {
        let (stream, stream_handle) = OutputStream::try_default()
            .context("Failed to create audio output stream")?;
        
        let sink = Sink::try_new(&stream_handle)
            .context("Failed to create audio sink")?;
        
        Ok(AudioManager {
            _stream: stream,
            sink,
        })
    }
    
    /// Play an audio file
    pub async fn play_file(&self, path: &str) -> Result<()> {
        if !Path::new(path).exists() {
            return Err(anyhow::anyhow!("Audio file not found: {}", path));
        }
        
        let file = File::open(path)
            .with_context(|| format!("Failed to open audio file: {}", path))?;
        
        let source = Decoder::new(BufReader::new(file))
            .with_context(|| format!("Failed to decode audio file: {}", path))?;
        
        self.sink.append(source);
        
        // Wait for playback to complete
        while !self.sink.empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        
        Ok(())
    }
    
    /// Play startup sound
    pub async fn play_startup_sound(&self, file_path: &str) -> Result<()> {
        info!("Playing {}", file_path);
        self.play_file(file_path).await
    }
}

/// Convenience function to play startup sound without managing AudioManager
pub async fn play_startup_sound(file_path: &str) -> Result<()> {
    match AudioManager::new() {
        Ok(audio_manager) => {
            if let Err(e) = audio_manager.play_startup_sound(file_path).await {
                warn!("Failed to play startup sound: {}", e);
            }
        }
        Err(e) => {
            warn!("Failed to initialize audio system: {}", e);
        }
    }
    
    Ok(())
}
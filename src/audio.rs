use anyhow::{Context, Result};
use rodio::{Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::BufReader;
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
        // Synchronous file I/O is acceptable here — this is only used for the
        // short startup sound, not in the hot path. rodio's Decoder/Source
        // types are not guaranteed Send, so spawn_blocking is not viable.
        let file = File::open(path)
            .with_context(|| format!("Failed to open audio file: {}", path))?;
        
        let source = Decoder::new(BufReader::new(file))
            .with_context(|| format!("Failed to decode audio file: {}", path))?;
        
        self.sink.append(source);

        // Wait for playback to complete. rodio's sleep_until_end() is
        // synchronous and Sink is not Clone, so we can't move it into
        // spawn_blocking. Use a polling loop with a short interval instead.
        while !self.sink.empty() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        
        Ok(())
    }
    
    /// Play startup sound
    pub async fn play_startup_sound(&self, file_path: &str) -> Result<()> {
        self.play_file(file_path).await
    }
}
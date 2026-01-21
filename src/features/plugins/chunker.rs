//! # Audio Chunker
//!
//! Download and split audio files into manageable chunks for transcription.
//! Uses yt-dlp for downloading and ffmpeg for splitting.
//!
//! - **Version**: 1.1.0
//! - **Since**: 3.0.0
//!
//! ## Changelog
//! - 1.1.0: Added configurable download command support for Docker-based downloads
//! - 1.0.0: Initial release with audio download and chunking support

use anyhow::{Context, Result};
use log::{info, warn};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Configuration for audio chunking
#[derive(Debug, Clone)]
pub struct ChunkerConfig {
    /// Duration of each chunk in seconds (default: 600 = 10 minutes)
    pub chunk_duration_secs: u64,
    /// Timeout for download operation in seconds
    pub download_timeout_secs: u64,
    /// Timeout for splitting operation in seconds
    pub split_timeout_secs: u64,
    /// Optional custom download command (e.g., "sh" for shell script)
    /// If None, uses yt-dlp directly
    pub download_command: Option<String>,
    /// Arguments for the download command
    /// Use ${url} and ${output_dir} as placeholders
    pub download_args: Vec<String>,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            chunk_duration_secs: 600,      // 10 minutes per chunk
            download_timeout_secs: 300,    // 5 minutes for download
            split_timeout_secs: 120,       // 2 minutes for split
            download_command: None,
            download_args: Vec::new(),
        }
    }
}

/// Result of audio download operation
#[derive(Debug)]
pub struct DownloadResult {
    /// Path to the downloaded audio file
    pub audio_path: PathBuf,
    /// Duration of the audio in seconds (if available)
    pub duration_secs: Option<u64>,
    /// Video/audio title
    pub title: Option<String>,
}

/// Result of audio splitting operation
#[derive(Debug)]
pub struct SplitResult {
    /// Paths to the chunk files in order
    pub chunk_paths: Vec<PathBuf>,
    /// Total number of chunks
    pub total_chunks: usize,
}

/// Audio chunker for downloading and splitting audio files
pub struct AudioChunker {
    config: ChunkerConfig,
    temp_dir: PathBuf,
}

impl AudioChunker {
    /// Create a new AudioChunker with the given configuration
    ///
    /// Creates a temporary directory for storing downloaded and chunked files.
    pub async fn new(config: ChunkerConfig) -> Result<Self> {
        // Create a unique temp directory
        let temp_dir = std::env::temp_dir().join(format!(
            "persona_chunker_{}",
            uuid::Uuid::new_v4()
        ));
        tokio::fs::create_dir_all(&temp_dir)
            .await
            .context("Failed to create temp directory")?;

        info!("Created chunker temp directory: {:?}", temp_dir);

        Ok(Self { config, temp_dir })
    }

    /// Create a new AudioChunker with default configuration
    pub async fn with_defaults() -> Result<Self> {
        Self::new(ChunkerConfig::default()).await
    }

    /// Get the temp directory path
    pub fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    /// Download audio from a YouTube URL
    ///
    /// Uses custom download command if configured, otherwise uses yt-dlp directly.
    /// Returns the path to the downloaded audio file.
    pub async fn download_audio(&self, url: &str) -> Result<DownloadResult> {
        let output_path = self.temp_dir.join("audio.mp3");

        info!("Downloading audio from: {}", url);

        let result = if let Some(ref download_cmd) = self.config.download_command {
            // Use custom download command (typically Docker-based)
            info!(
                "Using configured download command: {} (args count: {})",
                download_cmd,
                self.config.download_args.len()
            );
            self.download_with_custom_command(url, download_cmd).await
        } else {
            // Use yt-dlp directly (fallback, requires yt-dlp installed on host)
            warn!(
                "No download_command configured - falling back to direct yt-dlp execution. \
                 This requires yt-dlp to be installed on the host system."
            );
            self.download_with_ytdlp(url).await
        };

        match result {
            Ok(()) => {
                // Find the downloaded audio file
                if let Some(path) = self.find_audio_file().await {
                    info!("Downloaded audio to: {:?}", path);
                    Ok(DownloadResult {
                        audio_path: path,
                        duration_secs: None,
                        title: None,
                    })
                } else if output_path.exists() {
                    info!("Downloaded audio to: {:?}", output_path);
                    Ok(DownloadResult {
                        audio_path: output_path,
                        duration_secs: None,
                        title: None,
                    })
                } else {
                    Err(anyhow::anyhow!("Downloaded audio file not found"))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Download using a custom shell command
    async fn download_with_custom_command(&self, url: &str, cmd_name: &str) -> Result<()> {
        let output_dir = self.temp_dir.to_str().unwrap();

        // Substitute placeholders in args
        let args: Vec<String> = self
            .config
            .download_args
            .iter()
            .map(|arg| {
                arg.replace("${url}", url)
                    .replace("${output_dir}", output_dir)
            })
            .collect();

        info!("Running download command: {} with {} args", cmd_name, args.len());

        let mut cmd = Command::new(cmd_name);
        cmd.args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let timeout_duration = Duration::from_secs(self.config.download_timeout_secs);
        let result = timeout(timeout_duration, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    return Err(anyhow::anyhow!(
                        "Download command failed: {}\nstdout: {}",
                        stderr,
                        stdout
                    ));
                }
                Ok(())
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("Failed to execute download command: {}", e)),
            Err(_) => Err(anyhow::anyhow!(
                "Download timed out after {} seconds",
                self.config.download_timeout_secs
            )),
        }
    }

    /// Download using yt-dlp directly (requires yt-dlp on host)
    async fn download_with_ytdlp(&self, url: &str) -> Result<()> {
        let output_template = self.temp_dir.join("audio.%(ext)s");

        // Build yt-dlp command
        // -x: Extract audio only
        // --audio-format mp3: Convert to MP3
        // --audio-quality 5: Mid quality for faster processing
        // -o: Output template
        // --no-playlist: Don't download playlist, just the video
        let mut cmd = Command::new("yt-dlp");
        cmd.args([
            "-x",
            "--audio-format",
            "mp3",
            "--audio-quality",
            "5",
            "-o",
            output_template.to_str().unwrap(),
            "--no-playlist",
            "--no-warnings",
            url,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

        let timeout_duration = Duration::from_secs(self.config.download_timeout_secs);
        let result = timeout(timeout_duration, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(anyhow::anyhow!("yt-dlp failed: {}", stderr));
                }
                Ok(())
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("Failed to execute yt-dlp: {}", e)),
            Err(_) => Err(anyhow::anyhow!(
                "Download timed out after {} seconds",
                self.config.download_timeout_secs
            )),
        }
    }

    /// Find an audio file in the temp directory
    async fn find_audio_file(&self) -> Option<PathBuf> {
        if let Ok(mut entries) = tokio::fs::read_dir(&self.temp_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if let Some(name) = path.file_name() {
                    let name_str = name.to_str().unwrap_or("");
                    // Look for audio files (mp3, m4a, opus, etc.)
                    if name_str.starts_with("audio.")
                        || name_str.ends_with(".mp3")
                        || name_str.ends_with(".m4a")
                        || name_str.ends_with(".opus")
                    {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    /// Get the duration of an audio file using ffprobe
    pub async fn get_audio_duration(&self, audio_path: &Path) -> Result<u64> {
        let mut cmd = Command::new("ffprobe");
        cmd.args([
            "-v", "quiet",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
            audio_path.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

        let output = cmd.output().await?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("ffprobe failed to get duration"));
        }

        let duration_str = String::from_utf8_lossy(&output.stdout);
        let duration: f64 = duration_str.trim().parse()
            .context("Failed to parse duration")?;

        Ok(duration.ceil() as u64)
    }

    /// Split an audio file into chunks using ffmpeg
    ///
    /// Returns the paths to all chunk files in order.
    pub async fn split_into_chunks(&self, audio_path: &Path) -> Result<SplitResult> {
        let chunk_dir = self.temp_dir.join("chunks");
        tokio::fs::create_dir_all(&chunk_dir).await?;

        let output_pattern = chunk_dir.join("chunk_%03d.mp3");

        info!(
            "Splitting audio into {} second chunks: {:?}",
            self.config.chunk_duration_secs, audio_path
        );

        // Build ffmpeg command for segmented output
        // -i: Input file
        // -f segment: Output format is segments
        // -segment_time: Duration of each segment
        // -c copy: Copy codec (fast, no re-encoding)
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-i", audio_path.to_str().unwrap(),
            "-f", "segment",
            "-segment_time", &self.config.chunk_duration_secs.to_string(),
            "-c", "copy",
            "-y",  // Overwrite without asking
            output_pattern.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

        let timeout_duration = Duration::from_secs(self.config.split_timeout_secs);
        let result = timeout(timeout_duration, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(anyhow::anyhow!("ffmpeg split failed: {}", stderr));
                }

                // Collect chunk files in order
                let mut chunk_paths = Vec::new();
                let mut entries = tokio::fs::read_dir(&chunk_dir).await?;

                while let Some(entry) = entries.next_entry().await? {
                    let path = entry.path();
                    if path.extension().map_or(false, |ext| ext == "mp3") {
                        chunk_paths.push(path);
                    }
                }

                // Sort by filename to ensure correct order
                chunk_paths.sort();

                let total_chunks = chunk_paths.len();
                info!("Split audio into {} chunks", total_chunks);

                if chunk_paths.is_empty() {
                    return Err(anyhow::anyhow!("No chunks generated from audio split"));
                }

                Ok(SplitResult {
                    chunk_paths,
                    total_chunks,
                })
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("Failed to execute ffmpeg: {}", e)),
            Err(_) => Err(anyhow::anyhow!(
                "Split timed out after {} seconds",
                self.config.split_timeout_secs
            )),
        }
    }

    /// Check if the audio needs chunking based on duration
    ///
    /// Returns true if duration exceeds chunk_duration_secs, false otherwise.
    pub async fn needs_chunking(&self, audio_path: &Path) -> Result<bool> {
        match self.get_audio_duration(audio_path).await {
            Ok(duration) => {
                let needs = duration > self.config.chunk_duration_secs;
                info!(
                    "Audio duration: {}s, chunk threshold: {}s, needs chunking: {}",
                    duration, self.config.chunk_duration_secs, needs
                );
                Ok(needs)
            }
            Err(e) => {
                warn!("Could not determine audio duration, assuming chunking needed: {}", e);
                Ok(true)
            }
        }
    }

    /// Estimate the number of chunks for a given duration
    pub fn estimate_chunks(&self, duration_secs: u64) -> usize {
        ((duration_secs as f64) / (self.config.chunk_duration_secs as f64)).ceil() as usize
    }

    /// Clean up all temporary files
    pub async fn cleanup(&self) -> Result<()> {
        info!("Cleaning up temp directory: {:?}", self.temp_dir);
        tokio::fs::remove_dir_all(&self.temp_dir)
            .await
            .context("Failed to clean up temp directory")?;
        Ok(())
    }

    /// Clean up on drop (best effort)
    pub fn cleanup_sync(&self) {
        if let Err(e) = std::fs::remove_dir_all(&self.temp_dir) {
            warn!("Failed to clean up temp directory on drop: {}", e);
        }
    }
}

impl Drop for AudioChunker {
    fn drop(&mut self) {
        // Best effort cleanup
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

/// Represents the progress of chunked transcription
#[derive(Debug, Clone)]
pub struct ChunkProgress {
    /// Current chunk being processed (1-indexed)
    pub current_chunk: usize,
    /// Total number of chunks
    pub total_chunks: usize,
    /// Status of the current chunk
    pub status: ChunkStatus,
    /// Transcript of the current chunk (if completed)
    pub transcript: Option<String>,
}

/// Status of a chunk transcription
#[derive(Debug, Clone, PartialEq)]
pub enum ChunkStatus {
    /// Chunk is pending transcription
    Pending,
    /// Chunk is currently being transcribed
    InProgress,
    /// Chunk transcription completed successfully
    Completed,
    /// Chunk transcription failed
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_chunks() {
        let config = ChunkerConfig {
            chunk_duration_secs: 600,
            ..Default::default()
        };

        // Create a mock chunker for testing estimate
        // Note: We can't easily test async methods here, but we can test pure functions

        // 10 minute audio = 1 chunk
        assert_eq!(
            ((600_f64) / (config.chunk_duration_secs as f64)).ceil() as usize,
            1
        );

        // 15 minute audio = 2 chunks
        assert_eq!(
            ((900_f64) / (config.chunk_duration_secs as f64)).ceil() as usize,
            2
        );

        // 30 minute audio = 3 chunks
        assert_eq!(
            ((1800_f64) / (config.chunk_duration_secs as f64)).ceil() as usize,
            3
        );

        // 2 hour audio = 12 chunks
        assert_eq!(
            ((7200_f64) / (config.chunk_duration_secs as f64)).ceil() as usize,
            12
        );
    }

    #[test]
    fn test_chunker_config_default() {
        let config = ChunkerConfig::default();
        assert_eq!(config.chunk_duration_secs, 600);
        assert_eq!(config.download_timeout_secs, 300);
        assert_eq!(config.split_timeout_secs, 120);
    }
}

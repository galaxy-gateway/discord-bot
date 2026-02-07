//! # Feature: Audio Transcription
//!
//! Whisper-powered transcription of audio attachments with automatic format conversion.
//! Supports a wide range of audio and video formats via ffmpeg conversion.
//!
//! - **Version**: 1.4.0
//! - **Since**: 0.1.0
//! - **Toggleable**: true
//!
//! ## Changelog
//! - 1.4.0: Added audio duration tracking for usage metrics via ffprobe
//! - 1.3.0: Fixed double-posting bug, added configurable output mode (transcription_only/with_commentary)
//! - 1.2.0: Added ffmpeg conversion for broader format support
//! - 1.1.0: Added configurable transcription modes (always/mention_only/disabled)
//! - 1.0.0: Initial release with Whisper API integration

use anyhow::Result;
use log::{debug, error, info, warn};
use std::process::Command;
use std::time::Instant;
use tokio::fs;

/// Result of audio transcription with duration for usage tracking
#[derive(Debug)]
pub struct TranscriptionResult {
    pub text: String,
    pub duration_seconds: f64,
}

/// Formats that OpenAI Whisper supports natively (no conversion needed)
const WHISPER_NATIVE_FORMATS: &[&str] =
    &[".mp3", ".mp4", ".m4a", ".wav", ".webm", ".mpeg", ".mpga"];

/// All formats we accept (will convert if not native)
const SUPPORTED_FORMATS: &[&str] = &[
    // Whisper native
    ".mp3", ".mp4", ".m4a", ".wav", ".webm", ".mpeg", ".mpga",
    // Require conversion via ffmpeg
    ".flac", ".ogg", ".aac", ".wma", ".mov", ".avi", ".mkv", ".opus", ".m4v",
];

#[derive(Clone)]
pub struct AudioTranscriber {
    openai_api_key: String,
}

impl AudioTranscriber {
    pub fn new(openai_api_key: String) -> Self {
        AudioTranscriber { openai_api_key }
    }

    pub async fn transcribe_file(&self, file_path: &str) -> Result<String> {
        info!("Transcribing audio file: {file_path}");

        if !self.is_audio_file(file_path) {
            return Err(anyhow::anyhow!("File is not a supported audio format"));
        }

        if fs::metadata(file_path).await.is_err() {
            return Err(anyhow::anyhow!("Audio file not found: {}", file_path));
        }

        let output = Command::new("curl")
            .args([
                "https://api.openai.com/v1/audio/transcriptions",
                "-H",
                &format!("Authorization: Bearer {}", self.openai_api_key),
                "-H",
                "Content-Type: multipart/form-data",
                "-F",
                &format!("file=@{file_path}"),
                "-F",
                "model=whisper-1",
            ])
            .output()?;

        if output.status.success() {
            let response = String::from_utf8(output.stdout)?;
            let json: serde_json::Value = serde_json::from_str(&response)?;

            if let Some(text) = json.get("text").and_then(|t| t.as_str()) {
                info!(
                    "Transcription successful, length: {} characters",
                    text.len()
                );
                Ok(text.to_string())
            } else if let Some(error) = json.get("error") {
                error!("OpenAI API error: {error}");
                Err(anyhow::anyhow!("OpenAI API error: {}", error))
            } else {
                error!("Unexpected response format: {response}");
                Err(anyhow::anyhow!("Unexpected response format"))
            }
        } else {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            error!("Transcription failed: {error_msg}");
            Err(anyhow::anyhow!("Transcription failed: {}", error_msg))
        }
    }

    /// Check if file is a supported audio/video format
    fn is_audio_file(&self, file_path: &str) -> bool {
        let file_path_lower = file_path.to_lowercase();
        SUPPORTED_FORMATS
            .iter()
            .any(|ext| file_path_lower.ends_with(ext))
    }

    /// Check if file format needs conversion before sending to Whisper
    fn needs_conversion(&self, filename: &str) -> bool {
        let lower = filename.to_lowercase();
        !WHISPER_NATIVE_FORMATS
            .iter()
            .any(|ext| lower.ends_with(ext))
    }

    /// Convert audio/video file to mp3 using ffmpeg
    fn convert_to_mp3(&self, input_path: &str) -> Result<String> {
        // Generate output path by replacing extension with .mp3
        let output_path = if let Some(dot_pos) = input_path.rfind('.') {
            format!("{}.mp3", &input_path[..dot_pos])
        } else {
            format!("{input_path}.mp3")
        };

        info!("Converting {input_path} to mp3 via ffmpeg");
        let start = Instant::now();

        let output = Command::new("ffmpeg")
            .args([
                "-i",
                input_path,
                "-vn", // No video output
                "-acodec",
                "libmp3lame",
                "-q:a",
                "2",  // High quality (VBR ~190kbps)
                "-y", // Overwrite output file
                &output_path,
            ])
            .output()?;

        let duration = start.elapsed();

        if output.status.success() {
            info!("FFmpeg conversion completed in {duration:?}");
            Ok(output_path)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Check if ffmpeg is not installed
            if stderr.contains("not found") || stderr.contains("No such file") {
                error!("FFmpeg not found. Install with: apt install ffmpeg");
                return Err(anyhow::anyhow!(
                    "FFmpeg is required for this format but not installed. Install with: apt install ffmpeg"
                ));
            }

            error!("FFmpeg conversion failed: {stderr}");
            Err(anyhow::anyhow!("FFmpeg conversion failed: {}", stderr))
        }
    }

    /// Check if ffmpeg is available on the system
    pub fn is_ffmpeg_available() -> bool {
        Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get audio duration in seconds using ffprobe
    fn get_audio_duration(file_path: &str) -> f64 {
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                file_path,
            ])
            .output();

        match output {
            Ok(out) if out.status.success() => String::from_utf8(out.stdout)
                .ok()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .unwrap_or(0.0),
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                debug!("ffprobe failed: {stderr}");
                0.0
            }
            Err(e) => {
                debug!("ffprobe not available: {e}");
                0.0
            }
        }
    }

    /// Download and transcribe with duration tracking
    pub async fn download_and_transcribe_with_duration(
        &self,
        url: &str,
        filename: &str,
    ) -> Result<TranscriptionResult> {
        let temp_file = format!("/tmp/discord_audio_{filename}");
        let mut converted_file: Option<String> = None;

        info!("Downloading audio attachment: {filename}");

        // Download the file
        let output = Command::new("curl")
            .args(["-o", &temp_file, url])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("Failed to download audio file"));
        }

        // Check if conversion is needed
        let file_to_transcribe = if self.needs_conversion(filename) {
            info!("Format requires conversion: {filename}");

            match self.convert_to_mp3(&temp_file) {
                Ok(mp3_path) => {
                    converted_file = Some(mp3_path.clone());
                    mp3_path
                }
                Err(e) => {
                    // Cleanup original file before returning error
                    let _ = fs::remove_file(&temp_file).await;
                    return Err(e);
                }
            }
        } else {
            temp_file.clone()
        };

        // Get audio duration before transcription (for usage tracking)
        let duration_seconds = Self::get_audio_duration(&file_to_transcribe);
        info!("Audio duration: {duration_seconds:.1}s");

        // Transcribe the file
        let transcription = self.transcribe_file(&file_to_transcribe).await;

        // Cleanup temp files
        if let Err(e) = fs::remove_file(&temp_file).await {
            warn!("Failed to cleanup temp file {temp_file}: {e}");
        }

        if let Some(ref converted) = converted_file {
            if let Err(e) = fs::remove_file(converted).await {
                warn!("Failed to cleanup converted file {converted}: {e}");
            }
        }

        transcription.map(|text| TranscriptionResult {
            text,
            duration_seconds,
        })
    }

    /// Legacy method for backwards compatibility
    pub async fn download_and_transcribe_attachment(
        &self,
        url: &str,
        filename: &str,
    ) -> Result<String> {
        let result = self
            .download_and_transcribe_with_duration(url, filename)
            .await?;
        Ok(result.text)
    }
}

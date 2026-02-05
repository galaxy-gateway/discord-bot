//! # YouTube URL Parsing and Playlist Enumeration
//!
//! Parse YouTube URLs and enumerate playlist videos using yt-dlp.
//!
//! - **Version**: 1.2.0
//! - **Since**: 1.0.0
//!
//! ## Changelog
//! - 1.2.0: Added video_url() method to get clean video URLs without playlist parameters
//! - 1.1.0: Added VideoMetadata, fetch_video_metadata, format_description_preview for video descriptions
//! - 1.0.0: Initial release with URL parsing and yt-dlp playlist enumeration

use anyhow::{anyhow, Result};
use log::{debug, info, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::process::Command;

/// Type of YouTube URL detected
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum YouTubeUrlType {
    /// Single video URL (no playlist context)
    SingleVideo,
    /// Direct playlist URL (youtube.com/playlist?list=...)
    Playlist,
    /// Video URL that includes a playlist reference (&list=...)
    VideoInPlaylist,
}

/// Parsed YouTube URL information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YouTubeUrl {
    /// Original URL
    pub original_url: String,
    /// Extracted video ID (if present)
    pub video_id: Option<String>,
    /// Extracted playlist ID (if present)
    pub playlist_id: Option<String>,
    /// Type of URL detected
    pub url_type: YouTubeUrlType,
}

impl YouTubeUrl {
    /// Check if this URL contains a playlist
    pub fn has_playlist(&self) -> bool {
        self.playlist_id.is_some()
    }

    /// Check if this is a single video (no playlist)
    pub fn is_single_video(&self) -> bool {
        self.url_type == YouTubeUrlType::SingleVideo
    }

    /// Get a clean video URL without playlist parameters
    ///
    /// Returns a URL in the format `https://www.youtube.com/watch?v=VIDEO_ID`
    /// This strips out any playlist parameters that might confuse downloaders.
    pub fn video_url(&self) -> Option<String> {
        self.video_id
            .as_ref()
            .map(|id| format!("https://www.youtube.com/watch?v={}", id))
    }

    /// Get the playlist URL for enumeration
    pub fn playlist_url(&self) -> Option<String> {
        self.playlist_id
            .as_ref()
            .map(|id| format!("https://www.youtube.com/playlist?list={}", id))
    }
}

/// A video item from a playlist
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistItem {
    /// Video ID
    pub video_id: String,
    /// Video title
    pub title: String,
    /// Video URL
    pub url: String,
    /// Duration in seconds (if available)
    pub duration: Option<u64>,
    /// Position in playlist (0-indexed)
    pub index: usize,
    /// Video description (if available)
    pub description: Option<String>,
}

/// Video metadata from yt-dlp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoMetadata {
    /// Video title
    pub title: String,
    /// Video description
    pub description: Option<String>,
    /// Duration in seconds
    pub duration: Option<u64>,
    /// Uploader/channel name
    pub uploader: Option<String>,
}

/// Playlist metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistInfo {
    /// Playlist ID
    pub id: String,
    /// Playlist title
    pub title: String,
    /// Uploader/channel name
    pub uploader: Option<String>,
    /// Total video count (may differ from items if some are unavailable)
    pub video_count: usize,
    /// Videos in the playlist
    pub items: Vec<PlaylistItem>,
}

/// Parse a YouTube URL to extract video ID, playlist ID, and URL type
pub fn parse_youtube_url(url: &str) -> Result<YouTubeUrl> {
    // Patterns for different YouTube URL formats
    let video_patterns = [
        // youtube.com/watch?v=VIDEO_ID
        r"(?:youtube\.com/watch\?(?:[^&]*&)*v=)([a-zA-Z0-9_-]{11})",
        // youtube.com/shorts/VIDEO_ID
        r"youtube\.com/shorts/([a-zA-Z0-9_-]{11})",
        // youtu.be/VIDEO_ID
        r"youtu\.be/([a-zA-Z0-9_-]{11})",
    ];

    // Pattern for playlist ID
    let playlist_pattern = r"[?&]list=([a-zA-Z0-9_-]+)";
    // Direct playlist URL pattern
    let direct_playlist_pattern = r"youtube\.com/playlist\?(?:[^&]*&)*list=([a-zA-Z0-9_-]+)";

    let mut video_id: Option<String> = None;
    let mut playlist_id: Option<String> = None;

    // Try to extract video ID
    for pattern in &video_patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(caps) = re.captures(url) {
                video_id = caps.get(1).map(|m| m.as_str().to_string());
                break;
            }
        }
    }

    // Try to extract playlist ID (first check direct playlist URL)
    if let Ok(re) = Regex::new(direct_playlist_pattern) {
        if let Some(caps) = re.captures(url) {
            playlist_id = caps.get(1).map(|m| m.as_str().to_string());
        }
    }

    // If no direct playlist URL, check for list= parameter
    if playlist_id.is_none() {
        if let Ok(re) = Regex::new(playlist_pattern) {
            if let Some(caps) = re.captures(url) {
                playlist_id = caps.get(1).map(|m| m.as_str().to_string());
            }
        }
    }

    // Determine URL type
    let url_type = match (&video_id, &playlist_id) {
        (None, Some(_)) => YouTubeUrlType::Playlist,
        (Some(_), Some(_)) => YouTubeUrlType::VideoInPlaylist,
        (Some(_), None) => YouTubeUrlType::SingleVideo,
        (None, None) => {
            return Err(anyhow!(
                "Invalid YouTube URL: could not extract video or playlist ID"
            ));
        }
    };

    debug!(
        "Parsed YouTube URL: video_id={:?}, playlist_id={:?}, type={:?}",
        video_id, playlist_id, url_type
    );

    Ok(YouTubeUrl {
        original_url: url.to_string(),
        video_id,
        playlist_id,
        url_type,
    })
}

/// Enumerate videos in a playlist using yt-dlp
///
/// Returns playlist info with all video items (up to max_videos if specified)
pub async fn enumerate_playlist(
    playlist_id: &str,
    max_videos: Option<u32>,
) -> Result<PlaylistInfo> {
    let playlist_url = format!("https://www.youtube.com/playlist?list={}", playlist_id);

    info!("Enumerating playlist: {}", playlist_id);

    // Build yt-dlp command for flat playlist extraction
    let mut cmd = Command::new("yt-dlp");
    cmd.arg("--flat-playlist")
        .arg("--dump-json")
        .arg("--no-warnings")
        .arg("--quiet");

    // Limit videos if specified
    if let Some(max) = max_videos {
        cmd.arg("--playlist-end").arg(max.to_string());
    }

    cmd.arg(&playlist_url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("yt-dlp failed to enumerate playlist: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse JSONL output (one JSON object per line)
    let mut items: Vec<PlaylistItem> = Vec::new();
    let mut playlist_title = String::new();
    let mut uploader: Option<String> = None;

    for (index, line) in stdout.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let json: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| anyhow!("Failed to parse yt-dlp JSON output: {}", e))?;

        // Extract video info
        let video_id = json
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing video ID in playlist item"))?
            .to_string();

        let title = json
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown Title")
            .to_string();

        let duration = json
            .get("duration")
            .and_then(|v| v.as_f64())
            .map(|d| d as u64);

        // Get playlist metadata from first item
        if index == 0 {
            playlist_title = json
                .get("playlist_title")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown Playlist")
                .to_string();

            uploader = json
                .get("playlist_uploader")
                .or_else(|| json.get("uploader"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }

        let description = json
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let item = PlaylistItem {
            video_id: video_id.clone(),
            title,
            url: format!("https://www.youtube.com/watch?v={}", video_id),
            duration,
            index,
            description,
        };

        items.push(item);
    }

    if items.is_empty() {
        return Err(anyhow!("Playlist is empty or unavailable"));
    }

    info!(
        "Enumerated {} videos from playlist '{}'",
        items.len(),
        playlist_title
    );

    Ok(PlaylistInfo {
        id: playlist_id.to_string(),
        title: playlist_title,
        uploader,
        video_count: items.len(),
        items,
    })
}

/// Fetch YouTube video/playlist title via oEmbed API
pub async fn fetch_youtube_title(url: &str) -> Option<String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://www.youtube.com/oembed")
        .query(&[("url", url), ("format", "json")])
        .send()
        .await;

    match resp {
        Ok(r) => {
            if let Ok(json) = r.json::<serde_json::Value>().await {
                json.get("title")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        }
        Err(e) => {
            warn!("Failed to fetch YouTube title: {}", e);
            None
        }
    }
}

/// Fetch video metadata (title, description, duration, uploader) using yt-dlp
///
/// Uses --no-download to quickly fetch metadata without downloading the video.
/// Times out after 30 seconds.
pub async fn fetch_video_metadata(url: &str) -> Result<VideoMetadata> {
    use tokio::time::{timeout, Duration};

    info!("Fetching video metadata for: {}", url);

    let mut cmd = Command::new("yt-dlp");
    cmd.arg("--dump-json")
        .arg("--no-download")
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("--quiet")
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = timeout(Duration::from_secs(30), cmd.output())
        .await
        .map_err(|_| anyhow!("Metadata fetch timed out after 30 seconds"))??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("yt-dlp failed to fetch metadata: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|e| anyhow!("Failed to parse yt-dlp JSON: {}", e))?;

    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown Title")
        .to_string();

    let description = json
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let duration = json
        .get("duration")
        .and_then(|v| v.as_f64())
        .map(|d| d as u64);

    let uploader = json
        .get("uploader")
        .or_else(|| json.get("channel"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    debug!(
        "Fetched metadata for '{}': duration={:?}, has_description={}",
        title,
        duration,
        description.is_some()
    );

    Ok(VideoMetadata {
        title,
        description,
        duration,
        uploader,
    })
}

/// Format a description for preview display, truncating to max lines
///
/// Returns a preview of the description limited to the specified number of lines.
/// Adds "..." if the description was truncated.
pub fn format_description_preview(description: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = description.lines().collect();

    if lines.len() <= max_lines {
        description.to_string()
    } else {
        let preview: String = lines[..max_lines].join("\n");
        format!("{}...", preview)
    }
}

/// Estimate total transcription time for a playlist
pub fn estimate_transcription_time(items: &[PlaylistItem]) -> std::time::Duration {
    // Rough estimate: transcription takes about 1.5x real-time on average
    // Plus ~30 seconds overhead per video for download, processing, etc.
    let total_duration: u64 = items.iter().filter_map(|item| item.duration).sum();

    let estimated_seconds = (total_duration as f64 * 1.5) as u64 + (items.len() as u64 * 30);
    std::time::Duration::from_secs(estimated_seconds)
}

/// Format a duration as a human-readable string
pub fn format_duration(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;

    if hours > 0 {
        format!("~{}h {}m", hours, minutes)
    } else if minutes > 0 {
        format!("~{}m", minutes)
    } else {
        "< 1m".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_video() {
        let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        let parsed = parse_youtube_url(url).unwrap();

        assert_eq!(parsed.video_id, Some("dQw4w9WgXcQ".to_string()));
        assert_eq!(parsed.playlist_id, None);
        assert_eq!(parsed.url_type, YouTubeUrlType::SingleVideo);
        assert!(!parsed.has_playlist());
        assert!(parsed.is_single_video());
    }

    #[test]
    fn test_parse_shorts() {
        let url = "https://www.youtube.com/shorts/dQw4w9WgXcQ";
        let parsed = parse_youtube_url(url).unwrap();

        assert_eq!(parsed.video_id, Some("dQw4w9WgXcQ".to_string()));
        assert_eq!(parsed.url_type, YouTubeUrlType::SingleVideo);
    }

    #[test]
    fn test_parse_youtu_be() {
        let url = "https://youtu.be/dQw4w9WgXcQ";
        let parsed = parse_youtube_url(url).unwrap();

        assert_eq!(parsed.video_id, Some("dQw4w9WgXcQ".to_string()));
        assert_eq!(parsed.url_type, YouTubeUrlType::SingleVideo);
    }

    #[test]
    fn test_parse_playlist() {
        let url = "https://www.youtube.com/playlist?list=PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf";
        let parsed = parse_youtube_url(url).unwrap();

        assert_eq!(parsed.video_id, None);
        assert_eq!(
            parsed.playlist_id,
            Some("PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf".to_string())
        );
        assert_eq!(parsed.url_type, YouTubeUrlType::Playlist);
        assert!(parsed.has_playlist());
    }

    #[test]
    fn test_parse_video_in_playlist() {
        let url =
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf";
        let parsed = parse_youtube_url(url).unwrap();

        assert_eq!(parsed.video_id, Some("dQw4w9WgXcQ".to_string()));
        assert_eq!(
            parsed.playlist_id,
            Some("PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf".to_string())
        );
        assert_eq!(parsed.url_type, YouTubeUrlType::VideoInPlaylist);
        assert!(parsed.has_playlist());
        assert!(!parsed.is_single_video());
        // video_url() should return a clean URL without playlist parameter
        assert_eq!(
            parsed.video_url(),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn test_parse_video_with_timestamp() {
        let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=120";
        let parsed = parse_youtube_url(url).unwrap();

        assert_eq!(parsed.video_id, Some("dQw4w9WgXcQ".to_string()));
        assert_eq!(parsed.url_type, YouTubeUrlType::SingleVideo);
    }

    #[test]
    fn test_invalid_url() {
        let url = "https://example.com/video";
        assert!(parse_youtube_url(url).is_err());
    }

    #[test]
    fn test_estimate_transcription_time() {
        let items = vec![
            PlaylistItem {
                video_id: "abc".to_string(),
                title: "Video 1".to_string(),
                url: "https://youtube.com/watch?v=abc".to_string(),
                duration: Some(600), // 10 minutes
                index: 0,
                description: Some("Test description".to_string()),
            },
            PlaylistItem {
                video_id: "def".to_string(),
                title: "Video 2".to_string(),
                url: "https://youtube.com/watch?v=def".to_string(),
                duration: Some(300), // 5 minutes
                index: 1,
                description: None,
            },
        ];

        let estimate = estimate_transcription_time(&items);
        // (600 + 300) * 1.5 + 2 * 30 = 1350 + 60 = 1410 seconds = ~23.5 minutes
        assert!(estimate.as_secs() > 1400);
    }

    #[test]
    fn test_format_description_preview() {
        // Short description - no truncation
        let short = "Line 1\nLine 2\nLine 3";
        assert_eq!(format_description_preview(short, 5), short);

        // Long description - truncation
        let long = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7";
        let expected = "Line 1\nLine 2\nLine 3...";
        assert_eq!(format_description_preview(long, 3), expected);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(std::time::Duration::from_secs(30)), "< 1m");
        assert_eq!(format_duration(std::time::Duration::from_secs(600)), "~10m");
        assert_eq!(
            format_duration(std::time::Duration::from_secs(5400)),
            "~1h 30m"
        );
    }
}

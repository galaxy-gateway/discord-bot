//! File utility functions for Discord file management
//!
//! Reusable helpers for downloading files, detecting content types,
//! managing filenames, and respecting Discord upload limits.
//!
//! - **Version**: 1.0.0
//! - **Since**: 4.3.0
//!
//! ## Changelog
//! - 1.0.0: Initial creation with download, content detection, and file utilities

use anyhow::{anyhow, Result};
use log::{debug, warn};

// ============================================================================
// Constants
// ============================================================================

/// Discord upload limit for servers with no boost or boost tier 1 (25 MB)
pub const DISCORD_UPLOAD_LIMIT_DEFAULT: u64 = 25 * 1024 * 1024;

/// Discord upload limit for boost tier 2 (50 MB)
pub const DISCORD_UPLOAD_LIMIT_TIER2: u64 = 50 * 1024 * 1024;

/// Discord upload limit for boost tier 3 (100 MB)
pub const DISCORD_UPLOAD_LIMIT_TIER3: u64 = 100 * 1024 * 1024;

/// Default timeout for file downloads in seconds
pub const DEFAULT_DOWNLOAD_TIMEOUT_SECS: u64 = 60;

// ============================================================================
// Types
// ============================================================================

/// Classification of HTTP response content
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentKind {
    Html,
    PlainText,
    Image,
    Audio,
    Video,
    /// PDF, DOC, DOCX, etc.
    Document,
    /// ZIP, TAR, GZ, etc.
    Archive,
    /// Catch-all for unrecognized binary content
    Binary,
}

/// A file downloaded from a URL, ready for Discord upload
pub struct DownloadedFile {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub content_type: String,
    pub size: u64,
}

// ============================================================================
// Content detection
// ============================================================================

/// Detect content kind from Content-Type header value and URL extension.
///
/// The Content-Type header is checked first. If it is empty or generic
/// (`application/octet-stream`), the URL extension is used as a fallback.
pub fn detect_content_kind(content_type: &str, url: &str) -> ContentKind {
    let ct = content_type.split(';').next().unwrap_or("").trim().to_lowercase();

    // Check Content-Type first
    if !ct.is_empty() && ct != "application/octet-stream" {
        if ct.contains("text/html") || ct.contains("application/xhtml") {
            return ContentKind::Html;
        }
        if ct.contains("text/plain") {
            return ContentKind::PlainText;
        }
        if ct.starts_with("image/") {
            return ContentKind::Image;
        }
        if ct.starts_with("audio/") {
            return ContentKind::Audio;
        }
        if ct.starts_with("video/") {
            return ContentKind::Video;
        }
        if ct == "application/pdf"
            || ct == "application/msword"
            || ct == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            || ct == "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            || ct == "application/vnd.ms-excel"
            || ct == "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            || ct == "application/vnd.ms-powerpoint"
            || ct == "application/rtf"
        {
            return ContentKind::Document;
        }
        if ct == "application/zip"
            || ct == "application/gzip"
            || ct == "application/x-tar"
            || ct == "application/x-7z-compressed"
            || ct == "application/x-rar-compressed"
            || ct == "application/x-bzip2"
        {
            return ContentKind::Archive;
        }
        if ct.starts_with("text/") {
            return ContentKind::PlainText;
        }
        // Unknown application/* or other → fall through to extension check
    }

    // Fallback: check URL extension
    let ext = url_extension(url);
    match ext {
        "html" | "htm" | "xhtml" => ContentKind::Html,
        "txt" | "csv" | "json" | "xml" | "yaml" | "yml" | "toml" | "md" | "rs" | "py" | "js"
        | "ts" | "css" | "log" => ContentKind::PlainText,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "tiff" => {
            ContentKind::Image
        }
        "mp3" | "wav" | "ogg" | "flac" | "aac" | "m4a" | "wma" => ContentKind::Audio,
        "mp4" | "webm" | "avi" | "mov" | "mkv" | "flv" | "wmv" => ContentKind::Video,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "rtf" | "odt" | "ods" => {
            ContentKind::Document
        }
        "zip" | "tar" | "gz" | "7z" | "rar" | "bz2" | "xz" | "tgz" => ContentKind::Archive,
        _ => ContentKind::Binary,
    }
}

/// Extract the lowercase file extension from a URL path (without query string).
fn url_extension(url: &str) -> &str {
    // Strip query string and fragment
    let path = url.split('?').next().unwrap_or(url);
    let path = path.split('#').next().unwrap_or(path);

    // Get last path segment
    if let Some(last_segment) = path.rsplit('/').next() {
        if let Some(dot_pos) = last_segment.rfind('.') {
            let ext = &last_segment[dot_pos + 1..];
            // Only return if it looks like a real extension (no slashes, reasonable length)
            if !ext.is_empty() && ext.len() <= 10 {
                return ext;
            }
        }
    }
    ""
}

// ============================================================================
// Filename utilities
// ============================================================================

/// Extract a filename from a URL path and optional Content-Disposition header.
///
/// Priority:
/// 1. Content-Disposition `filename=` parameter
/// 2. Last segment of the URL path
/// 3. Fallback to `"download"`
pub fn extract_filename(url: &str, content_disposition: Option<&str>) -> String {
    // Try Content-Disposition header first
    if let Some(cd) = content_disposition {
        if let Some(name) = parse_content_disposition_filename(cd) {
            if !name.is_empty() {
                return name;
            }
        }
    }

    // Try URL path
    let path = url.split('?').next().unwrap_or(url);
    let path = path.split('#').next().unwrap_or(path);
    if let Some(segment) = path.rsplit('/').next() {
        let decoded = percent_decode(segment);
        if !decoded.is_empty() && decoded != "/" && decoded.contains('.') {
            return decoded;
        }
    }

    "download".to_string()
}

/// Parse filename from Content-Disposition header value.
fn parse_content_disposition_filename(header: &str) -> Option<String> {
    // Look for filename*= (RFC 5987 extended) or filename=
    for part in header.split(';') {
        let trimmed = part.trim();

        // filename*=UTF-8''encoded_name
        if let Some(rest) = trimmed.strip_prefix("filename*=") {
            let rest = rest.trim().trim_matches('"');
            // Strip encoding prefix like UTF-8''
            if let Some(pos) = rest.find("''") {
                let decoded = percent_decode(&rest[pos + 2..]);
                if !decoded.is_empty() {
                    return Some(decoded);
                }
            }
        }

        // filename="name" or filename=name
        if let Some(rest) = trimmed.strip_prefix("filename=") {
            let name = rest.trim().trim_matches('"');
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Simple percent-decode for URL segments.
fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next();
            let lo = chars.next();
            if let (Some(h), Some(l)) = (hi, lo) {
                let hex = [h, l];
                if let Ok(s) = std::str::from_utf8(&hex) {
                    if let Ok(val) = u8::from_str_radix(s, 16) {
                        result.push(val as char);
                        continue;
                    }
                }
            }
            // Failed to decode — push raw
            result.push('%');
        } else {
            result.push(b as char);
        }
    }
    result
}

/// Sanitize a string for use as a filename.
///
/// Keeps only alphanumeric characters, spaces, hyphens, underscores, and dots.
/// Replaces spaces with underscores and lowercases. Truncates to 50 characters.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_' || *c == '.')
        .take(50)
        .collect::<String>()
        .trim()
        .replace(' ', "_")
        .to_lowercase()
}

// ============================================================================
// File size utilities
// ============================================================================

/// Format a byte count as a human-readable string (e.g., "1.5 MB", "340 KB").
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Get the maximum upload size in bytes for a Discord server's boost tier.
///
/// - Tier 0–1: 25 MB
/// - Tier 2: 50 MB
/// - Tier 3: 100 MB
pub fn max_upload_size(premium_tier: u8) -> u64 {
    match premium_tier {
        3 => DISCORD_UPLOAD_LIMIT_TIER3,
        2 => DISCORD_UPLOAD_LIMIT_TIER2,
        _ => DISCORD_UPLOAD_LIMIT_DEFAULT,
    }
}

/// Check if a file size is within the Discord upload limit for the given boost tier.
pub fn is_within_upload_limit(size: u64, premium_tier: u8) -> bool {
    size <= max_upload_size(premium_tier)
}

// ============================================================================
// MIME / extension mapping
// ============================================================================

/// Map a common file extension to its MIME type.
pub fn extension_to_mime(ext: &str) -> &str {
    match ext.to_lowercase().as_str() {
        // Images
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        // Audio
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "m4a" => "audio/mp4",
        // Video
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        // Documents
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "rtf" => "application/rtf",
        // Archives
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/x-rar-compressed",
        // Text
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "csv" => "text/csv",
        "md" => "text/markdown",
        _ => "application/octet-stream",
    }
}

/// Map a MIME type to a common file extension (without dot).
pub fn mime_to_extension(mime: &str) -> &str {
    let mime = mime.split(';').next().unwrap_or("").trim();
    match mime {
        // Images
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/x-icon" => "ico",
        // Audio
        "audio/mpeg" => "mp3",
        "audio/wav" => "wav",
        "audio/ogg" => "ogg",
        "audio/flac" => "flac",
        "audio/aac" => "aac",
        "audio/mp4" => "m4a",
        // Video
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/x-msvideo" => "avi",
        "video/quicktime" => "mov",
        "video/x-matroska" => "mkv",
        // Documents
        "application/pdf" => "pdf",
        "application/msword" => "doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.ms-excel" => "xls",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "application/vnd.ms-powerpoint" => "ppt",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
        "application/rtf" => "rtf",
        // Archives
        "application/zip" => "zip",
        "application/x-tar" => "tar",
        "application/gzip" => "gz",
        "application/x-7z-compressed" => "7z",
        "application/x-rar-compressed" => "rar",
        // Text
        "text/plain" => "txt",
        "text/html" => "html",
        "text/css" => "css",
        "application/javascript" => "js",
        "application/json" => "json",
        "application/xml" | "text/xml" => "xml",
        "text/csv" => "csv",
        "text/markdown" => "md",
        _ => "bin",
    }
}

// ============================================================================
// Download
// ============================================================================

/// Download a file from a URL with size limits and timeout.
///
/// Returns a [`DownloadedFile`] containing the raw bytes, detected filename,
/// content type, and size. Returns an error if the file exceeds `max_bytes`
/// or the request times out.
pub async fn download_file(
    url: &str,
    max_bytes: u64,
    timeout_secs: u64,
) -> Result<DownloadedFile> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent("Mozilla/5.0 (compatible; PersonaBot/1.0)")
        .build()?;

    let response = client.get(url).send().await.map_err(|e| {
        if e.is_timeout() {
            anyhow!("Request timed out after {timeout_secs} seconds")
        } else if e.is_connect() {
            anyhow!("Could not connect to the server")
        } else {
            anyhow!("HTTP request failed: {e}")
        }
    })?;

    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!("Server returned HTTP {status}"));
    }

    // Check Content-Length before downloading body
    if let Some(content_length) = response.content_length() {
        if content_length > max_bytes {
            return Err(anyhow!(
                "File is too large ({}, limit {})",
                format_file_size(content_length),
                format_file_size(max_bytes)
            ));
        }
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let content_disposition = response
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    debug!("Downloading file from {url} (Content-Type: {content_type})");

    let bytes = response.bytes().await?;
    let size = bytes.len() as u64;

    if size > max_bytes {
        return Err(anyhow!(
            "File is too large ({}, limit {})",
            format_file_size(size),
            format_file_size(max_bytes)
        ));
    }

    if size == 0 {
        warn!("Downloaded file is empty (0 bytes)");
    }

    let filename = extract_filename(url, content_disposition.as_deref());

    Ok(DownloadedFile {
        bytes: bytes.to_vec(),
        filename,
        content_type,
        size,
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- detect_content_kind ----

    #[test]
    fn test_detect_html_from_content_type() {
        assert_eq!(
            detect_content_kind("text/html; charset=utf-8", "https://example.com"),
            ContentKind::Html
        );
        assert_eq!(
            detect_content_kind("application/xhtml+xml", "https://example.com"),
            ContentKind::Html
        );
    }

    #[test]
    fn test_detect_image_from_content_type() {
        assert_eq!(
            detect_content_kind("image/png", "https://example.com/file"),
            ContentKind::Image
        );
        assert_eq!(
            detect_content_kind("image/jpeg", "https://example.com/file"),
            ContentKind::Image
        );
    }

    #[test]
    fn test_detect_pdf_from_content_type() {
        assert_eq!(
            detect_content_kind("application/pdf", "https://example.com/file"),
            ContentKind::Document
        );
    }

    #[test]
    fn test_detect_archive_from_content_type() {
        assert_eq!(
            detect_content_kind("application/zip", "https://example.com/file"),
            ContentKind::Archive
        );
        assert_eq!(
            detect_content_kind("application/gzip", "https://example.com/file"),
            ContentKind::Archive
        );
    }

    #[test]
    fn test_detect_from_url_extension_fallback() {
        assert_eq!(
            detect_content_kind("application/octet-stream", "https://example.com/file.pdf"),
            ContentKind::Document
        );
        assert_eq!(
            detect_content_kind("", "https://example.com/image.png"),
            ContentKind::Image
        );
        assert_eq!(
            detect_content_kind("", "https://example.com/archive.zip"),
            ContentKind::Archive
        );
    }

    #[test]
    fn test_detect_html_from_url_extension() {
        assert_eq!(
            detect_content_kind("", "https://example.com/page.html"),
            ContentKind::Html
        );
    }

    #[test]
    fn test_detect_plain_text_variants() {
        assert_eq!(
            detect_content_kind("text/plain", "https://example.com"),
            ContentKind::PlainText
        );
        assert_eq!(
            detect_content_kind("text/csv", "https://example.com/data"),
            ContentKind::PlainText
        );
    }

    #[test]
    fn test_detect_audio_video() {
        assert_eq!(
            detect_content_kind("audio/mpeg", "https://example.com/song"),
            ContentKind::Audio
        );
        assert_eq!(
            detect_content_kind("video/mp4", "https://example.com/video"),
            ContentKind::Video
        );
    }

    #[test]
    fn test_detect_binary_fallback() {
        assert_eq!(
            detect_content_kind("", "https://example.com/unknown"),
            ContentKind::Binary
        );
    }

    // ---- extract_filename ----

    #[test]
    fn test_extract_filename_from_url() {
        assert_eq!(
            extract_filename("https://example.com/docs/report.pdf", None),
            "report.pdf"
        );
    }

    #[test]
    fn test_extract_filename_with_query_string() {
        assert_eq!(
            extract_filename("https://example.com/file.zip?token=abc", None),
            "file.zip"
        );
    }

    #[test]
    fn test_extract_filename_from_content_disposition() {
        assert_eq!(
            extract_filename(
                "https://example.com/download",
                Some("attachment; filename=\"my_report.pdf\"")
            ),
            "my_report.pdf"
        );
    }

    #[test]
    fn test_extract_filename_content_disposition_unquoted() {
        assert_eq!(
            extract_filename(
                "https://example.com/download",
                Some("attachment; filename=report.pdf")
            ),
            "report.pdf"
        );
    }

    #[test]
    fn test_extract_filename_content_disposition_extended() {
        assert_eq!(
            extract_filename(
                "https://example.com/download",
                Some("attachment; filename*=UTF-8''my%20file.pdf")
            ),
            "my file.pdf"
        );
    }

    #[test]
    fn test_extract_filename_fallback() {
        assert_eq!(
            extract_filename("https://example.com/", None),
            "download"
        );
        assert_eq!(
            extract_filename("https://example.com/noext", None),
            "download"
        );
    }

    // ---- sanitize_filename ----

    #[test]
    fn test_sanitize_filename_basic() {
        assert_eq!(sanitize_filename("My Report"), "my_report");
    }

    #[test]
    fn test_sanitize_filename_special_chars() {
        assert_eq!(
            sanitize_filename("file<>:\"/\\|?*.txt"),
            "file.txt"
        );
    }

    #[test]
    fn test_sanitize_filename_preserves_dots() {
        assert_eq!(sanitize_filename("report.final.pdf"), "report.final.pdf");
    }

    #[test]
    fn test_sanitize_filename_truncates() {
        let long_name = "a".repeat(100);
        let sanitized = sanitize_filename(&long_name);
        assert!(sanitized.len() <= 50);
    }

    // ---- format_file_size ----

    #[test]
    fn test_format_file_size_bytes() {
        assert_eq!(format_file_size(0), "0 B");
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(1023), "1023 B");
    }

    #[test]
    fn test_format_file_size_kb() {
        assert_eq!(format_file_size(1024), "1.0 KB");
        assert_eq!(format_file_size(1536), "1.5 KB");
    }

    #[test]
    fn test_format_file_size_mb() {
        assert_eq!(format_file_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_file_size(25 * 1024 * 1024), "25.0 MB");
    }

    #[test]
    fn test_format_file_size_gb() {
        assert_eq!(format_file_size(1024 * 1024 * 1024), "1.0 GB");
    }

    // ---- max_upload_size / is_within_upload_limit ----

    #[test]
    fn test_max_upload_size_tiers() {
        assert_eq!(max_upload_size(0), DISCORD_UPLOAD_LIMIT_DEFAULT);
        assert_eq!(max_upload_size(1), DISCORD_UPLOAD_LIMIT_DEFAULT);
        assert_eq!(max_upload_size(2), DISCORD_UPLOAD_LIMIT_TIER2);
        assert_eq!(max_upload_size(3), DISCORD_UPLOAD_LIMIT_TIER3);
    }

    #[test]
    fn test_is_within_upload_limit() {
        assert!(is_within_upload_limit(10 * 1024 * 1024, 0)); // 10 MB, no boost
        assert!(is_within_upload_limit(25 * 1024 * 1024, 0)); // exactly 25 MB
        assert!(!is_within_upload_limit(26 * 1024 * 1024, 0)); // 26 MB, no boost
        assert!(is_within_upload_limit(26 * 1024 * 1024, 2)); // 26 MB, tier 2
        assert!(is_within_upload_limit(99 * 1024 * 1024, 3)); // 99 MB, tier 3
        assert!(!is_within_upload_limit(101 * 1024 * 1024, 3)); // 101 MB, tier 3
    }

    // ---- MIME mappings ----

    #[test]
    fn test_extension_to_mime() {
        assert_eq!(extension_to_mime("pdf"), "application/pdf");
        assert_eq!(extension_to_mime("png"), "image/png");
        assert_eq!(extension_to_mime("mp3"), "audio/mpeg");
        assert_eq!(extension_to_mime("mp4"), "video/mp4");
        assert_eq!(extension_to_mime("zip"), "application/zip");
        assert_eq!(extension_to_mime("unknown"), "application/octet-stream");
    }

    #[test]
    fn test_mime_to_extension() {
        assert_eq!(mime_to_extension("application/pdf"), "pdf");
        assert_eq!(mime_to_extension("image/png"), "png");
        assert_eq!(mime_to_extension("audio/mpeg"), "mp3");
        assert_eq!(mime_to_extension("video/mp4"), "mp4");
        assert_eq!(mime_to_extension("application/zip"), "zip");
        assert_eq!(mime_to_extension("unknown/type"), "bin");
    }

    #[test]
    fn test_mime_to_extension_with_params() {
        assert_eq!(mime_to_extension("text/html; charset=utf-8"), "html");
    }

    #[test]
    fn test_extension_to_mime_case_insensitive() {
        assert_eq!(extension_to_mime("PDF"), "application/pdf");
        assert_eq!(extension_to_mime("Png"), "image/png");
    }

    // ---- url_extension ----

    #[test]
    fn test_url_extension() {
        assert_eq!(url_extension("https://example.com/file.pdf"), "pdf");
        assert_eq!(url_extension("https://example.com/file.pdf?q=1"), "pdf");
        assert_eq!(url_extension("https://example.com/file.pdf#section"), "pdf");
        assert_eq!(url_extension("https://example.com/noext"), "");
        assert_eq!(url_extension("https://example.com/"), "");
    }

    // ---- percent_decode ----

    #[test]
    fn test_percent_decode() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("file%2Fname"), "file/name");
        assert_eq!(percent_decode("no_encoding"), "no_encoding");
    }
}

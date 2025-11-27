//! # Feature: Image Generation
//!
//! DALL-E 3 powered image creation with configurable size (square, landscape, portrait)
//! and style (vivid, natural) options.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.2.0
//! - **Toggleable**: true
//!
//! ## Changelog
//! - 1.0.0: Initial release with DALL-E 3 integration

use anyhow::Result;
use log::{debug, error, info};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct ImageGenerator {
    openai_api_key: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSize {
    Square,      // 1024x1024
    Landscape,   // 1792x1024
    Portrait,    // 1024x1792
}

impl ImageSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImageSize::Square => "1024x1024",
            ImageSize::Landscape => "1792x1024",
            ImageSize::Portrait => "1024x1792",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "square" | "1024x1024" => Some(ImageSize::Square),
            "landscape" | "wide" | "1792x1024" => Some(ImageSize::Landscape),
            "portrait" | "tall" | "1024x1792" => Some(ImageSize::Portrait),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageStyle {
    Vivid,   // Hyper-real and dramatic
    Natural, // More natural, less hyper-real
}

impl ImageStyle {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImageStyle::Vivid => "vivid",
            ImageStyle::Natural => "natural",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "vivid" => Some(ImageStyle::Vivid),
            "natural" => Some(ImageStyle::Natural),
            _ => None,
        }
    }
}

#[derive(Serialize)]
struct DalleRequest {
    model: String,
    prompt: String,
    n: u32,
    size: String,
    style: String,
    response_format: String,
}

#[derive(Deserialize, Debug)]
struct DalleResponse {
    data: Vec<DalleImageData>,
}

#[derive(Deserialize, Debug)]
struct DalleImageData {
    url: Option<String>,
    revised_prompt: Option<String>,
}

#[derive(Deserialize, Debug)]
struct DalleError {
    error: DalleErrorDetails,
}

#[derive(Deserialize, Debug)]
struct DalleErrorDetails {
    message: String,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

/// Result of image generation
#[derive(Debug)]
pub struct GeneratedImage {
    pub url: String,
    pub revised_prompt: Option<String>,
}

impl ImageGenerator {
    pub fn new(openai_api_key: String) -> Self {
        ImageGenerator {
            openai_api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Generate an image using DALL-E 3
    pub async fn generate_image(
        &self,
        prompt: &str,
        size: ImageSize,
        style: ImageStyle,
    ) -> Result<GeneratedImage> {
        info!("Generating image with DALL-E 3 | Size: {} | Style: {} | Prompt: '{}'",
              size.as_str(), style.as_str(), prompt.chars().take(100).collect::<String>());

        let request = DalleRequest {
            model: "dall-e-3".to_string(),
            prompt: prompt.to_string(),
            n: 1,
            size: size.as_str().to_string(),
            style: style.as_str().to_string(),
            response_format: "url".to_string(),
        };

        debug!("Sending request to OpenAI DALL-E API");
        let response = self.client
            .post("https://api.openai.com/v1/images/generations")
            .header("Authorization", format!("Bearer {}", self.openai_api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        let response_text = response.text().await?;

        if status.is_success() {
            let dalle_response: DalleResponse = serde_json::from_str(&response_text)
                .map_err(|e| anyhow::anyhow!("Failed to parse DALL-E response: {}", e))?;

            if let Some(image_data) = dalle_response.data.first() {
                if let Some(url) = &image_data.url {
                    info!("Image generated successfully | URL length: {}", url.len());
                    return Ok(GeneratedImage {
                        url: url.clone(),
                        revised_prompt: image_data.revised_prompt.clone(),
                    });
                }
            }

            error!("No image data in response: {response_text}");
            Err(anyhow::anyhow!("No image data in DALL-E response"))
        } else {
            // Try to parse error response
            if let Ok(error_response) = serde_json::from_str::<DalleError>(&response_text) {
                error!("DALL-E API error: {} (type: {:?})",
                       error_response.error.message,
                       error_response.error.error_type);
                Err(anyhow::anyhow!("DALL-E error: {}", error_response.error.message))
            } else {
                error!("DALL-E API error (status {status}): {response_text}");
                Err(anyhow::anyhow!("DALL-E API error (status {})", status))
            }
        }
    }

    /// Download an image from URL to bytes
    pub async fn download_image(&self, url: &str) -> Result<Vec<u8>> {
        debug!("Downloading generated image");
        let response = self.client
            .get(url)
            .send()
            .await?;

        if response.status().is_success() {
            let bytes = response.bytes().await?;
            info!("Image downloaded | Size: {} bytes", bytes.len());
            Ok(bytes.to_vec())
        } else {
            error!("Failed to download image: {}", response.status());
            Err(anyhow::anyhow!("Failed to download image: {}", response.status()))
        }
    }
}

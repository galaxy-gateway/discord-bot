//! # Image Generation Feature
//!
//! DALL-E 3 powered image creation with size and style options.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.2.0
//! - **Toggleable**: true

pub mod generator;

pub use generator::{GeneratedImage, ImageGenerator, ImageSize, ImageStyle};

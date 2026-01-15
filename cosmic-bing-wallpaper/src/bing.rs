//! # Bing API Client
//!
//! Handles communication with the Bing Homepage Image Archive API to fetch
//! daily wallpaper metadata and download the actual images.
//!
//! ## API Endpoint
//! The Bing API is accessed at:
//! ```text
//! https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1&mkt=<market>
//! ```
//!
//! Parameters:
//! - `format=js`: Return JSON response
//! - `idx=0`: Start from today's image (0=today, 1=yesterday, etc.)
//! - `n=1`: Number of images to return
//! - `mkt=<market>`: Regional market code (e.g., "en-US")
//!
//! ## Image URLs
//! The API returns partial URLs that need `https://www.bing.com` prepended.
//! Images are typically available in multiple resolutions; this client uses
//! the default high-resolution version (1920x1080).

use serde::Deserialize;
use std::path::Path;
use chrono::Local;

/// Base URL for the Bing Homepage Image Archive API.
const BING_API_URL: &str = "https://www.bing.com/HPImageArchive.aspx";

/// Raw API response from Bing.
///
/// The API returns a JSON object with an `images` array containing
/// one or more image entries.
#[derive(Debug, Clone, Deserialize)]
pub struct BingApiResponse {
    /// Array of image metadata entries
    pub images: Vec<BingImageData>,
}

/// Raw image data as returned by the Bing API.
///
/// This struct maps directly to the JSON structure. Fields are renamed
/// using serde to match Rust naming conventions.
#[derive(Debug, Clone, Deserialize)]
pub struct BingImageData {
    /// Partial URL path (needs `https://www.bing.com` prefix)
    pub url: String,
    /// Copyright/attribution text for the image
    pub copyright: String,
    /// Image title/description
    pub title: String,
    /// Date when this image was featured (format: YYYYMMDD)
    #[serde(rename = "startdate")]
    pub start_date: String,
}

/// Processed image information ready for use in the application.
///
/// This is the canonical representation used throughout the app,
/// with the URL already prefixed with the Bing domain.
#[derive(Debug, Clone)]
pub struct BingImage {
    /// Full download URL for the image
    pub url: String,
    /// Copyright/attribution text
    pub copyright: String,
    /// Image title/description
    pub title: String,
    /// Feature date (format: YYYYMMDD) - retained for potential future use
    #[allow(dead_code)]
    pub date: String,
}

impl From<BingImageData> for BingImage {
    /// Converts raw API data to the application's image format.
    ///
    /// Prepends the Bing domain to the partial URL path.
    fn from(data: BingImageData) -> Self {
        Self {
            url: format!("https://www.bing.com{}", data.url),
            copyright: data.copyright,
            title: data.title,
            date: data.start_date,
        }
    }
}

/// Fetches today's Bing image metadata from the API.
///
/// Queries the Bing Homepage Image Archive for the current day's image
/// in the specified regional market.
///
/// # Arguments
/// * `market` - Regional market code (e.g., "en-US", "de-DE")
///
/// # Returns
/// * `Ok(BingImage)` - Image metadata including URL, title, and copyright
/// * `Err(String)` - Error message if the API request or parsing fails
///
/// # Example
/// ```ignore
/// let image = fetch_bing_image_info("en-US").await?;
/// println!("Today's image: {}", image.title);
/// ```
pub async fn fetch_bing_image_info(market: &str) -> Result<BingImage, String> {
    let url = format!(
        "{}?format=js&idx=0&n=1&mkt={}",
        BING_API_URL, market
    );

    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to fetch Bing API: {}", e))?;

    let api_response: BingApiResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Bing response: {}", e))?;

    api_response
        .images
        .into_iter()
        .next()
        .map(BingImage::from)
        .ok_or_else(|| "No images in Bing response".to_string())
}

/// Downloads a Bing image to the local wallpaper directory.
///
/// The image is saved with a date and market-based filename
/// (e.g., "bing-en-US-2026-01-15.jpg"). If the file already exists,
/// the download is skipped and the existing path is returned.
///
/// # Arguments
/// * `image` - Image metadata from [`fetch_bing_image_info`]
/// * `wallpaper_dir` - Directory to save the image (created if it doesn't exist)
/// * `market` - Market code to include in filename (allows different images per region)
///
/// # Returns
/// * `Ok(String)` - Absolute path to the downloaded (or existing) image file
/// * `Err(String)` - Error message if directory creation, download, or save fails
///
/// # Filename Format
/// Images are saved as `bing-{market}-YYYY-MM-DD.jpg` where the date is the
/// local system date at download time.
pub async fn download_image(image: &BingImage, wallpaper_dir: &str, market: &str) -> Result<String, String> {
    // Create wallpaper directory if needed
    let dir = Path::new(wallpaper_dir);
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Failed to create wallpaper directory: {}", e))?;

    // Generate filename based on market and local date
    let date = Local::now().format("%Y-%m-%d");
    let filename = format!("bing-{}-{}.jpg", market, date);
    let filepath = dir.join(&filename);
    let filepath_str = filepath.to_string_lossy().to_string();

    // Skip download if already exists (idempotent operation)
    if filepath.exists() {
        return Ok(filepath_str);
    }

    // Download the image bytes
    let response = reqwest::get(&image.url)
        .await
        .map_err(|e| format!("Failed to download image: {}", e))?;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read image data: {}", e))?;

    // Save to disk
    std::fs::write(&filepath, bytes)
        .map_err(|e| format!("Failed to save image: {}", e))?;

    Ok(filepath_str)
}

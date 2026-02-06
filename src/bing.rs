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
use std::time::Duration;

/// Base URL for the Bing Homepage Image Archive API.
const BING_API_URL: &str = "https://www.bing.com/HPImageArchive.aspx";

/// HTTP request timeout in seconds
const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Creates an HTTP client with appropriate timeout settings.
fn create_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))
}

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
    /// Feature date (format: YYYYMMDD) - used in download_image for filename
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

    let client = create_client()?;
    let response = client.get(&url)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "Request timed out - check your internet connection".to_string()
            } else {
                format!("Failed to fetch Bing API: {e}")
            }
        })?;

    let api_response: BingApiResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Bing response: {e}"))?;

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
/// Images are saved as `bing-{market}-YYYY-MM-DD.jpg` where the date is from
/// Bing's API response (the date the image was featured), not the local system date.
pub async fn download_image(image: &BingImage, wallpaper_dir: &str, market: &str) -> Result<String, String> {
    // Create wallpaper directory if needed
    let dir = Path::new(wallpaper_dir);
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Failed to create wallpaper directory: {e}"))?;

    // Generate filename based on market and Bing's image date (not local date)
    // Bing returns date as YYYYMMDD, convert to YYYY-MM-DD for filename
    let date = if image.date.len() == 8 {
        format!("{}-{}-{}", &image.date[0..4], &image.date[4..6], &image.date[6..8])
    } else {
        // Fallback to raw date if format is unexpected
        image.date.clone()
    };
    let filename = format!("bing-{}-{}.jpg", market, date);
    let filepath = dir.join(&filename);
    let filepath_str = filepath.to_string_lossy().to_string();

    // Skip download if already exists (idempotent operation)
    if filepath.exists() {
        return Ok(filepath_str);
    }

    // Download the image bytes with timeout
    let client = create_client()?;
    let response = client.get(&image.url)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "Download timed out - check your internet connection".to_string()
            } else {
                format!("Failed to download image: {e}")
            }
        })?;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read image data: {e}"))?;

    // Validate that we received an actual image (check magic bytes)
    // JPEG starts with FF D8 FF, PNG starts with 89 50 4E 47
    if bytes.len() < 4 {
        return Err("Downloaded file is too small to be an image".to_string());
    }
    let is_jpeg = bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF;
    let is_png = bytes[0] == 0x89 && bytes[1] == 0x50 && bytes[2] == 0x4E && bytes[3] == 0x47;
    if !is_jpeg && !is_png {
        return Err("Downloaded content is not a valid image (may be an error page)".to_string());
    }

    // Save to disk
    std::fs::write(&filepath, bytes)
        .map_err(|e| format!("Failed to save image: {e}"))?;

    Ok(filepath_str)
}

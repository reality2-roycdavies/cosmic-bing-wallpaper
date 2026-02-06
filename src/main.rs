//! # Cosmic Bing Wallpaper
//!
//! A COSMIC desktop panel applet that fetches and sets Microsoft Bing's daily
//! wallpaper images. Built using the libcosmic toolkit.
//!
//! ## Features
//! - Fetches today's Bing image from multiple regional markets
//! - Previews images before applying as wallpaper
//! - Maintains a history of downloaded wallpapers
//! - Internal timer for automatic daily updates
//! - Native COSMIC panel applet integration
//! - D-Bus service for IPC between applet and settings window
//!
//! ## Architecture
//! The applet process runs in the COSMIC panel and owns the wallpaper service:
//!
//! - `applet.rs` - COSMIC panel applet with popup controls
//! - `settings.rs` - Full settings window (launched via --settings)
//! - `service.rs` - Wallpaper service (embedded in applet, exposes D-Bus interface)
//! - `timer.rs` - Internal timer for scheduled fetches
//! - `dbus_client.rs` - Client proxy for settings window to communicate with applet
//! - `bing.rs` - Bing API client for fetching image metadata and downloading
//! - `config.rs` - User configuration and regional market definitions
//!
//! ## CLI Usage
//! - No arguments: Run as COSMIC panel applet
//! - `--settings`, `-s`: Open the settings window
//! - `--fetch`, `-f`: CLI fetch and apply (one-shot)
//! - `--help`, `-h`: Show help message
//!
//! ## Created with Claude
//! This project was created collaboratively with Claude (Anthropic's AI assistant)
//! using Claude Code as a demonstration of AI-assisted software development.

// --- Module declarations ---
// Each `mod` statement tells Rust to include the corresponding .rs file as part of this crate.

mod applet;      // COSMIC panel applet (lives in the panel bar, shows popup on click)
mod config;      // User configuration and Bing market definitions
mod bing;        // Bing API client (fetches image metadata and downloads images)
mod settings;    // Full settings window (launched via --settings)
mod service;     // D-Bus service + wallpaper apply logic (embedded in the applet)
mod timer;       // Internal daily timer for automatic wallpaper updates
mod dbus_client; // D-Bus client proxy (used by settings window to talk to the applet)

/// Application entry point — dispatches to the appropriate mode based on CLI arguments.
///
/// The same binary serves three purposes:
/// 1. **Panel applet** (no args): Runs as a COSMIC panel applet with popup controls
/// 2. **Settings window** (`--settings`): Opens a full GUI for configuration
/// 3. **CLI fetch** (`--fetch`): Headless one-shot fetch and apply (for scripts/cron)
///
/// The panel applet is the primary mode — COSMIC launches it automatically when
/// the user adds it to their panel. The settings window is a separate process
/// that communicates with the applet via D-Bus.
fn main() -> cosmic::iced::Result {
    // Collect command-line arguments into a vector
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        // Dispatch based on the first argument
        match args[1].as_str() {
            "--settings" | "-s" => {
                // Launch the full settings/management window
                settings::run_settings()
            }
            "--fetch-and-apply" | "--fetch" | "-f" => {
                // Headless mode: fetch today's wallpaper and apply it, then exit
                run_headless()
            }
            "--help" | "-h" => {
                print_help(&args[0]);
                Ok(())
            }
            "--version" | "-v" => {
                // env!("CARGO_PKG_VERSION") is set at compile time from Cargo.toml
                println!("cosmic-bing-wallpaper {}", env!("CARGO_PKG_VERSION"));
                Ok(())
            }
            _ => {
                eprintln!("Unknown argument: {}", args[1]);
                eprintln!("Use --help for usage information");
                std::process::exit(1);
            }
        }
    } else {
        // No arguments: run as COSMIC panel applet (the default/primary mode)
        applet::run_applet()
    }
}

/// Prints help message
fn print_help(program: &str) {
    println!("Bing Wallpaper for COSMIC Desktop\n");
    println!("Usage: {} [OPTIONS]\n", program);
    println!("Options:");
    println!("  (none)             Run as COSMIC panel applet");
    println!("  --settings, -s     Open the settings window");
    println!("  --fetch, -f        Fetch and apply wallpaper (one-shot, no GUI)");
    println!("  --version, -v      Show version information");
    println!("  --help, -h         Show this help message");
    println!();
    println!("The applet runs in the COSMIC panel with D-Bus service and timer.");
    println!("The settings window connects to the applet via D-Bus.");
}

/// Maximum number of retry attempts for network operations in headless mode
const MAX_RETRIES: u32 = 3;

/// Initial delay between retries in seconds.
/// Uses exponential backoff: 10s, 20s, 40s (doubles each attempt).
const INITIAL_RETRY_DELAY_SECS: u64 = 10;

/// Runs the application in headless mode (no GUI).
///
/// This mode is used for command-line fetch-and-apply operations,
/// useful for scripts, cron jobs, or one-shot usage.
///
/// The function creates its own tokio runtime (since there's no COSMIC event loop)
/// and attempts to fetch and apply the wallpaper with exponential backoff retries
/// in case of network failures.
///
/// # Retry Strategy
/// - Attempt 0: immediate
/// - Attempt 1: wait 10 seconds
/// - Attempt 2: wait 20 seconds
fn run_headless() -> cosmic::iced::Result {
    use tokio::runtime::Runtime;
    use std::time::Duration;

    // Create a tokio runtime for async HTTP operations
    let rt = Runtime::new().expect("Failed to create tokio runtime");

    rt.block_on(async {
        let config = config::Config::load();

        println!("Fetching Bing image for market: {}", config.market);

        let mut last_error = String::new();
        for attempt in 0..MAX_RETRIES {
            // Wait before retrying (skip delay on first attempt)
            if attempt > 0 {
                // Exponential backoff: 10s, 20s, 40s...
                // (1 << (attempt - 1)) is a bit shift that gives us powers of 2: 1, 2, 4...
                let delay = INITIAL_RETRY_DELAY_SECS * (1 << (attempt - 1));
                println!("Retry {} of {} in {} seconds...", attempt, MAX_RETRIES - 1, delay);
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }

            // Step 1: Fetch image metadata from Bing API
            match bing::fetch_bing_image_info(&config.market).await {
                Ok(image) => {
                    println!("Found: {}", image.title);

                    // Step 2: Download the actual image file
                    match bing::download_image(&image, &config.wallpaper_dir, &config.market).await {
                        Ok(path) => {
                            println!("Downloaded to: {}", path);

                            // Step 3: Apply the wallpaper to COSMIC desktop
                            match settings::apply_wallpaper_headless(&path).await {
                                Ok(()) => {
                                    println!("Wallpaper applied successfully!");
                                    return;  // Success! Exit the retry loop
                                }
                                Err(e) => {
                                    last_error = format!("Failed to apply wallpaper: {e}");
                                    eprintln!("{}", last_error);
                                }
                            }
                        }
                        Err(e) => {
                            last_error = format!("Failed to download: {e}");
                            eprintln!("{}", last_error);
                        }
                    }
                }
                Err(e) => {
                    last_error = format!("Failed to fetch: {e}");
                    eprintln!("{}", last_error);
                }
            }
        }

        eprintln!("All {} attempts failed. Last error: {}", MAX_RETRIES, last_error);
    });

    Ok(())
}

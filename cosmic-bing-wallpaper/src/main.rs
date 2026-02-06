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

mod applet;
mod config;
mod bing;
mod settings;
mod service;
mod timer;
mod dbus_client;

/// Application entry point.
///
/// Supports three modes:
/// 1. Default: Run as COSMIC panel applet
/// 2. Settings mode (`--settings`): Open the settings/management window
/// 3. CLI mode (`--fetch`): Headless fetch and apply (one-shot)
fn main() -> cosmic::iced::Result {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        match args[1].as_str() {
            "--settings" | "-s" => {
                settings::run_settings()
            }
            "--fetch-and-apply" | "--fetch" | "-f" => {
                run_headless()
            }
            "--help" | "-h" => {
                print_help(&args[0]);
                Ok(())
            }
            "--version" | "-v" => {
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
        // Default: run as COSMIC panel applet
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

/// Maximum number of retry attempts for network operations
const MAX_RETRIES: u32 = 3;

/// Initial delay between retries (doubles each attempt)
const INITIAL_RETRY_DELAY_SECS: u64 = 10;

/// Runs the application in headless mode (no GUI).
///
/// Used for CLI fetch mode to fetch and apply the wallpaper automatically.
/// Includes retry logic with exponential backoff for network failures.
fn run_headless() -> cosmic::iced::Result {
    use tokio::runtime::Runtime;
    use std::time::Duration;

    let rt = Runtime::new().expect("Failed to create tokio runtime");

    rt.block_on(async {
        let config = config::Config::load();

        println!("Fetching Bing image for market: {}", config.market);

        let mut last_error = String::new();
        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                let delay = INITIAL_RETRY_DELAY_SECS * (1 << (attempt - 1));
                println!("Retry {} of {} in {} seconds...", attempt, MAX_RETRIES - 1, delay);
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }

            match bing::fetch_bing_image_info(&config.market).await {
                Ok(image) => {
                    println!("Found: {}", image.title);

                    match bing::download_image(&image, &config.wallpaper_dir, &config.market).await {
                        Ok(path) => {
                            println!("Downloaded to: {}", path);

                            match settings::apply_wallpaper_headless(&path).await {
                                Ok(()) => {
                                    println!("Wallpaper applied successfully!");
                                    return;
                                }
                                Err(e) => {
                                    last_error = format!("Failed to apply wallpaper: {}", e);
                                    eprintln!("{}", last_error);
                                }
                            }
                        }
                        Err(e) => {
                            last_error = format!("Failed to download: {}", e);
                            eprintln!("{}", last_error);
                        }
                    }
                }
                Err(e) => {
                    last_error = format!("Failed to fetch: {}", e);
                    eprintln!("{}", last_error);
                }
            }
        }

        eprintln!("All {} attempts failed. Last error: {}", MAX_RETRIES, last_error);
    });

    Ok(())
}

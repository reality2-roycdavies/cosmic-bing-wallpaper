//! # Cosmic Bing Wallpaper
//!
//! A COSMIC desktop application that fetches and sets Microsoft Bing's daily
//! wallpaper images. Built using the libcosmic toolkit (based on iced).
//!
//! ## Features
//! - Fetches today's Bing image from multiple regional markets
//! - Previews images before applying as wallpaper
//! - Maintains a history of downloaded wallpapers
//! - Integrates with systemd user timers for automatic daily updates
//! - System tray icon for background operation
//!
//! ## Architecture
//! The application follows the Model-View-Update (MVU) pattern used by iced/libcosmic:
//! - `app.rs` - Main application state, UI views, and message handling
//! - `bing.rs` - Bing API client for fetching image metadata and downloading
//! - `config.rs` - User configuration and regional market definitions
//! - `tray.rs` - System tray icon and menu
//!
//! ## CLI Usage
//! - No arguments: Launch the GUI application
//! - `--tray`: Run in system tray only (background mode)
//! - `--fetch-and-apply`: Fetch today's image and apply as wallpaper (for systemd timer)
//! - `--help`: Show help message
//!
//! ## Created with Claude
//! This project was created collaboratively with Claude (Anthropic's AI assistant)
//! using Claude Code as a demonstration of AI-assisted software development.

mod app;
mod config;
mod bing;
mod tray;

use app::BingWallpaper;
use cosmic::iced::Size;

/// Application entry point.
///
/// Supports three modes:
/// 1. GUI mode (default): Launches the COSMIC application window
/// 2. Tray mode (`--tray`): Runs in system tray for background operation
/// 3. CLI mode (`--fetch-and-apply`): Headless fetch and apply for systemd timer
fn main() -> cosmic::iced::Result {
    let args: Vec<String> = std::env::args().collect();

    // Check for CLI arguments
    if args.len() > 1 {
        match args[1].as_str() {
            "--tray" | "-t" => {
                // Run in system tray mode (background)
                println!("Starting Bing Wallpaper in system tray...");
                if let Err(e) = tray::run_tray() {
                    eprintln!("Tray error: {}", e);
                    std::process::exit(1);
                }
                return Ok(());
            }
            "--fetch-and-apply" | "-f" => {
                // Run in headless mode for systemd timer
                return run_headless();
            }
            "--help" | "-h" => {
                print_help(&args[0]);
                return Ok(());
            }
            _ => {
                eprintln!("Unknown argument: {}", args[1]);
                eprintln!("Use --help for usage information");
                std::process::exit(1);
            }
        }
    }

    // Default: Launch GUI
    let settings = cosmic::app::Settings::default()
        .size(Size::new(850.0, 750.0))
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(600.0)
                .min_height(550.0)
        );

    cosmic::app::run::<BingWallpaper>(settings, ())
}

/// Prints help message
fn print_help(program: &str) {
    println!("Bing Wallpaper for COSMIC Desktop\n");
    println!("Usage: {} [OPTIONS]\n", program);
    println!("Options:");
    println!("  (none)             Launch the GUI application");
    println!("  --tray, -t         Run in system tray (background mode)");
    println!("  --fetch-and-apply  Fetch today's image and apply as wallpaper");
    println!("  --help, -h         Show this help message");
    println!();
    println!("The system tray mode runs in the background and provides quick");
    println!("access to wallpaper functions via right-click menu.");
}

/// Runs the application in headless mode (no GUI).
///
/// Used by the systemd timer to fetch and apply the wallpaper automatically.
fn run_headless() -> cosmic::iced::Result {
    use tokio::runtime::Runtime;

    let rt = Runtime::new().expect("Failed to create tokio runtime");

    rt.block_on(async {
        let config = config::Config::load();

        println!("Fetching Bing image for market: {}", config.market);

        // Fetch image info
        match bing::fetch_bing_image_info(&config.market).await {
            Ok(image) => {
                println!("Found: {}", image.title);

                // Download image
                match bing::download_image(&image, &config.wallpaper_dir, &config.market).await {
                    Ok(path) => {
                        println!("Downloaded to: {}", path);

                        // Apply wallpaper
                        match app::apply_wallpaper_headless(&path).await {
                            Ok(()) => println!("Wallpaper applied successfully!"),
                            Err(e) => eprintln!("Failed to apply wallpaper: {}", e),
                        }
                    }
                    Err(e) => eprintln!("Failed to download: {}", e),
                }
            }
            Err(e) => eprintln!("Failed to fetch: {}", e),
        }
    });

    Ok(())
}

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
//! - D-Bus daemon for IPC between GUI and tray components
//!
//! ## Architecture
//! The application uses a daemon+clients architecture with D-Bus for IPC:
//!
//! - `daemon.rs` - Background D-Bus service providing core wallpaper functionality
//! - `dbus_client.rs` - Client proxy for communicating with the daemon
//! - `app.rs` - GUI application using libcosmic (MVU pattern)
//! - `tray.rs` - System tray icon (uses D-Bus when daemon is available)
//! - `bing.rs` - Bing API client for fetching image metadata and downloading
//! - `config.rs` - User configuration and regional market definitions
//!
//! ## CLI Usage
//! - No arguments: Launch the GUI application
//! - `--tray`: Run in system tray only (background mode)
//! - `--daemon`: Run as D-Bus daemon (background service)
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
mod daemon;
mod dbus_client;

use app::BingWallpaper;
use cosmic::iced::Size;
use std::fs;
use std::io::{Read, Write};
use std::process::Command;

/// Get the path to the tray lockfile
/// Uses config directory to work correctly in Flatpak sandboxes
fn tray_lockfile_path() -> std::path::PathBuf {
    dirs::config_dir()
        .map(|d| d.join("cosmic-bing-wallpaper"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("tray.lock")
}

/// Check if the tray is already running using a lockfile
/// In Flatpak, we can't check /proc/PID due to PID namespace isolation,
/// so we just check if the lockfile exists (with a timestamp check for stale files)
fn is_tray_running() -> bool {
    let lockfile = tray_lockfile_path();

    if let Ok(metadata) = fs::metadata(&lockfile) {
        // Check if lockfile is recent (less than 1 minute old means tray is likely running)
        if let Ok(modified) = metadata.modified() {
            if let Ok(elapsed) = modified.elapsed() {
                // If lockfile was modified less than 60 seconds ago, tray is running
                return elapsed.as_secs() < 60;
            }
        }
        // If we can't check time, assume running if file exists
        return true;
    }
    false
}

/// Create a lockfile to indicate the tray is running
pub fn create_tray_lockfile() {
    let lockfile = tray_lockfile_path();
    if let Some(parent) = lockfile.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = fs::File::create(&lockfile) {
        let _ = write!(file, "{}", std::process::id());
    }
}

/// Remove the lockfile when tray exits
pub fn remove_tray_lockfile() {
    let _ = fs::remove_file(tray_lockfile_path());
}

/// Application entry point.
///
/// Supports four modes:
/// 1. GUI mode (default): Launches the COSMIC application window
/// 2. Tray mode (`--tray`): Runs in system tray for background operation
/// 3. Daemon mode (`--daemon`): Runs as D-Bus service for IPC
/// 4. CLI mode (`--fetch-and-apply`): Headless fetch and apply for systemd timer
fn main() -> cosmic::iced::Result {
    let args: Vec<String> = std::env::args().collect();

    // Check for CLI arguments
    if args.len() > 1 {
        match args[1].as_str() {
            "--tray" | "-t" => {
                // Check if tray is already running
                if is_tray_running() {
                    println!("Bing Wallpaper tray is already running.");
                    return Ok(());
                }
                // Run in system tray mode (background)
                println!("Starting Bing Wallpaper in system tray...");
                create_tray_lockfile();
                let result = tray::run_tray();
                remove_tray_lockfile();
                if let Err(e) = result {
                    eprintln!("Tray error: {}", e);
                    std::process::exit(1);
                }
                return Ok(());
            }
            "--daemon" | "-d" => {
                // Run as D-Bus daemon
                println!("Starting Bing Wallpaper daemon...");
                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                if let Err(e) = rt.block_on(daemon::run_daemon()) {
                    eprintln!("Daemon error: {}", e);
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

    // Default: Smart mode - start tray if not running, then launch GUI
    if !is_tray_running() {
        println!("Starting Bing Wallpaper tray in background...");
        if let Err(e) = Command::new(std::env::current_exe().unwrap_or_else(|_| "cosmic-bing-wallpaper".into()))
            .arg("--tray")
            .spawn()
        {
            eprintln!("Warning: Failed to start tray: {}", e);
        }
        // Give tray time to initialize
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    // Launch GUI
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
    println!("  --daemon, -d       Run as D-Bus daemon (background service)");
    println!("  --fetch-and-apply  Fetch today's image and apply as wallpaper");
    println!("  --help, -h         Show this help message");
    println!();
    println!("The system tray mode runs in the background and provides quick");
    println!("access to wallpaper functions via right-click menu.");
    println!();
    println!("The daemon mode runs a D-Bus service that manages wallpapers.");
    println!("Both the GUI and tray can connect to the daemon for synchronized state.");
}

/// Maximum number of retry attempts for network operations
const MAX_RETRIES: u32 = 3;

/// Initial delay between retries (doubles each attempt)
const INITIAL_RETRY_DELAY_SECS: u64 = 10;

/// Runs the application in headless mode (no GUI).
///
/// Used by the systemd timer to fetch and apply the wallpaper automatically.
/// Includes retry logic with exponential backoff for network failures.
fn run_headless() -> cosmic::iced::Result {
    use tokio::runtime::Runtime;
    use std::time::Duration;

    let rt = Runtime::new().expect("Failed to create tokio runtime");

    rt.block_on(async {
        let config = config::Config::load();

        println!("Fetching Bing image for market: {}", config.market);

        // Retry loop with exponential backoff
        let mut last_error = String::new();
        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                let delay = INITIAL_RETRY_DELAY_SECS * (1 << (attempt - 1)); // 10s, 20s, 40s
                println!("Retry {} of {} in {} seconds...", attempt, MAX_RETRIES - 1, delay);
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }

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
                                Ok(()) => {
                                    println!("Wallpaper applied successfully!");
                                    return; // Success - exit retry loop
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

        // All retries exhausted
        eprintln!("All {} attempts failed. Last error: {}", MAX_RETRIES, last_error);
    });

    Ok(())
}

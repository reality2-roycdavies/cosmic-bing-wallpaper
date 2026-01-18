//! # Cosmic Bing Wallpaper
//!
//! A COSMIC desktop application that fetches and sets Microsoft Bing's daily
//! wallpaper images. Built using the libcosmic toolkit (based on iced).
//!
//! ## Features
//! - Fetches today's Bing image from multiple regional markets
//! - Previews images before applying as wallpaper
//! - Maintains a history of downloaded wallpapers
//! - Internal timer for automatic daily updates (Flatpak-friendly)
//! - System tray icon for background operation
//! - D-Bus service for IPC between GUI and tray components
//!
//! ## Architecture (Flatpak-friendly)
//! The tray process owns the wallpaper service and timer:
//!
//! - `service.rs` - Wallpaper service (embedded in tray, exposes D-Bus interface)
//! - `timer.rs` - Internal timer (replaces systemd timer)
//! - `dbus_client.rs` - Client proxy for GUI to communicate with tray
//! - `app.rs` - GUI application using libcosmic (MVU pattern)
//! - `tray.rs` - System tray icon with embedded service
//! - `bing.rs` - Bing API client for fetching image metadata and downloading
//! - `config.rs` - User configuration and regional market definitions
//!
//! ## CLI Usage
//! - No arguments: Start tray + open GUI
//! - `--tray`: Run in system tray only (for autostart)
//! - `--fetch`: CLI fetch and apply (one-shot, no tray)
//! - `--help`: Show help message
//!
//! ## Created with Claude
//! This project was created collaboratively with Claude (Anthropic's AI assistant)
//! using Claude Code as a demonstration of AI-assisted software development.

mod app;
mod config;
mod bing;
mod tray;
mod service;
mod timer;
mod dbus_client;

use app::BingWallpaper;
use cosmic::iced::Size;
use std::fs;
use std::io::Write;
use std::process::Command;

/// Get the app config directory path
/// In Flatpak, we use the exposed host config directory rather than XDG_CONFIG_HOME
/// because we have --filesystem=~/.config/cosmic-bing-wallpaper:create permission
fn app_config_dir() -> std::path::PathBuf {
    if service::is_flatpak() {
        // In Flatpak, use the exposed host config directory
        dirs::home_dir()
            .map(|h| h.join(".config/cosmic-bing-wallpaper"))
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp/cosmic-bing-wallpaper"))
    } else {
        // Native: use standard XDG config directory
        dirs::config_dir()
            .map(|d| d.join("cosmic-bing-wallpaper"))
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp/cosmic-bing-wallpaper"))
    }
}

/// Get the path to the tray lockfile
fn tray_lockfile_path() -> std::path::PathBuf {
    app_config_dir().join("tray.lock")
}

/// Get the path to the GUI lockfile
fn gui_lockfile_path() -> std::path::PathBuf {
    app_config_dir().join("gui.lock")
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
        // If we can't check time, assume NOT running (conservative approach)
        // This prevents stale lockfiles from blocking new instances after quit/restart
        return false;
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

/// Check if the GUI is already running
fn is_gui_running() -> bool {
    let lockfile = gui_lockfile_path();

    if let Ok(metadata) = fs::metadata(&lockfile) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(elapsed) = modified.elapsed() {
                // Only consider running if lockfile was touched in last 60 seconds
                return elapsed.as_secs() < 60;
            }
        }
        // If we can't check time, assume NOT running (conservative approach)
        // This prevents stale lockfiles from blocking new instances after logout/login
        return false;
    }
    false
}

/// Clean up stale lockfiles from previous sessions
/// Called at startup to prevent orphaned lockfiles from blocking new instances
pub fn cleanup_stale_lockfiles() {
    // Clean up stale GUI lockfile
    let gui_lockfile = gui_lockfile_path();
    cleanup_single_lockfile(&gui_lockfile, "GUI");

    // Clean up stale tray lockfile
    let tray_lockfile = tray_lockfile_path();
    cleanup_single_lockfile(&tray_lockfile, "tray");
}

/// Helper to clean up a single stale lockfile
fn cleanup_single_lockfile(lockfile: &std::path::Path, name: &str) {
    if let Ok(metadata) = fs::metadata(lockfile) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(elapsed) = modified.elapsed() {
                // If lockfile is older than 60 seconds, it's from a dead process
                if elapsed.as_secs() >= 60 {
                    let _ = fs::remove_file(lockfile);
                    eprintln!("Cleaned up stale {} lockfile", name);
                }
            }
        } else {
            // Can't check modification time - remove it to be safe
            let _ = fs::remove_file(lockfile);
            eprintln!("Removed {} lockfile with unreadable metadata", name);
        }
    }
}

/// Create a lockfile to indicate the GUI is running
pub fn create_gui_lockfile() {
    let lockfile = gui_lockfile_path();
    if let Some(parent) = lockfile.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = fs::File::create(&lockfile) {
        let _ = write!(file, "{}", std::process::id());
    }
}

/// Remove the GUI lockfile when app exits
pub fn remove_gui_lockfile() {
    let _ = fs::remove_file(gui_lockfile_path());
}

/// Ensure autostart entry exists for the tray
/// Creates an XDG autostart desktop file so the tray starts on login
fn ensure_autostart() {
    let autostart_dir = if service::is_flatpak() {
        // In Flatpak, write to the host's autostart directory
        dirs::home_dir().map(|h| h.join(".config/autostart"))
    } else {
        dirs::config_dir().map(|d| d.join("autostart"))
    };

    let Some(autostart_dir) = autostart_dir else {
        return;
    };

    let desktop_file = autostart_dir.join("io.github.reality2_roycdavies.cosmic-bing-wallpaper.desktop");

    // Only create if it doesn't exist (don't overwrite user modifications)
    if desktop_file.exists() {
        return;
    }

    let _ = fs::create_dir_all(&autostart_dir);

    let exec_cmd = if service::is_flatpak() {
        "flatpak run io.github.reality2_roycdavies.cosmic-bing-wallpaper --tray"
    } else {
        "cosmic-bing-wallpaper --tray"
    };

    let content = format!(
        r#"[Desktop Entry]
Type=Application
Name=Bing Wallpaper
Comment=Bing Daily Wallpaper system tray
Exec={exec_cmd}
Icon=io.github.reality2_roycdavies.cosmic-bing-wallpaper
Terminal=false
Categories=Utility;
X-GNOME-Autostart-enabled=true
"#
    );

    let _ = fs::write(&desktop_file, content);
}

/// Application entry point.
///
/// Supports three modes:
/// 1. Default: Start tray (if not running) + open GUI
/// 2. Tray mode (`--tray`): Run in system tray only (for autostart)
/// 3. CLI mode (`--fetch`): Headless fetch and apply (one-shot)
fn main() -> cosmic::iced::Result {
    let args: Vec<String> = std::env::args().collect();

    // Check for CLI arguments
    if args.len() > 1 {
        match args[1].as_str() {
            "--tray" | "-t" => {
                // Clean up any stale lockfiles from previous sessions
                cleanup_stale_lockfiles();

                // Check if tray is already running
                if is_tray_running() {
                    println!("Bing Wallpaper tray is already running.");
                    return Ok(());
                }
                // Run in system tray mode (background) - this includes D-Bus service
                println!("Starting Bing Wallpaper tray with D-Bus service...");
                ensure_autostart();
                create_tray_lockfile();
                let result = tray::run_tray();
                remove_tray_lockfile();
                if let Err(e) = result {
                    eprintln!("Tray error: {}", e);
                    std::process::exit(1);
                }
                return Ok(());
            }
            "--fetch-and-apply" | "--fetch" | "-f" => {
                // Run in headless mode (one-shot fetch and apply)
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
    // Clean up any stale lockfiles from previous sessions first
    cleanup_stale_lockfiles();

    if is_gui_running() {
        println!("Bing Wallpaper is already open.");
        return Ok(());
    }

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

    // Launch GUI with lockfile management
    create_gui_lockfile();
    let settings = cosmic::app::Settings::default()
        .size(Size::new(850.0, 750.0))
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(600.0)
                .min_height(550.0)
        );

    let result = cosmic::app::run::<BingWallpaper>(settings, ());
    remove_gui_lockfile();
    result
}

/// Prints help message
fn print_help(program: &str) {
    println!("Bing Wallpaper for COSMIC Desktop\n");
    println!("Usage: {} [OPTIONS]\n", program);
    println!("Options:");
    println!("  (none)             Start tray (if needed) + open GUI");
    println!("  --tray, -t         Run in system tray only (for autostart)");
    println!("  --fetch, -f        Fetch and apply wallpaper (one-shot, no GUI)");
    println!("  --help, -h         Show this help message");
    println!();
    println!("The tray process runs the D-Bus service and manages the internal timer.");
    println!("The GUI connects to the tray via D-Bus for wallpaper operations.");
    println!();
    println!("For autostart, add the --tray argument to your session startup.");
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

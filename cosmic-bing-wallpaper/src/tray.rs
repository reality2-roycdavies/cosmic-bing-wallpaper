//! System Tray Module
//!
//! Implements a system tray icon using the StatusNotifierItem (SNI) protocol
//! via the ksni crate. This allows the app to run in the background and
//! provide quick access to wallpaper functionality.
//!
//! ## Features
//! - Persistent tray icon in the system status area
//! - Right-click menu with common actions
//! - Daily update timer management
//! - Runs independently of the main GUI window

use ksni::{Tray, TrayService};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Check if the daily timer is enabled
fn is_timer_enabled() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-enabled", "cosmic-bing-wallpaper.timer"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Toggle the daily update timer (waits for completion)
fn toggle_timer(enable: bool) {
    let action = if enable { "enable" } else { "disable" };
    let _ = Command::new("systemctl")
        .args(["--user", action, "--now", "cosmic-bing-wallpaper.timer"])
        .status(); // Wait for completion so menu reflects new state
}

/// The system tray implementation
#[derive(Debug)]
pub struct BingWallpaperTray {
    /// Flag to signal when the tray should exit
    should_quit: Arc<AtomicBool>,
}

impl BingWallpaperTray {
    pub fn new(should_quit: Arc<AtomicBool>) -> Self {
        Self { should_quit }
    }
}

impl Tray for BingWallpaperTray {
    fn id(&self) -> String {
        "io.github.cosmic-bing-wallpaper".to_string()
    }

    fn title(&self) -> String {
        "Bing Wallpaper".to_string()
    }

    fn icon_name(&self) -> String {
        // Use the symbolic icon for system tray (smaller, simpler)
        "io.github.cosmic-bing-wallpaper-symbolic".to_string()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        let timer_enabled = is_timer_enabled();
        let timer_label = if timer_enabled {
            "Daily Update: On ✓"
        } else {
            "Daily Update: Off"
        };

        vec![
            StandardItem {
                label: "Fetch Today's Wallpaper".to_string(),
                icon_name: "emblem-downloads".to_string(),
                activate: Box::new(|_| {
                    // Run fetch-and-apply in background
                    std::thread::spawn(|| {
                        let exe = std::env::current_exe().unwrap_or_default();
                        let _ = Command::new(exe)
                            .arg("--fetch-and-apply")
                            .spawn();
                    });
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: timer_label.to_string(),
                icon_name: if timer_enabled { "appointment-recurring" } else { "appointment-missed" }.to_string(),
                activate: Box::new(move |_| {
                    toggle_timer(!timer_enabled);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Open Application".to_string(),
                icon_name: "preferences-desktop-wallpaper".to_string(),
                activate: Box::new(|_| {
                    std::thread::spawn(|| {
                        let exe = std::env::current_exe().unwrap_or_default();
                        let _ = Command::new(exe).spawn();
                    });
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Open Wallpaper Folder".to_string(),
                icon_name: "folder-pictures".to_string(),
                activate: Box::new(|_| {
                    if let Some(home) = dirs::home_dir() {
                        let wallpaper_dir = home.join("Pictures/BingWallpapers");
                        let _ = open::that(wallpaper_dir);
                    }
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".to_string(),
                icon_name: "application-exit".to_string(),
                activate: Box::new(|tray: &mut Self| {
                    tray.should_quit.store(true, Ordering::SeqCst);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Starts the system tray service.
///
/// This function blocks and runs the tray event loop. It should be called
/// from a dedicated thread or as the main entry point for tray-only mode.
///
/// Returns when the user selects "Quit" from the tray menu.
pub fn run_tray() -> Result<(), String> {
    let should_quit = Arc::new(AtomicBool::new(false));
    let tray = BingWallpaperTray::new(should_quit.clone());

    let service = TrayService::new(tray);
    let handle = service.handle();

    // Spawn the tray service
    service.spawn();

    // Wait for quit signal
    while !should_quit.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Shutdown the tray
    handle.shutdown();

    Ok(())
}


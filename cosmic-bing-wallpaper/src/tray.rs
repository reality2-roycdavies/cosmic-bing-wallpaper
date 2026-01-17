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
//!
//! ## D-Bus Integration
//! When the wallpaper daemon is running, the tray uses D-Bus for operations,
//! enabling instant synchronization with the GUI and other clients.
//! Falls back to direct systemctl calls when daemon is not available.

use ksni::{Tray, TrayService};
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::Duration;

use crate::config::Config;
use crate::dbus_client::WallpaperClient;

/// Check if running inside a Flatpak sandbox
fn is_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// Run systemctl command, using flatpak-spawn when in Flatpak sandbox
fn run_systemctl(args: &[&str]) -> std::io::Result<std::process::Output> {
    if is_flatpak() {
        let mut spawn_args = vec!["--host", "systemctl"];
        spawn_args.extend(args);
        Command::new("flatpak-spawn")
            .args(&spawn_args)
            .output()
    } else {
        Command::new("systemctl")
            .args(args)
            .output()
    }
}

/// Get the path to COSMIC's theme config file
fn cosmic_theme_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("cosmic/com.system76.CosmicTheme.Mode/v1/is_dark"))
}

/// Detect if the system is in dark mode
fn is_dark_mode() -> bool {
    // Try COSMIC's config file first
    if let Some(path) = cosmic_theme_path() {
        if let Ok(content) = fs::read_to_string(&path) {
            return content.trim() == "true";
        }
    }

    // Fall back to freedesktop portal via gdbus
    if let Ok(output) = Command::new("gdbus")
        .args([
            "call", "--session",
            "--dest", "org.freedesktop.portal.Desktop",
            "--object-path", "/org/freedesktop/portal/desktop",
            "--method", "org.freedesktop.portal.Settings.Read",
            "org.freedesktop.appearance", "color-scheme"
        ])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Returns 1 for dark, 2 for light, 0 for no preference
        if stdout.contains("uint32 1") {
            return true;
        } else if stdout.contains("uint32 2") {
            return false;
        }
    }

    // Default to dark mode (most common for tray panels)
    true
}

/// Check if the daily timer is enabled (direct systemctl check)
fn is_timer_enabled_direct() -> bool {
    run_systemctl(&["--user", "is-enabled", "cosmic-bing-wallpaper.timer"])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if the timer is enabled, preferring D-Bus if available
fn is_timer_enabled() -> bool {
    // Try D-Bus first for instant sync
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();

    if let Ok(rt) = rt {
        if let Ok(enabled) = rt.block_on(async {
            if let Ok(client) = WallpaperClient::connect().await {
                client.get_timer_enabled().await
            } else {
                Err(zbus::Error::Failure("Not connected".into()))
            }
        }) {
            return enabled;
        }
    }

    // Fall back to direct check
    is_timer_enabled_direct()
}

/// Toggle the daily update timer (waits for completion)
fn toggle_timer(enable: bool) {
    // Try D-Bus first
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();

    if let Ok(rt) = rt {
        let result = rt.block_on(async {
            if let Ok(client) = WallpaperClient::connect().await {
                client.set_timer_enabled(enable).await
            } else {
                Err(zbus::Error::Failure("Not connected".into()))
            }
        });

        if result.is_ok() {
            return;
        }
    }

    // Fall back to direct systemctl
    let action = if enable { "enable" } else { "disable" };
    let _ = run_systemctl(&["--user", action, "--now", "cosmic-bing-wallpaper.timer"]);
}

/// Fetch and apply wallpaper, preferring D-Bus if available
fn fetch_and_apply() {
    // Try D-Bus first
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();

    if let Ok(rt) = rt {
        let result = rt.block_on(async {
            if let Ok(client) = WallpaperClient::connect().await {
                client.fetch_wallpaper(true).await.map(|_| ())
            } else {
                Err(zbus::Error::Failure("Not connected".into()))
            }
        });

        if result.is_ok() {
            // Send success notification
            let _ = Command::new("notify-send")
                .args(["-i", "preferences-desktop-wallpaper",
                       "Bing Wallpaper", "Today's wallpaper has been applied!"])
                .spawn();
            return;
        }
    }

    // Fall back to spawning the binary
    let exe = std::env::current_exe().unwrap_or_default();
    match Command::new(&exe)
        .arg("--fetch-and-apply")
        .status()
    {
        Ok(status) if status.success() => {
            let _ = Command::new("notify-send")
                .args(["-i", "preferences-desktop-wallpaper",
                       "Bing Wallpaper", "Today's wallpaper has been applied!"])
                .spawn();
        }
        Ok(_) => {
            let _ = Command::new("notify-send")
                .args(["-u", "critical", "-i", "dialog-error",
                       "Bing Wallpaper", "Failed to fetch or apply wallpaper"])
                .spawn();
        }
        Err(e) => {
            eprintln!("Failed to run fetch: {}", e);
        }
    }
}

/// The system tray implementation
#[derive(Debug)]
pub struct BingWallpaperTray {
    /// Flag to signal when the tray should exit
    should_quit: Arc<AtomicBool>,
    /// Channel to signal menu updates needed.
    /// None = refresh from system, Some(val) = set explicit state
    update_tx: Sender<Option<bool>>,
    /// Cached timer enabled state (refreshed on menu rebuild)
    timer_enabled: bool,
    /// Cached dark mode state for theme-aware icons
    dark_mode: bool,
}

impl BingWallpaperTray {
    pub fn new(should_quit: Arc<AtomicBool>, update_tx: Sender<Option<bool>>) -> Self {
        Self {
            should_quit,
            update_tx,
            timer_enabled: is_timer_enabled(),
            dark_mode: is_dark_mode(),
        }
    }
}

impl Tray for BingWallpaperTray {
    fn id(&self) -> String {
        "io.github.cosmic-bing-wallpaper".to_string()
    }

    fn icon_theme_path(&self) -> String {
        // SNI hosts wait valid search paths (base dir containing hicolor)
        dirs::data_dir()
            .map(|p| p.join("icons").to_string_lossy().to_string())
            .unwrap_or_default()
    }

    fn icon_name(&self) -> String {
        // Fallback or empty if pixmap is used
        "".to_string()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        // Provide multiple icon sizes - tray host picks the best one
        // Small sizes have larger/bolder indicators for visibility
        // Use 24px icons designed with bold indicators - COSMIC scales as needed
        let icon_data: &[u8] = match (self.timer_enabled, self.dark_mode) {
            (true, true) => include_bytes!("../resources/icon-on-light-24.png"),
            (true, false) => include_bytes!("../resources/icon-on-24.png"),
            (false, true) => include_bytes!("../resources/icon-off-light-24.png"),
            (false, false) => include_bytes!("../resources/icon-off-24.png"),
        };

        let img = match image::load_from_memory(icon_data) {
            Ok(img) => img.to_rgba8(),
            Err(_) => return vec![],
        };

        // Convert RGBA to ARGB (network byte order) which KSNI/DBus expects
        let mut argb_data = Vec::with_capacity((img.width() * img.height() * 4) as usize);
        for pixel in img.pixels() {
            let [r, g, b, a] = pixel.0;
            argb_data.push(a);
            argb_data.push(r);
            argb_data.push(g);
            argb_data.push(b);
        }

        vec![ksni::Icon {
            width: img.width() as i32,
            height: img.height() as i32,
            data: argb_data,
        }]
    }

    fn title(&self) -> String {
        // Include state in title for accessibility
        if self.timer_enabled {
            "Bing Wallpaper (Daily Update ON)".to_string()
        } else {
            "Bing Wallpaper (Daily Update OFF)".to_string()
        }
    }

    fn status(&self) -> ksni::Status {
        // Active when timer enabled, Passive when disabled (some trays grey out Passive)
        if self.timer_enabled {
            ksni::Status::Active
        } else {
            ksni::Status::Passive
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let status = if self.timer_enabled {
            "Daily updates enabled"
        } else {
            "Daily updates disabled"
        };
        ksni::ToolTip {
            title: "Bing Wallpaper".to_string(),
            description: status.to_string(),
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        vec![
            StandardItem {
                label: "Fetch Today's Wallpaper".to_string(),
                icon_name: "emblem-downloads".to_string(),
                activate: Box::new(|_| {
                    std::thread::spawn(|| {
                        fetch_and_apply();
                    });
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            // Timer toggle - icon updates dynamically via ksni
            StandardItem {
                label: "Toggle Daily Update".to_string(),
                icon_name: if self.timer_enabled {
                    "appointment-recurring".to_string()
                } else {
                    "appointment-missed".to_string()
                },
                activate: Box::new(|tray: &mut Self| {
                    let new_state = !tray.timer_enabled;
                    // We update local state immediately so if the menu is reopened quickly it looks right
                    tray.timer_enabled = new_state; 
                    let tx = tray.update_tx.clone();

                    std::thread::spawn(move || {
                        // Toggle the timer - icon updates automatically via ksni
                        toggle_timer(new_state);
                        // Signal the main loop to set the state explicitly (avoiding race conditions)
                        let _ = tx.send(Some(new_state));
                    });
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
                    // Try D-Bus first to get the path
                    let wallpaper_dir = {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build();

                        if let Ok(rt) = rt {
                            if let Ok(dir) = rt.block_on(async {
                                if let Ok(client) = WallpaperClient::connect().await {
                                    client.get_wallpaper_dir().await
                                } else {
                                    Err(zbus::Error::Failure("Not connected".into()))
                                }
                            }) {
                                Some(dir)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };

                    // Fall back to loading config directly
                    let dir = wallpaper_dir.unwrap_or_else(|| {
                        let config = Config::load();
                        config.wallpaper_dir
                    });

                    let _ = open::that(&dir);
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
    let (update_tx, update_rx): (Sender<Option<bool>>, _) = channel();
    let tray = BingWallpaperTray::new(should_quit.clone(), update_tx);

    let service = TrayService::new(tray);
    let handle = service.handle();

    // Spawn the tray service
    service.spawn();

    // Set up file watcher for theme changes
    let (theme_tx, theme_rx) = channel();
    let _watcher = if let Some(theme_path) = cosmic_theme_path() {
        // Watch the parent directory since the file might be replaced atomically
        let watch_dir = theme_path.parent().map(|p| p.to_path_buf());
        if let Some(watch_dir) = watch_dir {
            let tx = theme_tx.clone();
            let config = NotifyConfig::default()
                .with_poll_interval(Duration::from_secs(1));
            let mut watcher: Result<RecommendedWatcher, _> = Watcher::new(
                move |res: Result<notify::Event, _>| {
                    if let Ok(event) = res {
                        // Only trigger on modify/create events
                        if matches!(
                            event.kind,
                            notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                        ) {
                            let _ = tx.send(());
                        }
                    }
                },
                config,
            );
            if let Ok(ref mut w) = watcher {
                let _ = w.watch(&watch_dir, RecursiveMode::NonRecursive);
            }
            watcher.ok()
        } else {
            None
        }
    } else {
        None
    };

    // Track timer state for external change detection
    let mut last_timer_enabled = is_timer_enabled();
    let mut timer_check_counter = 0u32;

    // Main loop: check for quit signal and handle update requests
    loop {
        if should_quit.load(Ordering::SeqCst) {
            break;
        }

        // Check for update requests (non-blocking)
        if let Ok(maybe_state) = update_rx.try_recv() {
            // Trigger a tray refresh
            handle.update(|tray| {
                if let Some(new_state) = maybe_state {
                    // Trust the explicit state change from our action
                    tray.timer_enabled = new_state;
                } else {
                    // Re-sync with actual state for general refreshes
                    tray.timer_enabled = is_timer_enabled();
                }
                // Always sync dark mode state
                tray.dark_mode = is_dark_mode();
            });
            last_timer_enabled = is_timer_enabled();
        }

        // Check for theme file changes (non-blocking)
        if theme_rx.try_recv().is_ok() {
            // Theme config file changed - update the icon
            handle.update(|tray| {
                tray.dark_mode = is_dark_mode();
            });
        }

        // Check for external timer state changes every ~1 second (20 iterations * 50ms)
        // This catches changes made by the GUI app or other tools
        timer_check_counter += 1;
        if timer_check_counter >= 20 {
            timer_check_counter = 0;
            let current_timer_enabled = is_timer_enabled();
            if current_timer_enabled != last_timer_enabled {
                last_timer_enabled = current_timer_enabled;
                // Timer state changed externally - update the icon
                handle.update(|tray| {
                    tray.timer_enabled = current_timer_enabled;
                });
            }
        }

        // Refresh lockfile timestamp every 30 seconds to indicate we're still running
        static LOCKFILE_REFRESH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let last_refresh = LOCKFILE_REFRESH.load(std::sync::atomic::Ordering::Relaxed);
        if now - last_refresh >= 30 {
            crate::create_tray_lockfile();
            LOCKFILE_REFRESH.store(now, std::sync::atomic::Ordering::Relaxed);
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // Shutdown the tray
    handle.shutdown();

    Ok(())
}

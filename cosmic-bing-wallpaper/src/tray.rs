//! System Tray Module
//!
//! Implements a system tray icon using the StatusNotifierItem (SNI) protocol
//! via the ksni crate. The tray process now owns the WallpaperService and
//! exposes it via D-Bus for the GUI to use.
//!
//! ## Architecture (Flatpak-friendly)
//!
//! The tray process:
//! - Runs the D-Bus service for wallpaper operations
//! - Manages the internal timer (replaces systemd timer)
//! - Shows the tray icon with status indication
//! - Provides quick access menu
//!
//! The GUI connects to this service via D-Bus.

use ksni::{Tray, TrayMethods};
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use zbus::connection;

use crate::service::{ServiceState, WallpaperService, SERVICE_NAME, OBJECT_PATH};
use crate::timer::InternalTimer;

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

/// Message types for tray updates
#[derive(Debug)]
pub enum TrayUpdate {
    /// Set timer state explicitly
    SetTimerEnabled(bool),
    /// Trigger wallpaper fetch
    FetchWallpaper,
}

/// The system tray implementation
#[derive(Debug)]
pub struct BingWallpaperTray {
    /// Flag to signal when the tray should exit
    should_quit: Arc<AtomicBool>,
    /// Channel to signal menu updates needed
    update_tx: Sender<TrayUpdate>,
    /// Cached timer enabled state
    timer_enabled: bool,
    /// Cached dark mode state for theme-aware icons
    dark_mode: bool,
    /// Reference to the shared timer for state queries
    timer: Arc<InternalTimer>,
}

impl BingWallpaperTray {
    pub fn new(
        should_quit: Arc<AtomicBool>,
        update_tx: Sender<TrayUpdate>,
        timer: Arc<InternalTimer>,
    ) -> Self {
        Self {
            should_quit,
            update_tx,
            timer_enabled: timer.is_enabled(),
            dark_mode: is_dark_mode(),
            timer,
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
        // Active when timer enabled, Passive when disabled
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
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.update_tx.send(TrayUpdate::FetchWallpaper);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            // Timer toggle
            StandardItem {
                label: "Toggle Daily Update".to_string(),
                icon_name: if self.timer_enabled {
                    "appointment-recurring".to_string()
                } else {
                    "appointment-missed".to_string()
                },
                activate: Box::new(|tray: &mut Self| {
                    let new_state = !tray.timer_enabled;
                    // Update local state immediately
                    tray.timer_enabled = new_state;
                    // Update the actual timer
                    tray.timer.set_enabled(new_state);
                    // Signal for icon refresh
                    let _ = tray.update_tx.send(TrayUpdate::SetTimerEnabled(new_state));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Settings ...".to_string(),
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

/// Starts the system tray service with embedded D-Bus service.
///
/// This function blocks and runs the tray event loop. It should be called
/// from a dedicated thread or as the main entry point for tray-only mode.
///
/// Returns when the user selects "Quit" from the tray menu.
pub fn run_tray() -> Result<(), String> {
    // Create the tokio runtime for async operations
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("Failed to create tokio runtime: {}", e))?;

    rt.block_on(async {
        run_tray_async().await
    })
}

/// Async implementation of the tray service
async fn run_tray_async() -> Result<(), String> {
    // Create the internal timer
    let timer = Arc::new(InternalTimer::new());

    // Start the timer and get the receiver for timer events
    let mut timer_rx = timer.start();

    // Create shared state with the timer
    let state = Arc::new(RwLock::new(ServiceState::new(timer.clone())));

    // Start D-Bus service
    let service = WallpaperService::new(state.clone());
    let dbus_conn = connection::Builder::session()
        .map_err(|e| format!("Failed to connect to D-Bus: {}", e))?
        .name(SERVICE_NAME)
        .map_err(|e| format!("Failed to request D-Bus name: {}", e))?
        .serve_at(OBJECT_PATH, service)
        .map_err(|e| format!("Failed to serve D-Bus interface: {}", e))?
        .build()
        .await
        .map_err(|e| format!("Failed to build D-Bus connection: {}", e))?;

    println!("D-Bus service running at {} on {}", OBJECT_PATH, SERVICE_NAME);

    // Create tray components
    let should_quit = Arc::new(AtomicBool::new(false));
    let (update_tx, update_rx) = channel();
    let tray = BingWallpaperTray::new(should_quit.clone(), update_tx.clone(), timer.clone());

    // Spawn the tray service
    // In Flatpak, disable D-Bus well-known name to avoid PID conflicts
    let is_sandboxed = std::path::Path::new("/.flatpak-info").exists();
    let handle = tray
        .disable_dbus_name(is_sandboxed)
        .spawn()
        .await
        .map_err(|e| format!("Failed to spawn tray service: {}", e))?;

    // Set up file watcher for theme changes
    let (theme_tx, theme_rx) = channel();
    let _watcher = if let Some(theme_path) = cosmic_theme_path() {
        let watch_dir = theme_path.parent().map(|p| p.to_path_buf());
        if let Some(watch_dir) = watch_dir {
            let tx = theme_tx.clone();
            let config = NotifyConfig::default()
                .with_poll_interval(Duration::from_secs(1));
            let mut watcher: Result<RecommendedWatcher, _> = Watcher::new(
                move |res: Result<notify::Event, _>| {
                    if let Ok(event) = res {
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

    // Spawn timer event handler
    let state_for_timer = state.clone();
    let timer_handle = tokio::spawn(async move {
        while let Some(()) = timer_rx.recv().await {
            // Timer fired - fetch and apply wallpaper
            println!("Timer fired - fetching wallpaper...");

            let (market, wallpaper_dir) = {
                let state = state_for_timer.read().await;
                (
                    state.config.market.clone(),
                    state.config.wallpaper_dir.clone(),
                )
            };

            // Fetch and apply
            match crate::bing::fetch_bing_image_info(&market).await {
                Ok(image) => {
                    println!("Found: {}", image.title);
                    match crate::bing::download_image(&image, &wallpaper_dir, &market).await {
                        Ok(path) => {
                            println!("Downloaded to: {}", path);
                            match crate::service::apply_cosmic_wallpaper(&path) {
                                Ok(()) => {
                                    println!("Wallpaper applied successfully!");
                                    // Record fetch for timer state
                                    let state = state_for_timer.read().await;
                                    state.timer.record_fetch();

                                    // Send notification
                                    let _ = Command::new("notify-send")
                                        .args(["-i", "preferences-desktop-wallpaper",
                                               "Bing Wallpaper", "Today's wallpaper has been applied!"])
                                        .spawn();
                                }
                                Err(e) => eprintln!("Failed to apply: {}", e),
                            }
                        }
                        Err(e) => eprintln!("Failed to download: {}", e),
                    }
                }
                Err(e) => eprintln!("Failed to fetch: {}", e),
            }
        }
    });

    // Main loop
    loop {
        if should_quit.load(Ordering::SeqCst) {
            break;
        }

        // Check for update requests (non-blocking)
        if let Ok(update) = update_rx.try_recv() {
            match update {
                TrayUpdate::SetTimerEnabled(enabled) => {
                    handle.update(|tray| {
                        tray.timer_enabled = enabled;
                    }).await;
                }
                TrayUpdate::FetchWallpaper => {
                    // Spawn fetch task
                    let state_clone = state.clone();
                    tokio::spawn(async move {
                        let (market, wallpaper_dir) = {
                            let state = state_clone.read().await;
                            (state.config.market.clone(), state.config.wallpaper_dir.clone())
                        };

                        match crate::bing::fetch_bing_image_info(&market).await {
                            Ok(image) => {
                                match crate::bing::download_image(&image, &wallpaper_dir, &market).await {
                                    Ok(path) => {
                                        match crate::service::apply_cosmic_wallpaper(&path) {
                                            Ok(()) => {
                                                let state = state_clone.read().await;
                                                state.timer.record_fetch();

                                                let _ = Command::new("notify-send")
                                                    .args(["-i", "preferences-desktop-wallpaper",
                                                           "Bing Wallpaper", "Today's wallpaper has been applied!"])
                                                    .spawn();
                                            }
                                            Err(e) => {
                                                let _ = Command::new("notify-send")
                                                    .args(["-u", "critical", "-i", "dialog-error",
                                                           "Bing Wallpaper", &format!("Failed to apply: {}", e)])
                                                    .spawn();
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let _ = Command::new("notify-send")
                                            .args(["-u", "critical", "-i", "dialog-error",
                                                   "Bing Wallpaper", &format!("Failed to download: {}", e)])
                                            .spawn();
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = Command::new("notify-send")
                                    .args(["-u", "critical", "-i", "dialog-error",
                                           "Bing Wallpaper", &format!("Failed to fetch: {}", e)])
                                    .spawn();
                            }
                        }
                    });
                }
            }
        }

        // Check for theme file changes (non-blocking)
        if theme_rx.try_recv().is_ok() {
            handle.update(|tray| {
                tray.dark_mode = is_dark_mode();
            }).await;
        }

        // Periodically check for external timer state changes (from GUI via D-Bus)
        // Check every ~500ms (10 iterations * 50ms sleep)
        static TIMER_CHECK_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let counter = TIMER_CHECK_COUNTER.fetch_add(1, Ordering::Relaxed);
        if counter % 10 == 0 {
            let current_enabled = timer.is_enabled();
            handle.update(|tray| {
                if tray.timer_enabled != current_enabled {
                    tray.timer_enabled = current_enabled;
                }
            }).await;
        }

        // Refresh lockfile timestamp every 30 seconds
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

        // Short sleep to avoid busy-waiting
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }

    // Cleanup
    timer_handle.abort();
    handle.shutdown();
    drop(dbus_conn);

    Ok(())
}

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
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use zbus::connection;

use crate::service::{ServiceState, WallpaperService, SERVICE_NAME, OBJECT_PATH};
use crate::timer::InternalTimer;

/// Get the host's COSMIC config directory
/// In Flatpak, dirs::config_dir() returns the sandboxed config, not the host's
fn host_cosmic_config_dir() -> Option<PathBuf> {
    // Always use home directory + .config to get host's config
    // This works both in Flatpak (with filesystem access) and native
    dirs::home_dir().map(|h| h.join(".config/cosmic"))
}

/// Get the path to COSMIC's theme mode config file
fn cosmic_theme_path() -> Option<PathBuf> {
    host_cosmic_config_dir().map(|d| d.join("com.system76.CosmicTheme.Mode/v1/is_dark"))
}

/// Get the path to the active theme directory
fn cosmic_theme_dir() -> Option<PathBuf> {
    let is_dark = is_dark_mode();
    let theme_name = if is_dark { "Dark" } else { "Light" };
    host_cosmic_config_dir().map(|d| d.join(format!("com.system76.CosmicTheme.{}/v1", theme_name)))
}

/// Get modification time of theme color files for change detection
fn get_theme_files_mtime() -> Option<std::time::SystemTime> {
    let theme_dir = cosmic_theme_dir()?;
    let accent_path = theme_dir.join("accent");
    let bg_path = theme_dir.join("background");

    // Return the most recent modification time of either file
    let accent_mtime = fs::metadata(&accent_path).ok()?.modified().ok()?;
    let bg_mtime = fs::metadata(&bg_path).ok()?.modified().ok()?;

    Some(accent_mtime.max(bg_mtime))
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

/// Reason for tray exit - used for suspend/resume detection
#[derive(Debug)]
enum TrayExitReason {
    /// User requested quit via menu
    Quit,
    /// Detected suspend/resume, should restart tray
    SuspendResume,
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
    // Show menu on left-click (same as right-click)
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "io.github.reality2_roycdavies.cosmic-bing-wallpaper".to_string()
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
                icon_name: "emblem-downloads-symbolic".to_string(),
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
                    "appointment-recurring-symbolic".to_string()
                } else {
                    "appointment-missed-symbolic".to_string()
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
                label: "Settings...".to_string(),
                icon_name: "preferences-system-symbolic".to_string(),
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
                icon_name: "application-exit-symbolic".to_string(),
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
///
/// The tray automatically restarts after suspend/resume to recover from
/// stale D-Bus connections that cause the icon to disappear.
pub fn run_tray() -> Result<(), String> {
    // Brief delay on startup to ensure StatusNotifierWatcher is ready
    // This helps when autostarting at login before the panel is fully initialized
    std::thread::sleep(Duration::from_secs(2));

    // Create the tokio runtime for async operations
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("Failed to create tokio runtime: {}", e))?;

    // Outer retry loop - restarts tray after suspend/resume
    loop {
        match rt.block_on(run_tray_inner())? {
            TrayExitReason::Quit => break,
            TrayExitReason::SuspendResume => {
                println!("Detected suspend/resume, restarting tray...");
                // Brief delay before restarting to let D-Bus settle
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
        }
    }

    Ok(())
}

/// Inner async implementation of the tray service
/// Returns the reason for exit so the outer loop can decide whether to restart
async fn run_tray_inner() -> Result<TrayExitReason, String> {
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
    let _watcher = {
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
            // Watch theme mode directory (is_dark)
            if let Some(theme_path) = cosmic_theme_path() {
                if let Some(watch_dir) = theme_path.parent() {
                    let _ = w.watch(watch_dir, RecursiveMode::NonRecursive);
                }
            }
            // Watch theme color files directory (accent, background)
            if let Some(theme_dir) = cosmic_theme_dir() {
                let _ = w.watch(&theme_dir, RecursiveMode::NonRecursive);
            }
        }
        watcher.ok()
    };

    // Track theme file modification times for robust change detection
    let mut tracked_theme_mtime = get_theme_files_mtime();

    // Spawn timer event handler
    let state_for_timer = state.clone();
    let timer_handle = tokio::spawn(async move {
        while let Some(()) = timer_rx.recv().await {
            // Timer fired - fetch and apply wallpaper
            println!("Timer fired - fetching wallpaper...");

            // Reload config from disk to get latest settings (GUI may have changed them)
            let fresh_config = crate::config::Config::load();
            let (market, wallpaper_dir) = (
                fresh_config.market.clone(),
                fresh_config.wallpaper_dir.clone(),
            );

            // Update state with fresh config
            {
                let mut state = state_for_timer.write().await;
                state.config = fresh_config;
            }

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

    // Track time for suspend/resume detection
    let mut loop_start = Instant::now();

    // Main loop
    loop {
        // Detect suspend/resume by checking for time jumps
        // If the sleep took much longer than expected (>5 seconds vs expected 50ms),
        // we likely woke from suspend and should restart to recover D-Bus connections
        let elapsed = loop_start.elapsed();
        if elapsed > Duration::from_secs(5) {
            println!("Time jump detected ({:?}), likely suspend/resume", elapsed);
            // Cleanup before returning
            timer_handle.abort();
            handle.shutdown();
            drop(dbus_conn);
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            crate::remove_tray_lockfile();
            return Ok(TrayExitReason::SuspendResume);
        }
        loop_start = Instant::now();

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
                        // Reload config from disk to get latest settings
                        let fresh_config = crate::config::Config::load();
                        let (market, wallpaper_dir) = (
                            fresh_config.market.clone(),
                            fresh_config.wallpaper_dir.clone(),
                        );

                        // Update state with fresh config
                        {
                            let mut state = state_clone.write().await;
                            state.config = fresh_config;
                        }

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

        // Check for theme file changes (non-blocking via watcher)
        // Also poll periodically as fallback since inotify isn't always reliable
        static THEME_CHECK_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let theme_counter = THEME_CHECK_COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut theme_changed = theme_rx.try_recv().is_ok() || theme_counter % 20 == 0; // Check every ~1 second

        // Also check theme file modification times as robust backup
        if theme_counter % 20 == 0 {
            let new_mtime = get_theme_files_mtime();
            if new_mtime != tracked_theme_mtime {
                tracked_theme_mtime = new_mtime;
                theme_changed = true;
            }
        }

        if theme_changed {
            let new_dark_mode = is_dark_mode();
            handle.update(|tray| {
                if tray.dark_mode != new_dark_mode {
                    tray.dark_mode = new_dark_mode;
                }
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

    // Explicitly drop the D-Bus connection to release the well-known name
    // This needs to happen before we exit the async block so the release
    // message is actually sent
    drop(dbus_conn);

    // Small delay to ensure D-Bus has time to process the name release
    // Without this, the name might still appear "owned" briefly after exit
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Clean up lockfile on exit
    crate::remove_tray_lockfile();

    Ok(TrayExitReason::Quit)
}

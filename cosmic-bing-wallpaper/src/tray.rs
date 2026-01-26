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

/// Parse a color from COSMIC theme RON format
/// Looks for pattern like: red: 0.5, green: 0.3, blue: 0.2,
fn parse_color_from_ron(content: &str, color_name: &str) -> Option<(u8, u8, u8)> {
    let search_pattern = format!("{}:", color_name);
    let start_idx = content.find(&search_pattern)?;
    let block_start = content[start_idx..].find('(')?;
    let block_end = content[start_idx + block_start..].find(')')?;
    let block = &content[start_idx + block_start..start_idx + block_start + block_end + 1];

    let extract_float = |name: &str| -> Option<f32> {
        let pattern = format!("{}: ", name);
        let idx = block.find(&pattern)?;
        let start = idx + pattern.len();
        let end = block[start..].find(',')?;
        block[start..start + end].trim().parse().ok()
    };

    let red = extract_float("red")?;
    let green = extract_float("green")?;
    let blue = extract_float("blue")?;

    Some((
        (red.clamp(0.0, 1.0) * 255.0) as u8,
        (green.clamp(0.0, 1.0) * 255.0) as u8,
        (blue.clamp(0.0, 1.0) * 255.0) as u8,
    ))
}

/// Get theme colors for the tray icon by reading directly from config files
fn get_theme_colors() -> ((u8, u8, u8), (u8, u8, u8)) {
    // Default colors
    let default_normal = (200, 200, 200);
    let default_accent = (0, 200, 200);

    let theme_dir = match cosmic_theme_dir() {
        Some(dir) => dir,
        None => return (default_normal, default_accent),
    };

    // Read accent color
    let accent_path = theme_dir.join("accent");
    let accent = if let Ok(content) = fs::read_to_string(&accent_path) {
        parse_color_from_ron(&content, "base").unwrap_or(default_accent)
    } else {
        default_accent
    };

    // Read background on color (foreground)
    let bg_path = theme_dir.join("background");
    let normal = if let Ok(content) = fs::read_to_string(&bg_path) {
        parse_color_from_ron(&content, "on").unwrap_or(default_normal)
    } else {
        default_normal
    };

    (normal, accent)
}

/// Generate the tray icon dynamically using theme colors
/// Icon is 24x24, showing a landscape/frame with sun and on/off indicator
fn create_tray_icon(timer_enabled: bool) -> Vec<u8> {
    let size: i32 = 24;
    let mut pixels = vec![0u8; (size * size * 4) as usize];

    let (normal_color, accent_color) = get_theme_colors();
    let (r, g, b) = normal_color;
    let (ar, ag, ab) = accent_color;

    // Helper to set a pixel (ARGB format for ksni)
    let set_pixel = |pixels: &mut Vec<u8>, x: i32, y: i32, r: u8, g: u8, b: u8, a: u8| {
        if x >= 0 && x < size && y >= 0 && y < size {
            let idx = ((y * size + x) * 4) as usize;
            pixels[idx] = a;
            pixels[idx + 1] = r;
            pixels[idx + 2] = g;
            pixels[idx + 3] = b;
        }
    };

    // Draw frame (rectangle outline)
    for x in 1..23 {
        set_pixel(&mut pixels, x, 3, r, g, b, 255);   // top
        set_pixel(&mut pixels, x, 20, r, g, b, 255);  // bottom
    }
    for y in 3..21 {
        set_pixel(&mut pixels, 1, y, r, g, b, 255);   // left
        set_pixel(&mut pixels, 22, y, r, g, b, 255);  // right
    }

    // Draw sun (filled circle at top-right area)
    let sun_cx = 17.0f32;
    let sun_cy = 7.0f32;
    let sun_r = 2.5f32;
    for y in 4..11 {
        for x in 14..21 {
            let dx = x as f32 - sun_cx;
            let dy = y as f32 - sun_cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= sun_r {
                let alpha = if dist > sun_r - 1.0 {
                    ((sun_r - dist) * 255.0) as u8
                } else {
                    255
                };
                set_pixel(&mut pixels, x, y, r, g, b, alpha);
            }
        }
    }

    // Draw mountain/landscape (filled polygon approximation)
    // Mountain 1: peak at (9, 10), base from (3, 17) to (15, 17)
    for y in 10..18 {
        let half_width = ((y - 10) as f32 * 1.0) as i32;
        for x in (9 - half_width).max(3)..(9 + half_width).min(15) {
            set_pixel(&mut pixels, x, y, r, g, b, 200);
        }
    }
    // Mountain 2: peak at (15, 8), base from (10, 17) to (20, 17)
    for y in 8..18 {
        let half_width = ((y - 8) as f32 * 0.8) as i32;
        for x in (15 - half_width).max(10)..(15 + half_width).min(20) {
            set_pixel(&mut pixels, x, y, r, g, b, 220);
        }
    }

    // Draw on/off indicator (bottom-right badge)
    let badge_cx = 19.0f32;
    let badge_cy = 17.0f32;
    let badge_r = 4.0f32;

    // Badge background circle
    for y in 13..22 {
        for x in 15..24 {
            let dx = x as f32 - badge_cx;
            let dy = y as f32 - badge_cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= badge_r {
                if timer_enabled {
                    set_pixel(&mut pixels, x, y, ar, ag, ab, 255);
                } else {
                    set_pixel(&mut pixels, x, y, 128, 128, 128, 255);
                }
            }
        }
    }

    // Draw checkmark (on) or X (off) inside badge
    if timer_enabled {
        // Checkmark
        set_pixel(&mut pixels, 17, 17, 255, 255, 255, 255);
        set_pixel(&mut pixels, 18, 18, 255, 255, 255, 255);
        set_pixel(&mut pixels, 19, 17, 255, 255, 255, 255);
        set_pixel(&mut pixels, 20, 16, 255, 255, 255, 255);
        set_pixel(&mut pixels, 21, 15, 255, 255, 255, 255);
    } else {
        // X mark
        for i in 0..5 {
            set_pixel(&mut pixels, 17 + i, 15 + i, 255, 255, 255, 255);
            set_pixel(&mut pixels, 21 - i, 15 + i, 255, 255, 255, 255);
        }
    }

    pixels
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
        // Generate icon dynamically using current theme colors
        let icon_data = create_tray_icon(self.timer_enabled);

        vec![ksni::Icon {
            width: 24,
            height: 24,
            data: icon_data,
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
            // Force icon refresh by updating tray state
            // The icon is generated dynamically with current theme colors
            let new_dark_mode = is_dark_mode();
            handle.update(|tray| {
                tray.dark_mode = new_dark_mode;
                // Touch timer_enabled to force icon regeneration
                // (icon_pixmap is called after any update)
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

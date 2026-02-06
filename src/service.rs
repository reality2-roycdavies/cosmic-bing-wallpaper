//! # Wallpaper Service Module
//!
//! Implements the core wallpaper functionality as a D-Bus service.
//! This service is embedded in the panel applet process.
//!
//! ## D-Bus Interface
//!
//! Service name: `io.github.reality2_roycdavies.cosmic-bing-wallpaper.Wallpaper1`
//! Object path: `/io/github/reality2_roycdavies/cosmic_bing_wallpaper/Wallpaper1`
//!
//! ### Methods
//! - `FetchWallpaper(apply: bool)` - Fetch today's wallpaper, optionally apply it
//! - `ApplyWallpaper(path: String)` - Apply a specific wallpaper by path
//! - `GetConfig()` - Get current configuration
//! - `SetMarket(market: String)` - Set the Bing regional market
//! - `GetTimerEnabled()` - Check if auto-update timer is enabled
//! - `SetTimerEnabled(enabled: bool)` - Enable or disable auto-update timer
//! - `GetHistory()` - Get list of downloaded wallpapers
//!
//! ### Signals
//! - `WallpaperChanged(path: String, title: String)` - Emitted when wallpaper changes
//! - `TimerStateChanged(enabled: bool)` - Emitted when timer state changes
//! - `FetchProgress(state: String, message: String)` - Emitted during fetch operations

// --- Standard library and async imports ---
use std::future::Future;    // Trait for async functions (used by run_in_tokio)
use std::sync::Arc;         // Thread-safe reference counting
use tokio::sync::RwLock;    // Async read-write lock for shared state

// --- D-Bus framework ---
use zbus::{interface, SignalContext};  // interface = attribute macro, SignalContext = for emitting signals

// --- Internal modules ---
use crate::bing::{self, BingImage};   // Bing API client
use crate::config::Config;           // User configuration
use crate::timer::InternalTimer;     // Daily timer

/// Checks if the application is running inside a Flatpak sandbox.
///
/// Flatpak creates a `/.flatpak-info` file inside the sandbox. We use this
/// to decide whether to use `flatpak-spawn --host` for running commands
/// on the host system (outside the sandbox).
pub fn is_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// Helper to run async code that requires a tokio runtime.
///
/// The D-Bus service methods are called by zbus's own async executor,
/// which is NOT tokio. But our HTTP client (reqwest) requires tokio.
/// This helper creates a temporary single-threaded tokio runtime to
/// bridge the gap. Each D-Bus method call gets its own mini-runtime.
fn run_in_tokio<T>(future: impl Future<Output = T>) -> T {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()    // Enable I/O and time drivers
        .build()
        .expect("Failed to create tokio runtime");
    rt.block_on(future)
}

/// D-Bus service name — must be unique on the session bus.
/// Other applications use this name to find and call our service.
pub const SERVICE_NAME: &str = "io.github.reality2_roycdavies.cosmic-bing-wallpaper.Wallpaper1";

/// D-Bus object path — the "address" where our interface lives.
/// Follows the convention of converting dots to slashes.
pub const OBJECT_PATH: &str = "/io/github/reality2_roycdavies/cosmic_bing_wallpaper/Wallpaper1";

/// Represents a wallpaper in the download history.
///
/// This struct is sent over D-Bus (requires zbus::zvariant::Type for serialization)
/// and also used for JSON serialization (serde::Serialize/Deserialize).
#[derive(Debug, Clone, zbus::zvariant::Type, serde::Serialize, serde::Deserialize)]
pub struct WallpaperInfo {
    /// Full filesystem path to the image (e.g., "/home/user/Pictures/BingWallpapers/bing-en-US-2026-02-05.jpg")
    pub path: String,
    /// Just the filename (e.g., "bing-en-US-2026-02-05.jpg")
    pub filename: String,
    /// Date extracted from the filename (e.g., "2026-02-05")
    pub date: String,
}

/// Shared mutable state for the wallpaper service.
///
/// This is wrapped in `Arc<RwLock<...>>` and shared between:
/// - The D-Bus service (handles requests from the settings window)
/// - The timer event handler (triggers automatic fetches)
/// - The command handler (processes UI requests from the applet popup)
pub struct ServiceState {
    /// User configuration (market, wallpaper directory, keep_days, etc.)
    pub config: Config,
    /// Metadata for the most recently fetched image (for display/reference)
    pub current_image: Option<BingImage>,
    /// Filesystem path to the most recently applied wallpaper
    pub current_path: Option<String>,
    /// Reference to the internal timer (shared with the applet for enable/disable)
    pub timer: Arc<InternalTimer>,
}

impl ServiceState {
    /// Creates a new ServiceState with default config loaded from disk
    pub fn new(timer: Arc<InternalTimer>) -> Self {
        Self {
            config: Config::load(),
            current_image: None,
            current_path: None,
            timer,
        }
    }
}

/// The D-Bus interface implementation.
///
/// This struct is registered with zbus and its methods become callable
/// over D-Bus by other processes (like the settings window).
/// The `#[interface]` attribute macro on the impl block below
/// generates the D-Bus interface boilerplate.
pub struct WallpaperService {
    /// Shared state — allows the service to read/write the same state
    /// as the applet's background thread
    state: Arc<RwLock<ServiceState>>,
}

impl WallpaperService {
    pub fn new(state: Arc<RwLock<ServiceState>>) -> Self {
        Self { state }
    }
}

/// D-Bus interface implementation.
///
/// The `#[interface]` macro makes each method below callable over D-Bus.
/// Methods marked with `#[zbus(signal)]` become D-Bus signals that clients
/// can subscribe to for real-time notifications.
///
/// Signal contexts (`SignalContext`) are automatically injected by zbus
/// when a method parameter is annotated with `#[zbus(signal_context)]`.
#[interface(name = "io.github.reality2_roycdavies.cosmic_bing_wallpaper.Wallpaper1")]
impl WallpaperService {
    /// Fetch today's wallpaper from Bing
    ///
    /// # Arguments
    /// * `apply` - If true, also apply the wallpaper after downloading
    ///
    /// # Returns
    /// * Success: WallpaperInfo with path, filename, and date
    /// * Error: Error message string
    async fn fetch_wallpaper(
        &self,
        apply: bool,
        #[zbus(signal_context)] ctx: SignalContext<'_>,
    ) -> zbus::fdo::Result<WallpaperInfo> {
        // Emit progress signal
        Self::fetch_progress(&ctx, "starting", "Fetching image info...").await?;

        let (market, wallpaper_dir) = {
            let state = self.state.read().await;
            (state.config.market.clone(), state.config.wallpaper_dir.clone())
        };

        // Fetch image info from Bing (must run in tokio runtime since reqwest requires it)
        let image = run_in_tokio(bing::fetch_bing_image_info(&market))
            .map_err(zbus::fdo::Error::Failed)?;

        Self::fetch_progress(&ctx, "downloading", &format!("Downloading: {}", image.title)).await?;

        // Download the image (must run in tokio runtime since reqwest requires it)
        let path = run_in_tokio(bing::download_image(&image, &wallpaper_dir, &market))
            .map_err(zbus::fdo::Error::Failed)?;

        // Clean up old wallpapers
        let keep_days = {
            let state = self.state.read().await;
            state.config.keep_days
        };
        cleanup_old_wallpapers(&wallpaper_dir, keep_days);

        // Update state
        {
            let mut state = self.state.write().await;
            state.current_image = Some(image.clone());
            state.current_path = Some(path.clone());
        }

        // Apply if requested
        if apply {
            Self::fetch_progress(&ctx, "applying", "Applying wallpaper...").await?;
            apply_cosmic_wallpaper(&path)
                .map_err(zbus::fdo::Error::Failed)?;

            // Emit wallpaper changed signal
            Self::wallpaper_changed(&ctx, &path, &image.title).await?;
        }

        // Record successful fetch for timer catch-up logic
        {
            let state = self.state.read().await;
            state.timer.record_fetch();
        }

        Self::fetch_progress(&ctx, "complete", "Done!").await?;

        let filename = std::path::Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        let date = extract_date_from_filename(&filename);

        Ok(WallpaperInfo { path, filename, date })
    }

    /// Apply a specific wallpaper by path
    async fn apply_wallpaper(
        &self,
        path: String,
        #[zbus(signal_context)] ctx: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        apply_cosmic_wallpaper(&path)
            .map_err(zbus::fdo::Error::Failed)?;

        // Get title from current image or use filename
        let title = {
            let state = self.state.read().await;
            state.current_image.as_ref()
                .filter(|_| state.current_path.as_ref() == Some(&path))
                .map(|img| img.title.clone())
                .unwrap_or_else(|| {
                    std::path::Path::new(&path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Wallpaper")
                        .to_string()
                })
        };

        Self::wallpaper_changed(&ctx, &path, &title).await?;
        Ok(())
    }

    /// Get current configuration as JSON
    async fn get_config(&self) -> zbus::fdo::Result<String> {
        let state = self.state.read().await;
        serde_json::to_string(&state.config)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// Get the current Bing market code
    async fn get_market(&self) -> String {
        let state = self.state.read().await;
        state.config.market.clone()
    }

    /// Set the Bing regional market
    async fn set_market(&self, market: String) -> zbus::fdo::Result<()> {
        let mut state = self.state.write().await;
        state.config.market = market;
        state.config.save()
            .map_err(zbus::fdo::Error::Failed)
    }

    /// Get the wallpaper directory path
    async fn get_wallpaper_dir(&self) -> String {
        let state = self.state.read().await;
        state.config.wallpaper_dir.clone()
    }

    /// Check if auto-update timer is enabled
    async fn get_timer_enabled(&self) -> bool {
        let state = self.state.read().await;
        state.timer.is_enabled()
    }

    /// Enable or disable the auto-update timer
    async fn set_timer_enabled(
        &self,
        enabled: bool,
        #[zbus(signal_context)] ctx: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        {
            let state = self.state.read().await;
            state.timer.set_enabled(enabled);
        }

        Self::timer_state_changed(&ctx, enabled).await?;
        Ok(())
    }

    /// Get the next scheduled timer run (empty string if not enabled)
    async fn get_timer_next_run(&self) -> String {
        let state = self.state.read().await;
        state.timer.next_run_string().await
    }

    /// Get the current wallpaper path (if any has been fetched this session)
    /// Returns empty string if no wallpaper has been fetched yet
    async fn get_current_wallpaper_path(&self) -> String {
        let state = self.state.read().await;
        state.current_path.clone().unwrap_or_default()
    }

    /// Get list of downloaded wallpapers
    async fn get_history(&self) -> Vec<WallpaperInfo> {
        let state = self.state.read().await;
        scan_history(&state.config.wallpaper_dir)
    }

    /// Delete a wallpaper from history
    async fn delete_wallpaper(&self, path: String) -> zbus::fdo::Result<()> {
        std::fs::remove_file(&path)
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to delete: {e}")))
    }

    // === Signals ===

    /// Signal emitted when the wallpaper changes
    #[zbus(signal)]
    async fn wallpaper_changed(ctx: &SignalContext<'_>, path: &str, title: &str) -> zbus::Result<()>;

    /// Signal emitted when timer state changes
    #[zbus(signal)]
    async fn timer_state_changed(ctx: &SignalContext<'_>, enabled: bool) -> zbus::Result<()>;

    /// Signal emitted during fetch operations
    #[zbus(signal)]
    async fn fetch_progress(ctx: &SignalContext<'_>, state: &str, message: &str) -> zbus::Result<()>;
}

/// Extracts the date from a wallpaper filename.
///
/// Filenames follow the pattern "bing-{market}-YYYY-MM-DD.jpg".
/// This function takes the last 10 characters before the extension
/// and checks if they match the YYYY-MM-DD pattern.
///
/// # Examples
/// ```ignore
/// extract_date_from_filename("bing-en-US-2026-02-05.jpg")  // → "2026-02-05"
/// extract_date_from_filename("unknown.jpg")                 // → "unknown"
/// ```
pub fn extract_date_from_filename(filename: &str) -> String {
    // Strip the file extension (.jpg, .jpeg, or .png)
    let name_without_ext = filename
        .strip_suffix(".jpg")
        .or_else(|| filename.strip_suffix(".jpeg"))
        .or_else(|| filename.strip_suffix(".png"))
        .unwrap_or(filename);

    // Check if the last 10 characters look like a date (YYYY-MM-DD)
    if name_without_ext.len() >= 10 {
        let potential_date = &name_without_ext[name_without_ext.len() - 10..];
        if potential_date.len() == 10
            && potential_date.chars().nth(4) == Some('-')
            && potential_date.chars().nth(7) == Some('-')
        {
            return potential_date.to_string();
        }
    }
    // Fallback: return the whole name without extension
    name_without_ext.to_string()
}

/// Scans the wallpaper directory for downloaded images and returns them as WallpaperInfo.
///
/// This is the D-Bus version (returns WallpaperInfo for serialization over D-Bus).
/// The settings window has its own version that returns HistoryItems (with PathBuf).
fn scan_history(wallpaper_dir: &str) -> Vec<WallpaperInfo> {
    let dir = std::path::Path::new(wallpaper_dir);
    if !dir.exists() {
        return Vec::new();
    }

    let mut items: Vec<WallpaperInfo> = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().extension()
                .map(|ext| ext == "jpg" || ext == "jpeg" || ext == "png")
                .unwrap_or(false)
        })
        .map(|entry| {
            let path = entry.path();
            let path_str = path.to_string_lossy().to_string();
            let filename = path.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let date = extract_date_from_filename(&filename);
            WallpaperInfo { path: path_str, filename, date }
        })
        .collect();

    items.sort_by(|a, b| b.date.cmp(&a.date));
    items
}

/// Removes old wallpapers that are past the retention period.
///
/// Scans the wallpaper directory for files matching "bing-*.jpg",
/// parses the date from each filename, and deletes any older than `keep_days`.
///
/// # Arguments
/// * `wallpaper_dir` - Path to the wallpaper storage directory
/// * `keep_days` - Number of days to keep wallpapers (0 = keep forever)
///
/// # Returns
/// The number of wallpapers deleted
pub fn cleanup_old_wallpapers(wallpaper_dir: &str, keep_days: u32) -> usize {
    if keep_days == 0 {
        return 0;
    }

    let dir = std::path::Path::new(wallpaper_dir);
    if !dir.exists() {
        return 0;
    }

    let cutoff_date = chrono::Local::now().date_naive() - chrono::Duration::days(keep_days as i64);
    let mut deleted = 0;

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let filename = path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            if !filename.starts_with("bing-") || !filename.ends_with(".jpg") {
                continue;
            }

            let name_without_ext = filename.strip_suffix(".jpg").unwrap_or(filename);
            if name_without_ext.len() < 10 {
                continue;
            }

            let date_str = &name_without_ext[name_without_ext.len() - 10..];
            if let Ok(file_date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                if file_date < cutoff_date && std::fs::remove_file(&path).is_ok() {
                    deleted += 1;
                }
            }
        }
    }

    deleted
}

/// Runs a command on the host system, automatically handling Flatpak sandboxing.
///
/// When running inside Flatpak, commands need to be prefixed with
/// `flatpak-spawn --host` to execute on the host rather than inside the sandbox.
/// This helper does that transparently.
///
/// # Arguments
/// * `cmd` - The command to run (e.g., "pkill", "pgrep")
/// * `args` - Command arguments (e.g., &["-TERM", "-x", "cosmic-bg"])
fn run_host_command(cmd: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    if is_flatpak() {
        let mut spawn_args = vec!["--host", cmd];
        spawn_args.extend(args);
        std::process::Command::new("flatpak-spawn")
            .args(&spawn_args)
            .output()
    } else {
        std::process::Command::new(cmd)
            .args(args)
            .output()
    }
}

/// Spawns a command in the background on the host system (non-blocking).
/// Like `run_host_command` but doesn't wait for the command to finish.
fn spawn_host_command(cmd: &str) -> std::io::Result<std::process::Child> {
    if is_flatpak() {
        std::process::Command::new("flatpak-spawn")
            .args(["--host", cmd])
            .spawn()
    } else {
        std::process::Command::new(cmd)
            .spawn()
    }
}

/// Applies a wallpaper image to the COSMIC desktop.
///
/// COSMIC desktop reads its background configuration from a RON (Rust Object Notation)
/// file at `~/.config/cosmic/com.system76.CosmicBackground/v1/all`. This function:
/// 1. Writes the config file with the new image path
/// 2. Kills the `cosmic-bg` process (COSMIC's background renderer)
/// 3. COSMIC automatically restarts `cosmic-bg`, which reads the new config
/// 4. If COSMIC doesn't restart it, we start it manually
///
/// # Arguments
/// * `image_path` - Absolute path to the wallpaper image file
///
/// # Why we kill cosmic-bg
/// COSMIC doesn't have a "reload config" API — the only way to make it
/// pick up a new wallpaper is to restart the background process.
pub fn apply_cosmic_wallpaper(image_path: &str) -> Result<(), String> {
    // We use home_dir() instead of config_dir() because in Flatpak,
    // config_dir() returns the sandboxed path (~/.var/app/APP_ID/config/),
    // but COSMIC reads from the real ~/.config/ on the host.
    let config_path = dirs::home_dir()
        .ok_or("Could not find home directory")?
        .join(".config/cosmic/com.system76.CosmicBackground/v1/all");

    // Write the COSMIC background config in RON format.
    // RON is Rust's native serialization format, similar to JSON but Rust-flavored.
    let config_content = format!(
        r#"(
    output: "all",
    source: Path("{}"),
    filter_by_theme: false,
    rotation_frequency: 300,
    filter_method: Lanczos,
    scaling_mode: Zoom,
    sampling_method: Alphanumeric,
)"#,
        image_path
    );

    // Ensure the config directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config dir: {e}"))?;
    }

    // Write the config file
    std::fs::write(&config_path, config_content)
        .map_err(|e| format!("Failed to write config: {e}"))?;

    // Send SIGTERM to cosmic-bg to trigger a restart with the new config
    let _ = run_host_command("pkill", &["-TERM", "-x", "cosmic-bg"]);

    // Give COSMIC a moment to auto-restart cosmic-bg
    std::thread::sleep(std::time::Duration::from_millis(1000));

    // Verify cosmic-bg is running; if COSMIC didn't restart it, start it manually
    let check = run_host_command("pgrep", &["-x", "cosmic-bg"]);
    match check {
        Ok(output) if output.status.success() => Ok(()),  // Already running
        _ => {
            // Not running — start it ourselves
            spawn_host_command("cosmic-bg")
                .map_err(|e| format!("Failed to start cosmic-bg: {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(500));
            Ok(())
        }
    }
}

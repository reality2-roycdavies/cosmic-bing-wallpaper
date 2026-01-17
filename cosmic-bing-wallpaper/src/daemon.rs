//! # D-Bus Daemon Module
//!
//! Implements a background D-Bus service that provides the core wallpaper functionality.
//! Both the GUI application and system tray act as clients to this daemon.
//!
//! ## D-Bus Interface
//!
//! Service name: `org.cosmicbing.Wallpaper1`
//! Object path: `/org/cosmicbing/Wallpaper1`
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

use std::future::Future;
use std::sync::Arc;
use tokio::sync::RwLock;
use zbus::{connection, interface, SignalContext};

use crate::bing::{self, BingImage};
use crate::config::Config;

/// Check if running inside a Flatpak sandbox
fn is_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// Run systemctl command, using flatpak-spawn when in Flatpak sandbox
fn run_systemctl(args: &[&str]) -> std::io::Result<std::process::Output> {
    if is_flatpak() {
        let mut spawn_args = vec!["--host", "systemctl"];
        spawn_args.extend(args);
        std::process::Command::new("flatpak-spawn")
            .args(&spawn_args)
            .output()
    } else {
        std::process::Command::new("systemctl")
            .args(args)
            .output()
    }
}

/// Helper to run async code that requires tokio runtime (like reqwest)
/// within the zbus async context which uses a different executor.
fn run_in_tokio<T>(future: impl Future<Output = T>) -> T {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    rt.block_on(future)
}

/// D-Bus service name
pub const SERVICE_NAME: &str = "org.cosmicbing.Wallpaper1";

/// D-Bus object path
pub const OBJECT_PATH: &str = "/org/cosmicbing/Wallpaper1";

/// Represents a wallpaper in the download history (D-Bus serializable)
#[derive(Debug, Clone, zbus::zvariant::Type, serde::Serialize, serde::Deserialize)]
pub struct WallpaperInfo {
    /// Full filesystem path to the image
    pub path: String,
    /// Filename only
    pub filename: String,
    /// Date extracted from filename
    pub date: String,
}

/// Shared daemon state
pub struct DaemonState {
    /// User configuration
    pub config: Config,
    /// Currently fetched image info
    pub current_image: Option<BingImage>,
    /// Path to current image
    pub current_path: Option<String>,
}

impl DaemonState {
    pub fn new() -> Self {
        Self {
            config: Config::load(),
            current_image: None,
            current_path: None,
        }
    }
}

/// The D-Bus interface implementation
pub struct WallpaperService {
    state: Arc<RwLock<DaemonState>>,
}

impl WallpaperService {
    pub fn new(state: Arc<RwLock<DaemonState>>) -> Self {
        Self { state }
    }
}

#[interface(name = "org.cosmicbing.Wallpaper1")]
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
            .map_err(|e| zbus::fdo::Error::Failed(e))?;

        Self::fetch_progress(&ctx, "downloading", &format!("Downloading: {}", image.title)).await?;

        // Download the image (must run in tokio runtime since reqwest requires it)
        let path = run_in_tokio(bing::download_image(&image, &wallpaper_dir, &market))
            .map_err(|e| zbus::fdo::Error::Failed(e))?;

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
                .map_err(|e| zbus::fdo::Error::Failed(e))?;

            // Emit wallpaper changed signal
            Self::wallpaper_changed(&ctx, &path, &image.title).await?;
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
            .map_err(|e| zbus::fdo::Error::Failed(e))?;

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
            .map_err(|e| zbus::fdo::Error::Failed(e))
    }

    /// Get the wallpaper directory path
    async fn get_wallpaper_dir(&self) -> String {
        let state = self.state.read().await;
        state.config.wallpaper_dir.clone()
    }

    /// Check if auto-update timer is enabled
    async fn get_timer_enabled(&self) -> bool {
        is_timer_enabled()
    }

    /// Enable or disable the auto-update timer
    async fn set_timer_enabled(
        &self,
        enabled: bool,
        #[zbus(signal_context)] ctx: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        let result = if enabled {
            install_timer()
        } else {
            uninstall_timer()
        };

        result.map_err(|e| zbus::fdo::Error::Failed(e))?;
        Self::timer_state_changed(&ctx, enabled).await?;
        Ok(())
    }

    /// Get the next scheduled timer run (empty string if not installed)
    async fn get_timer_next_run(&self) -> String {
        get_timer_next_run()
    }

    /// Get list of downloaded wallpapers
    async fn get_history(&self) -> Vec<WallpaperInfo> {
        let state = self.state.read().await;
        scan_history(&state.config.wallpaper_dir)
    }

    /// Delete a wallpaper from history
    async fn delete_wallpaper(&self, path: String) -> zbus::fdo::Result<()> {
        std::fs::remove_file(&path)
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to delete: {}", e)))
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

/// Extract date from wallpaper filename
fn extract_date_from_filename(filename: &str) -> String {
    let name_without_ext = filename
        .strip_suffix(".jpg")
        .or_else(|| filename.strip_suffix(".jpeg"))
        .or_else(|| filename.strip_suffix(".png"))
        .unwrap_or(filename);

    if name_without_ext.len() >= 10 {
        let potential_date = &name_without_ext[name_without_ext.len() - 10..];
        if potential_date.len() == 10
            && potential_date.chars().nth(4) == Some('-')
            && potential_date.chars().nth(7) == Some('-')
        {
            return potential_date.to_string();
        }
    }
    name_without_ext.to_string()
}

/// Scan wallpaper directory for history items
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

/// Clean up old wallpapers based on keep_days setting
fn cleanup_old_wallpapers(wallpaper_dir: &str, keep_days: u32) -> usize {
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
                if file_date < cutoff_date {
                    if std::fs::remove_file(&path).is_ok() {
                        deleted += 1;
                    }
                }
            }
        }
    }

    deleted
}

/// Check if the systemd timer is enabled
fn is_timer_enabled() -> bool {
    run_systemctl(&["--user", "is-enabled", "cosmic-bing-wallpaper.timer"])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the next scheduled timer run time
fn get_timer_next_run() -> String {
    let output = run_systemctl(&["--user", "is-active", "cosmic-bing-wallpaper.timer"]);

    match output {
        Ok(out) if String::from_utf8_lossy(&out.stdout).trim() == "active" => {
            let next_output = run_systemctl(&["--user", "show", "cosmic-bing-wallpaper.timer",
                       "--property=NextElapseUSecRealtime"]);

            match next_output {
                Ok(out) => {
                    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    let timestamp = raw.strip_prefix("NextElapseUSecRealtime=").unwrap_or(&raw);

                    if timestamp.is_empty() || timestamp == "n/a" {
                        "Scheduled".to_string()
                    } else if let Ok(usecs) = timestamp.parse::<u64>() {
                        let secs = usecs / 1_000_000;
                        if let Some(dt) = chrono::DateTime::from_timestamp(secs as i64, 0) {
                            let local: chrono::DateTime<chrono::Local> = dt.into();
                            local.format("%a %b %d %H:%M").to_string()
                        } else {
                            "Scheduled".to_string()
                        }
                    } else {
                        timestamp.to_string()
                    }
                }
                Err(_) => "Scheduled".to_string()
            }
        }
        _ => String::new()
    }
}

/// Install the systemd timer for automatic updates
fn install_timer() -> Result<(), String> {
    let home = dirs::home_dir().ok_or("Could not find home directory")?;
    let systemd_dir = home.join(".config/systemd/user");

    std::fs::create_dir_all(&systemd_dir)
        .map_err(|e| format!("Failed to create systemd directory: {}", e))?;

    // Determine the executable path based on environment
    let exec_path = if is_flatpak() {
        // In Flatpak, use flatpak run to invoke the app
        "flatpak run io.github.reality2_roycdavies.cosmic-bing-wallpaper --fetch-and-apply".to_string()
    } else {
        // Find executable
        let local_bin = home.join(".local/bin/cosmic-bing-wallpaper");
        let system_bin = std::path::Path::new("/usr/local/bin/cosmic-bing-wallpaper");
        let local_script = home.join(".local/share/cosmic-bing-wallpaper/bing-wallpaper.sh");

        if local_bin.exists() {
            format!("{} --fetch-and-apply", local_bin.display())
        } else if system_bin.exists() {
            format!("{} --fetch-and-apply", system_bin.display())
        } else if local_script.exists() {
            local_script.to_string_lossy().to_string()
        } else {
            return Err("No executable found. Please install the app first.".to_string())
        }
    };

    // Write service file
    let service_content = format!(r#"[Unit]
Description=Fetch and set Bing daily wallpaper for COSMIC desktop
After=network-online.target graphical-session.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart={}
Environment=HOME=%h
Environment=XDG_RUNTIME_DIR=/run/user/%U

[Install]
WantedBy=default.target
"#, exec_path);

    std::fs::write(systemd_dir.join("cosmic-bing-wallpaper.service"), &service_content)
        .map_err(|e| format!("Failed to write service file: {}", e))?;

    // Write timer file
    let timer_content = r#"[Unit]
Description=Daily Bing wallpaper update timer

[Timer]
OnCalendar=*-*-* 08:00:00
OnBootSec=5min
RandomizedDelaySec=300
Persistent=true

[Install]
WantedBy=timers.target
"#;

    std::fs::write(systemd_dir.join("cosmic-bing-wallpaper.timer"), timer_content)
        .map_err(|e| format!("Failed to write timer file: {}", e))?;

    // Write login service
    let login_service_content = format!(r#"[Unit]
Description=Fetch Bing wallpaper on login/wake
After=graphical-session.target network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStartPre=/bin/sleep 10
ExecStart={}
Environment=HOME=%h
Environment=XDG_RUNTIME_DIR=/run/user/%U

[Install]
WantedBy=graphical-session.target
"#, exec_path);

    std::fs::write(systemd_dir.join("cosmic-bing-wallpaper-login.service"), &login_service_content)
        .map_err(|e| format!("Failed to write login service file: {}", e))?;

    // Reload and enable (using blocking commands - these are quick operations)
    let reload = run_systemctl(&["--user", "daemon-reload"])
        .map_err(|e| format!("Failed to reload systemd: {}", e))?;

    if !reload.status.success() {
        return Err("Failed to reload systemd daemon".to_string());
    }

    let enable_timer = run_systemctl(&["--user", "enable", "--now", "cosmic-bing-wallpaper.timer"])
        .map_err(|e| format!("Failed to enable timer: {}", e))?;

    if !enable_timer.status.success() {
        return Err(format!("Failed to enable timer: {}", String::from_utf8_lossy(&enable_timer.stderr)));
    }

    let enable_login = run_systemctl(&["--user", "enable", "cosmic-bing-wallpaper-login.service"])
        .map_err(|e| format!("Failed to enable login service: {}", e))?;

    if !enable_login.status.success() {
        return Err(format!("Failed to enable login service: {}", String::from_utf8_lossy(&enable_login.stderr)));
    }

    Ok(())
}

/// Uninstall the systemd timer
fn uninstall_timer() -> Result<(), String> {
    let output = run_systemctl(&["--user", "disable", "--now", "cosmic-bing-wallpaper.timer"])
        .map_err(|e| format!("Failed to disable timer: {}", e))?;

    if !output.status.success() {
        return Err(format!("Failed to disable timer: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let _ = run_systemctl(&["--user", "disable", "cosmic-bing-wallpaper-login.service"]);

    let home = dirs::home_dir().ok_or("Could not find home directory")?;
    let systemd_dir = home.join(".config/systemd/user");

    let _ = std::fs::remove_file(systemd_dir.join("cosmic-bing-wallpaper.service"));
    let _ = std::fs::remove_file(systemd_dir.join("cosmic-bing-wallpaper.timer"));
    let _ = std::fs::remove_file(systemd_dir.join("cosmic-bing-wallpaper-login.service"));

    let _ = run_systemctl(&["--user", "daemon-reload"]);

    Ok(())
}

/// Run a host command, using flatpak-spawn when in Flatpak sandbox
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

/// Spawn a host command in background, using flatpak-spawn when in Flatpak sandbox
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

/// Apply wallpaper to COSMIC desktop
fn apply_cosmic_wallpaper(image_path: &str) -> Result<(), String> {
    let config_path = dirs::config_dir()
        .ok_or("Could not find config directory")?
        .join("cosmic/com.system76.CosmicBackground/v1/all");

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

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config dir: {}", e))?;
    }

    std::fs::write(&config_path, config_content)
        .map_err(|e| format!("Failed to write config: {}", e))?;

    // Kill and restart cosmic-bg using host commands in Flatpak
    let _ = run_host_command("pkill", &["-x", "cosmic-bg"]);

    std::thread::sleep(std::time::Duration::from_millis(500));

    spawn_host_command("cosmic-bg")
        .map_err(|e| format!("Failed to start cosmic-bg: {}", e))?;

    std::thread::sleep(std::time::Duration::from_millis(300));

    let check = run_host_command("pgrep", &["-x", "cosmic-bg"]);

    match check {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err("cosmic-bg failed to start".to_string())
    }
}

/// Run the D-Bus daemon
pub async fn run_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let service = WallpaperService::new(state);

    let _conn = connection::Builder::session()?
        .name(SERVICE_NAME)?
        .serve_at(OBJECT_PATH, service)?
        .build()
        .await?;

    println!("D-Bus daemon running at {} on {}", OBJECT_PATH, SERVICE_NAME);

    // Keep the daemon running
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
    }
}

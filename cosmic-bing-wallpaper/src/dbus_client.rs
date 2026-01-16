//! # D-Bus Client Module
//!
//! Provides a high-level client interface for communicating with the wallpaper daemon.
//! Used by both the GUI application and system tray.
//!
//! ## Usage
//!
//! ```ignore
//! let client = WallpaperClient::connect().await?;
//! let wallpaper = client.fetch_wallpaper(true).await?;
//! println!("Applied: {}", wallpaper.path);
//! ```
//!
//! ## Signal Handling
//!
//! The client can subscribe to daemon signals for real-time updates:
//!
//! ```ignore
//! let mut stream = client.subscribe_wallpaper_changed().await?;
//! while let Some(signal) = stream.next().await {
//!     let args = signal.args()?;
//!     println!("Wallpaper changed: {}", args.path);
//! }
//! ```

// These methods are part of the public API for future GUI integration
#![allow(dead_code)]

use zbus::{proxy, Connection};

use crate::daemon::{SERVICE_NAME, WallpaperInfo};

/// D-Bus proxy for the wallpaper service
#[proxy(
    interface = "org.cosmicbing.Wallpaper1",
    default_service = "org.cosmicbing.Wallpaper1",
    default_path = "/org/cosmicbing/Wallpaper1"
)]
trait WallpaperService {
    /// Fetch today's wallpaper, optionally apply it
    async fn fetch_wallpaper(&self, apply: bool) -> zbus::Result<WallpaperInfo>;

    /// Apply a specific wallpaper by path
    async fn apply_wallpaper(&self, path: &str) -> zbus::Result<()>;

    /// Get current configuration as JSON
    async fn get_config(&self) -> zbus::Result<String>;

    /// Get the current Bing market code
    async fn get_market(&self) -> zbus::Result<String>;

    /// Set the Bing regional market
    async fn set_market(&self, market: &str) -> zbus::Result<()>;

    /// Get the wallpaper directory path
    async fn get_wallpaper_dir(&self) -> zbus::Result<String>;

    /// Check if auto-update timer is enabled
    async fn get_timer_enabled(&self) -> zbus::Result<bool>;

    /// Enable or disable the auto-update timer
    async fn set_timer_enabled(&self, enabled: bool) -> zbus::Result<()>;

    /// Get the next scheduled timer run time
    async fn get_timer_next_run(&self) -> zbus::Result<String>;

    /// Get list of downloaded wallpapers
    async fn get_history(&self) -> zbus::Result<Vec<WallpaperInfo>>;

    /// Delete a wallpaper from history
    async fn delete_wallpaper(&self, path: &str) -> zbus::Result<()>;

    // === Signals ===

    /// Signal emitted when the wallpaper changes
    #[zbus(signal)]
    async fn wallpaper_changed(&self, path: String, title: String) -> zbus::Result<()>;

    /// Signal emitted when timer state changes
    #[zbus(signal)]
    async fn timer_state_changed(&self, enabled: bool) -> zbus::Result<()>;

    /// Signal emitted during fetch operations
    #[zbus(signal)]
    async fn fetch_progress(&self, state: String, message: String) -> zbus::Result<()>;
}

/// High-level client for the wallpaper daemon
pub struct WallpaperClient {
    proxy: WallpaperServiceProxy<'static>,
}

impl WallpaperClient {
    /// Connect to the wallpaper daemon
    ///
    /// Returns an error if the daemon is not running
    pub async fn connect() -> zbus::Result<Self> {
        let connection = Connection::session().await?;
        let proxy = WallpaperServiceProxy::new(&connection).await?;
        Ok(Self { proxy })
    }

    /// Try to connect, starting the daemon if necessary
    ///
    /// First attempts a direct connection. If that fails, tries to start
    /// the daemon via D-Bus activation.
    pub async fn connect_or_start() -> zbus::Result<Self> {
        // Try direct connection first
        if let Ok(client) = Self::connect().await {
            return Ok(client);
        }

        // D-Bus should auto-activate the service if .service file is installed
        // Just retry the connection - D-Bus will start the daemon
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        Self::connect().await
    }

    /// Fetch today's wallpaper from Bing
    ///
    /// # Arguments
    /// * `apply` - If true, also apply the wallpaper after downloading
    pub async fn fetch_wallpaper(&self, apply: bool) -> zbus::Result<WallpaperInfo> {
        self.proxy.fetch_wallpaper(apply).await
    }

    /// Apply a specific wallpaper by path
    pub async fn apply_wallpaper(&self, path: &str) -> zbus::Result<()> {
        self.proxy.apply_wallpaper(path).await
    }

    /// Get current configuration as JSON
    pub async fn get_config(&self) -> zbus::Result<String> {
        self.proxy.get_config().await
    }

    /// Get the current Bing market code
    pub async fn get_market(&self) -> zbus::Result<String> {
        self.proxy.get_market().await
    }

    /// Set the Bing regional market
    pub async fn set_market(&self, market: &str) -> zbus::Result<()> {
        self.proxy.set_market(market).await
    }

    /// Get the wallpaper directory path
    pub async fn get_wallpaper_dir(&self) -> zbus::Result<String> {
        self.proxy.get_wallpaper_dir().await
    }

    /// Check if auto-update timer is enabled
    pub async fn get_timer_enabled(&self) -> zbus::Result<bool> {
        self.proxy.get_timer_enabled().await
    }

    /// Enable or disable the auto-update timer
    pub async fn set_timer_enabled(&self, enabled: bool) -> zbus::Result<()> {
        self.proxy.set_timer_enabled(enabled).await
    }

    /// Get the next scheduled timer run time
    ///
    /// Returns empty string if timer is not active
    pub async fn get_timer_next_run(&self) -> zbus::Result<String> {
        self.proxy.get_timer_next_run().await
    }

    /// Get list of downloaded wallpapers
    pub async fn get_history(&self) -> zbus::Result<Vec<WallpaperInfo>> {
        self.proxy.get_history().await
    }

    /// Delete a wallpaper from history
    pub async fn delete_wallpaper(&self, path: &str) -> zbus::Result<()> {
        self.proxy.delete_wallpaper(path).await
    }

    /// Subscribe to wallpaper changed signals
    pub async fn subscribe_wallpaper_changed(&self) -> zbus::Result<WallpaperChangedStream<'static>> {
        self.proxy.receive_wallpaper_changed().await
    }

    /// Subscribe to timer state changed signals
    pub async fn subscribe_timer_state_changed(&self) -> zbus::Result<TimerStateChangedStream<'static>> {
        self.proxy.receive_timer_state_changed().await
    }

    /// Subscribe to fetch progress signals
    pub async fn subscribe_fetch_progress(&self) -> zbus::Result<FetchProgressStream<'static>> {
        self.proxy.receive_fetch_progress().await
    }

    /// Get the underlying proxy for advanced operations
    pub fn proxy(&self) -> &WallpaperServiceProxy<'static> {
        &self.proxy
    }
}

/// Check if the daemon is available (service is registered on D-Bus)
pub async fn is_daemon_available() -> bool {
    if let Ok(connection) = Connection::session().await {
        connection
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "NameHasOwner",
                &SERVICE_NAME,
            )
            .await
            .and_then(|reply| reply.body().deserialize::<bool>())
            .unwrap_or(false)
    } else {
        false
    }
}

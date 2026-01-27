//! Internal Timer Module
//!
//! Provides an internal timer for scheduled wallpaper updates, replacing
//! the systemd timer for Flatpak compatibility.
//!
//! ## Timer Behavior
//! - Runs daily at 08:00 local time
//! - Catches up on missed runs after boot (with 5-minute initial delay)
//! - Random delay up to 5 minutes to avoid API hammering
//! - Persists enabled state and last run time to config

use chrono::{DateTime, Duration, Local, NaiveTime};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use crate::config::app_config_dir;

/// Default scheduled run time (08:00 local time)
const SCHEDULED_HOUR: u32 = 8;
const SCHEDULED_MINUTE: u32 = 0;

/// Delay after boot before running catch-up (seconds)
const BOOT_DELAY_SECS: u64 = 300; // 5 minutes

/// Maximum random delay to spread API load (seconds)
const MAX_RANDOM_DELAY_SECS: u64 = 300; // 5 minutes

/// Timer state persisted to disk
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TimerState {
    /// Whether the timer is enabled
    pub enabled: bool,
    /// Last successful fetch time (ISO 8601)
    #[serde(default)]
    pub last_fetch: Option<String>,
}

impl TimerState {
    /// Get the path to the timer state file
    fn state_path() -> Option<std::path::PathBuf> {
        app_config_dir().map(|p| p.join("timer_state.json"))
    }

    /// Load timer state from disk
    pub fn load() -> Self {
        Self::state_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    /// Save timer state to disk
    pub fn save(&self) -> Result<(), String> {
        let path = Self::state_path()
            .ok_or("Could not determine state path")?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create state dir: {}", e))?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write state: {}", e))?;

        Ok(())
    }

    /// Get the last fetch time as DateTime
    pub fn last_fetch_time(&self) -> Option<DateTime<Local>> {
        self.last_fetch.as_ref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&Local))
        })
    }

    /// Update the last fetch time
    pub fn set_last_fetch(&mut self, time: DateTime<Local>) {
        self.last_fetch = Some(time.to_rfc3339());
    }
}

/// Internal timer that replaces systemd timer for Flatpak compatibility
#[derive(Debug)]
pub struct InternalTimer {
    /// Whether the timer is currently enabled
    enabled: Arc<AtomicBool>,
    /// Next scheduled run time
    next_run: Arc<RwLock<Option<DateTime<Local>>>>,
    /// Background task handle (not cloneable, so wrapped in Option)
    handle: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl Clone for InternalTimer {
    fn clone(&self) -> Self {
        Self {
            enabled: self.enabled.clone(),
            next_run: self.next_run.clone(),
            handle: std::sync::Mutex::new(None), // Handle is not cloned
        }
    }
}

impl InternalTimer {
    /// Create a new internal timer
    pub fn new() -> Self {
        let state = TimerState::load();
        Self {
            enabled: Arc::new(AtomicBool::new(state.enabled)),
            next_run: Arc::new(RwLock::new(None)),
            handle: std::sync::Mutex::new(None),
        }
    }

    /// Start the timer with a callback channel
    ///
    /// When the timer fires, a message is sent on the returned receiver.
    pub fn start(&self) -> tokio::sync::mpsc::Receiver<()> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);

        let enabled = self.enabled.clone();
        let next_run = self.next_run.clone();

        let handle = tokio::spawn(async move {
            // Initial boot delay
            let state = TimerState::load();
            let needs_catchup = check_needs_catchup(&state);

            if needs_catchup && enabled.load(Ordering::SeqCst) {
                // Wait boot delay before catch-up
                tokio::time::sleep(std::time::Duration::from_secs(BOOT_DELAY_SECS)).await;

                // Add random delay
                let random_delay = rand_delay();
                tokio::time::sleep(std::time::Duration::from_secs(random_delay)).await;

                // Fire the callback for catch-up
                if enabled.load(Ordering::SeqCst) {
                    let _ = tx.send(()).await;
                }
            }

            // Main timer loop
            loop {
                if !enabled.load(Ordering::SeqCst) {
                    // Timer disabled, just sleep and check again
                    *next_run.write().await = None;
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    continue;
                }

                // Calculate next run time
                let next = calculate_next_run();
                *next_run.write().await = Some(next);

                // Calculate duration until next run
                let now = Local::now();
                let until_next = next.signed_duration_since(now);

                if until_next.num_seconds() > 0 {
                    // Sleep until next scheduled time (check every minute for enable state)
                    let sleep_secs = until_next.num_seconds().min(60) as u64;
                    tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
                } else {
                    // Time to run!
                    // Add random delay to spread API load
                    let random_delay = rand_delay();
                    tokio::time::sleep(std::time::Duration::from_secs(random_delay)).await;

                    // Fire the callback
                    if enabled.load(Ordering::SeqCst) {
                        let _ = tx.send(()).await;
                    }

                    // Sleep a bit to avoid immediate re-trigger
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                }
            }
        });

        // Store the handle
        if let Ok(mut guard) = self.handle.lock() {
            *guard = Some(handle);
        }

        rx
    }

    /// Stop the timer background task
    pub fn stop(&self) {
        if let Ok(mut guard) = self.handle.lock() {
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }
    }

    /// Set whether the timer is enabled
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);

        // Persist to state file
        let mut state = TimerState::load();
        state.enabled = enabled;
        let _ = state.save();
    }

    /// Check if the timer is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Get the next scheduled run time
    pub async fn next_run(&self) -> Option<DateTime<Local>> {
        *self.next_run.read().await
    }

    /// Get the next run time formatted as a string
    pub async fn next_run_string(&self) -> String {
        if !self.is_enabled() {
            return String::new();
        }

        match self.next_run().await {
            Some(dt) => dt.format("%a %b %d %H:%M").to_string(),
            None => "Scheduled".to_string(),
        }
    }

    /// Record a successful fetch
    pub fn record_fetch(&self) {
        let mut state = TimerState::load();
        state.set_last_fetch(Local::now());
        let _ = state.save();
    }
}

impl Default for InternalTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for InternalTimer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Calculate the next scheduled run time (08:00 local time)
fn calculate_next_run() -> DateTime<Local> {
    let now = Local::now();
    let today_run = now.date_naive().and_time(
        NaiveTime::from_hms_opt(SCHEDULED_HOUR, SCHEDULED_MINUTE, 0).unwrap()
    );
    let today_run = today_run.and_local_timezone(Local).unwrap();

    if now < today_run {
        // Today's run hasn't happened yet
        today_run
    } else {
        // Schedule for tomorrow
        today_run + Duration::days(1)
    }
}

/// Check if we need to catch up on a missed run
fn check_needs_catchup(state: &TimerState) -> bool {
    let now = Local::now();
    let today_run_time = NaiveTime::from_hms_opt(SCHEDULED_HOUR, SCHEDULED_MINUTE, 0).unwrap();

    // Has today's scheduled time passed?
    let now_time = now.time();
    if now_time < today_run_time {
        // Today's run hasn't happened yet, no catch-up needed
        return false;
    }

    // Check if we already ran today
    if let Some(last_fetch) = state.last_fetch_time() {
        let last_date = last_fetch.date_naive();
        let today = now.date_naive();
        if last_date >= today {
            // Already ran today
            return false;
        }
    }

    // We missed today's run
    true
}

/// Generate a random delay (0 to MAX_RANDOM_DELAY_SECS)
fn rand_delay() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Simple random using system time
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);

    nanos as u64 % MAX_RANDOM_DELAY_SECS
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn test_calculate_next_run() {
        let next = calculate_next_run();
        let now = Local::now();

        // Next run should be in the future or at scheduled hour
        assert!(next > now || next.hour() == SCHEDULED_HOUR);
    }

    #[test]
    fn test_timer_state_default() {
        let state = TimerState::default();
        assert!(!state.enabled);
        assert!(state.last_fetch.is_none());
    }
}

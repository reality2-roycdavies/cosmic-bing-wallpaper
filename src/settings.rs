//! # Settings Window Module
//!
//! Implements the full settings/management window using the libcosmic toolkit,
//! following the Model-View-Update (MVU) architecture pattern.
//!
//! This window is launched via `--settings` from the panel applet.
//! It communicates with the applet via D-Bus for timer operations.
//!
//! ## Features
//! - Preview today's Bing wallpaper
//! - Browse and apply previously downloaded wallpapers
//! - Select regional Bing market
//! - Enable/disable daily auto-update timer
//! - Delete old wallpapers

// --- COSMIC framework imports ---
use cosmic::app::Core;                  // Core app state provided by the framework
use cosmic::iced::{Length, ContentFit}; // Length = sizing, ContentFit = image scaling
use cosmic::widget::{                   // UI building blocks
    self, button, column, container, row, text,
    dropdown, scrollable, settings, toggler,
};
use cosmic::{Action, Application, Element, Task}; // Core traits and types
use std::path::PathBuf;                 // Filesystem path type

// --- Internal module imports ---
use crate::bing::{BingImage, fetch_bing_image_info, download_image}; // Bing API client
use crate::config::{Config, MARKETS};   // User config and market list
use crate::dbus_client::WallpaperClient; // D-Bus client to talk to the panel applet
use crate::service::{cleanup_old_wallpapers, extract_date_from_filename}; // Shared utilities

/// Unique application identifier for the settings window.
/// Uses a different ID from the applet so COSMIC treats them as separate apps.
const APP_ID: &str = "io.github.reality2_roycdavies.cosmic-bing-wallpaper.settings";

/// The main settings window application state.
///
/// This is a full COSMIC window (not a panel applet) that provides:
/// - Image preview of today's Bing wallpaper
/// - Fetch and apply controls
/// - Regional market selection (which country's Bing image to use)
/// - Timer enable/disable
/// - Wallpaper history browsing with apply/delete
///
/// The settings window communicates with the panel applet via D-Bus
/// for timer operations (the applet owns the timer).
pub struct SettingsApp {
    /// Core COSMIC framework state (window management, theming, etc.)
    core: Core,
    /// User configuration loaded from ~/.config/cosmic-bing-wallpaper/config.json
    config: Config,
    /// Metadata for the currently fetched Bing image (title, copyright, URL)
    current_image: Option<BingImage>,
    /// Local filesystem path to the downloaded image file (for preview display)
    image_path: Option<String>,
    /// Status message displayed at the bottom of the window (e.g., "Ready", "Error: ...")
    status_message: String,
    /// True while an async operation (fetch/apply) is running — disables buttons to prevent double-clicks
    is_loading: bool,
    /// List of previously downloaded wallpaper files found in the wallpaper directory
    history: Vec<HistoryItem>,
    /// Index of the currently selected market in the dropdown (maps to MARKETS array)
    selected_market_idx: usize,
    /// Which view/screen is currently displayed (Main or History)
    view_mode: ViewMode,
    /// Pre-computed list of market display names for the dropdown widget
    market_names: Vec<String>,
    /// Current status of the auto-update timer (checked via D-Bus every 5 seconds)
    timer_status: TimerStatus,
    /// Path of a wallpaper the user wants to delete (shown with confirm/cancel buttons)
    pending_delete: Option<PathBuf>,
}

/// Represents a single downloaded wallpaper file in the history list.
#[derive(Debug, Clone)]
pub struct HistoryItem {
    /// Full filesystem path to the image file
    pub path: PathBuf,
    /// Just the filename (e.g., "bing-en-US-2026-02-05.jpg")
    pub filename: String,
    /// Date extracted from the filename (e.g., "2026-02-05")
    pub date: String,
}

/// Which screen/view is currently displayed in the settings window.
/// The app switches between these when the user clicks "History" or the back button.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ViewMode {
    /// Main view: image preview, fetch button, market selection, timer toggle
    #[default]
    Main,
    /// History view: scrollable list of all downloaded wallpapers with apply/delete
    History,
}

/// Represents the current state of the daily auto-update timer.
/// This is checked every 5 seconds by querying the panel applet via D-Bus.
#[derive(Debug, Clone, Default)]
pub enum TimerStatus {
    /// Initial state: haven't checked yet (shown as "Checking...")
    #[default]
    Checking,
    /// Timer is disabled (shown as "Disabled" with toggle off)
    NotInstalled,
    /// Timer is enabled (shown with the next scheduled run time)
    Installed { next_run: String },
    /// Something went wrong checking the timer (shown as "Error: ...")
    Error(String),
}

/// All possible messages (events) for the settings window.
///
/// Like in applet.rs, this follows the Model-View-Update (MVU) pattern.
/// Each user action or async result becomes a Message that flows through `update()`.
///
/// The fetch workflow is a multi-step async chain:
/// FetchToday → FetchedImageInfo → DownloadedImage → AppliedWallpaper
/// Each step triggers an async Task, and the result comes back as the next Message.
#[derive(Debug, Clone)]
pub enum Message {
    // === Image Fetching (multi-step async chain) ===
    /// User clicked "Fetch Today's Wallpaper" — starts the fetch pipeline
    FetchToday,
    /// Step 1 result: Got image metadata from Bing API (or error)
    FetchedImageInfo(Result<BingImage, String>),
    /// Step 2 result: Image downloaded to disk (path) or error
    DownloadedImage(Result<String, String>),

    // === Wallpaper Application ===
    /// User clicked "Apply" on a history item — apply that wallpaper
    ApplyHistoryWallpaper(PathBuf),
    /// Apply operation completed (success or error)
    AppliedWallpaper(Result<(), String>),

    // === UI Navigation ===
    /// User selected a different market from the dropdown (index into MARKETS array)
    MarketSelected(usize),
    /// User clicked "History" button — switch to history view
    ShowHistory,
    /// User clicked back button — switch to main view
    ShowMain,
    /// User clicked refresh in history view — rescan wallpaper directory
    RefreshHistory,
    /// User clicked "Delete" on a history item — show confirmation buttons
    RequestDeleteHistoryItem(PathBuf),
    /// User confirmed deletion — actually delete the file
    ConfirmDeleteHistoryItem,
    /// User cancelled deletion — hide confirmation buttons
    CancelDeleteHistoryItem,

    // === Timer Management (via D-Bus to applet) ===
    /// Periodic check: query the applet for current timer state
    CheckTimerStatus,
    /// Timer status check result
    TimerStatusChecked(TimerStatus),
    /// User toggled timer ON — tell the applet to enable it
    InstallTimer,
    /// Timer enable result
    TimerInstalled(Result<(), String>),
    /// User toggled timer OFF — tell the applet to disable it
    UninstallTimer,
    /// Timer disable result
    TimerUninstalled(Result<(), String>),

    // === State Sync (startup) ===
    /// On startup: ask the applet what wallpaper is currently applied
    SyncCurrentWallpaper,
    /// Got the current wallpaper path from the applet (or None if not available)
    CurrentWallpaperSynced(Option<String>),
}

/// COSMIC Application implementation for the settings window.
///
/// Unlike the applet (which uses SingleThreadExecutor for panel integration),
/// the settings window uses the Default executor since it's a full standalone window.
impl Application for SettingsApp {
    /// Default executor supports multi-threaded async operations (needed for HTTP requests)
    type Executor = cosmic::executor::Default;
    /// No startup flags needed
    type Flags = ();
    /// Our Message enum
    type Message = Message;

    /// Must be unique — different from the applet's APP_ID
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    /// Called once when the settings window opens.
    ///
    /// Sets up initial state and triggers startup tasks:
    /// 1. Check timer status via D-Bus
    /// 2. Sync current wallpaper path from the applet
    /// 3. Optionally auto-fetch today's wallpaper (if timer is enabled and fetch_on_startup is true)
    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Action<Self::Message>>) {
        let config = Config::load();

        // Find which market in the dropdown matches the user's saved config.
        // position() returns the index, or 0 (first item) as fallback.
        let selected_market_idx = MARKETS
            .iter()
            .position(|m| m.code == config.market)
            .unwrap_or(0);

        // Scan the wallpaper directory for existing downloaded images
        let history = scan_history(&config.wallpaper_dir);
        // Pre-compute display names for the market dropdown widget
        let market_names: Vec<String> = MARKETS.iter().map(|m| m.name.to_string()).collect();

        let app = Self {
            core,
            config,
            current_image: None,
            image_path: None,
            status_message: "Ready".to_string(),
            is_loading: false,
            history,
            selected_market_idx,
            view_mode: ViewMode::Main,
            market_names,
            timer_status: TimerStatus::Checking,
            pending_delete: None,
        };

        // Schedule startup tasks that run immediately after the window opens.
        // Task::perform runs an async operation and converts the result to a Message.
        let timer_task = Task::perform(async {}, |_| Action::App(Message::CheckTimerStatus));
        let sync_task = Task::perform(async {}, |_| Action::App(Message::SyncCurrentWallpaper));

        // Auto-fetch on startup if both timer is enabled AND fetch_on_startup is configured
        let timer_enabled = crate::timer::TimerState::load().enabled;
        if timer_enabled && app.config.fetch_on_startup {
            let fetch_task = Task::perform(async {}, |_| Action::App(Message::FetchToday));
            // Task::batch runs multiple tasks concurrently
            (app, Task::batch([sync_task, fetch_task, timer_task]))
        } else {
            (app, Task::batch([sync_task, timer_task]))
        }
    }

    /// No custom header elements needed
    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        vec![]
    }

    /// Renders the appropriate view based on the current ViewMode
    fn view(&self) -> Element<'_, Self::Message> {
        match self.view_mode {
            ViewMode::Main => self.view_main(),
            ViewMode::History => self.view_history(),
        }
    }

    /// Background subscription: check timer status every 5 seconds.
    /// This keeps the timer toggle in sync with the applet's actual state.
    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        cosmic::iced::time::every(std::time::Duration::from_secs(5))
            .map(|_| Message::CheckTimerStatus)
    }

    /// Handles all Messages — the central state update function.
    ///
    /// Many handlers return a Task that starts an async operation.
    /// When the operation completes, it produces another Message with the result.
    /// This creates async "chains" like: FetchToday → FetchedImageInfo → DownloadedImage → AppliedWallpaper
    fn update(&mut self, message: Self::Message) -> Task<Action<Self::Message>> {
        match message {
            // --- Fetch pipeline: step 1 of 4 ---
            // User clicked "Fetch Today's Wallpaper"
            Message::FetchToday => {
                self.status_message = "Fetching image info...".to_string();
                self.is_loading = true;
                let market = self.config.market.clone();

                // Start async operation: call the Bing API
                // When done, the result becomes a FetchedImageInfo message
                Task::perform(
                    async move { fetch_bing_image_info(&market).await },
                    |result| Action::App(Message::FetchedImageInfo(result)),
                )
            }

            // --- Fetch pipeline: step 2 of 4 ---
            // Got image metadata from Bing API
            Message::FetchedImageInfo(result) => {
                match result {
                    Ok(image) => {
                        // Save image info for display (title, copyright)
                        self.current_image = Some(image.clone());
                        self.status_message = "Downloading image...".to_string();
                        let dir = self.config.wallpaper_dir.clone();
                        let market = self.config.market.clone();

                        // Start async operation: download the actual image file
                        Task::perform(
                            async move { download_image(&image, &dir, &market).await },
                            |result| Action::App(Message::DownloadedImage(result)),
                        )
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {e}");
                        self.is_loading = false;
                        Task::none()
                    }
                }
            }

            // --- Fetch pipeline: step 3 of 4 ---
            // Image downloaded to disk
            Message::DownloadedImage(result) => {
                match result {
                    Ok(path) => {
                        // Store the path so the preview widget can display the image
                        self.image_path = Some(path.clone());

                        // Clean up old wallpapers beyond the keep_days limit
                        let deleted = cleanup_old_wallpapers(&self.config.wallpaper_dir, self.config.keep_days);
                        if deleted > 0 {
                            self.status_message = format!(
                                "Downloaded ({deleted} old cleaned up). Applying...",
                            );
                        } else {
                            self.status_message = "Downloaded. Applying wallpaper...".to_string();
                        }

                        // Refresh history to include the newly downloaded image
                        self.history = scan_history(&self.config.wallpaper_dir);

                        // Start async operation: apply the wallpaper to COSMIC desktop
                        Task::perform(
                            async move { apply_cosmic_wallpaper(&path).await },
                            |result| Action::App(Message::AppliedWallpaper(result)),
                        )
                    }
                    Err(e) => {
                        self.is_loading = false;
                        self.status_message = format!("Error: {e}");
                        Task::none()
                    }
                }
            }

            // User clicked "Apply" on a history item
            Message::ApplyHistoryWallpaper(path) => {
                self.apply_wallpaper_from_path(path.to_string_lossy().to_string())
            }

            // --- Fetch pipeline: step 4 of 4 ---
            // Wallpaper apply completed (success or error)
            Message::AppliedWallpaper(result) => {
                self.is_loading = false;
                match result {
                    Ok(()) => {
                        self.status_message = "Wallpaper applied!".to_string();
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {e}");
                    }
                }
                Task::none()
            }

            // User selected a different market from the dropdown
            Message::MarketSelected(idx) => {
                if idx < MARKETS.len() {
                    self.selected_market_idx = idx;
                    // Update config and save to disk immediately
                    self.config.market = MARKETS[idx].code.to_string();
                    let _ = self.config.save();
                }
                Task::none()
            }

            // --- View navigation ---
            Message::ShowHistory => {
                self.view_mode = ViewMode::History;
                self.status_message = String::new();
                // Rescan directory to show the latest files
                self.history = scan_history(&self.config.wallpaper_dir);
                Task::none()
            }

            Message::ShowMain => {
                self.view_mode = ViewMode::Main;
                self.status_message = "Ready".to_string();
                Task::none()
            }

            Message::RefreshHistory => {
                self.history = scan_history(&self.config.wallpaper_dir);
                Task::none()
            }

            // --- Delete confirmation flow ---
            // Step 1: User clicked "Delete" — show confirmation
            Message::RequestDeleteHistoryItem(path) => {
                let filename = path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("this image")
                    .to_string();
                self.pending_delete = Some(path);
                self.status_message = format!("Delete {filename}? Click 'Confirm' to delete or 'Cancel' to keep.");
                Task::none()
            }

            // Step 2a: User confirmed — delete the file
            Message::ConfirmDeleteHistoryItem => {
                if let Some(path) = self.pending_delete.take() {
                    if let Err(e) = std::fs::remove_file(&path) {
                        self.status_message = format!("Failed to delete: {e}");
                    } else {
                        // Rescan to update the list after deletion
                        self.history = scan_history(&self.config.wallpaper_dir);
                        self.status_message = "Image deleted".to_string();
                    }
                }
                Task::none()
            }

            // Step 2b: User cancelled — hide confirmation buttons
            Message::CancelDeleteHistoryItem => {
                self.pending_delete = None;
                self.status_message = "Delete cancelled".to_string();
                Task::none()
            }

            // --- Timer management via D-Bus ---

            // Periodic timer status check (every 5 seconds)
            Message::CheckTimerStatus => {
                Task::perform(
                    async { check_timer_status().await },
                    |status| Action::App(Message::TimerStatusChecked(status)),
                )
            }

            Message::TimerStatusChecked(status) => {
                self.timer_status = status;
                Task::none()
            }

            // User toggled the timer ON
            Message::InstallTimer => {
                self.status_message = "Enabling Daily Update...".to_string();
                Task::perform(
                    async { install_timer().await },
                    |result| Action::App(Message::TimerInstalled(result)),
                )
            }

            Message::TimerInstalled(result) => {
                match result {
                    Ok(()) => {
                        self.status_message = "Daily Update enabled!".to_string();
                    }
                    Err(e) => {
                        self.status_message = format!("Failed to enable Daily Update: {e}");
                    }
                }
                // Refresh timer status to show the new state
                Task::perform(
                    async { check_timer_status().await },
                    |status| Action::App(Message::TimerStatusChecked(status)),
                )
            }

            // User toggled the timer OFF
            Message::UninstallTimer => {
                self.status_message = "Disabling Daily Update...".to_string();
                Task::perform(
                    async { uninstall_timer().await },
                    |result| Action::App(Message::TimerUninstalled(result)),
                )
            }

            Message::TimerUninstalled(result) => {
                match result {
                    Ok(()) => {
                        self.status_message = "Daily Update disabled.".to_string();
                    }
                    Err(e) => {
                        self.status_message = format!("Failed to disable Daily Update: {e}");
                    }
                }
                // Refresh timer status to show the new state
                Task::perform(
                    async { check_timer_status().await },
                    |status| Action::App(Message::TimerStatusChecked(status)),
                )
            }

            // --- Startup sync: get current wallpaper from applet ---
            Message::SyncCurrentWallpaper => {
                Task::perform(
                    async {
                        // Try to connect to the applet via D-Bus and get the current path
                        match WallpaperClient::connect().await {
                            Ok(client) => {
                                match client.get_current_wallpaper_path().await {
                                    Ok(path) if !path.is_empty() => Some(path),
                                    _ => None,
                                }
                            }
                            Err(_) => None, // Applet not running — that's OK
                        }
                    },
                    |path| Action::App(Message::CurrentWallpaperSynced(path)),
                )
            }

            Message::CurrentWallpaperSynced(path) => {
                // Only set the preview image if we don't already have one
                // (avoids overwriting a freshly fetched image)
                if let Some(p) = path {
                    if self.image_path.is_none() {
                        self.image_path = Some(p);
                    }
                }
                Task::none()
            }
        }
    }
}

/// Helper methods for view building and wallpaper operations
impl SettingsApp {
    /// Start applying a wallpaper from a given file path.
    /// Used by both the main view fetch flow and the history "Apply" button.
    fn apply_wallpaper_from_path(&mut self, path: String) -> Task<Action<Message>> {
        self.status_message = "Applying wallpaper...".to_string();
        self.is_loading = true;

        Task::perform(
            async move { apply_cosmic_wallpaper(&path).await },
            |result| Action::App(Message::AppliedWallpaper(result)),
        )
    }

    /// Builds the main view with image preview, fetch button, and settings.
    ///
    /// Layout:
    /// ┌────────────────────────────────────┐
    /// │  Bing Daily Wallpaper (title)       │
    /// │ ┌────────────────────────────────┐ │
    /// │ │     [Image Preview]             │ │
    /// │ │  Title: Mountain Sunrise        │ │
    /// │ │  Copyright: © Photographer      │ │
    /// │ │  Status: Ready                  │ │
    /// │ └────────────────────────────────┘ │
    /// │ ┌────────────────────────────────┐ │
    /// │ │  Region: [United States ▼]      │ │
    /// │ │  Daily Update:    (...) [toggle]│ │
    /// │ └────────────────────────────────┘ │
    /// │ ┌────────────────────────────────┐ │
    /// │ │  [Fetch Today's Wallpaper]      │ │
    /// │ │  [History]                      │ │
    /// │ └────────────────────────────────┘ │
    /// └────────────────────────────────────┘
    fn view_main(&self) -> Element<'_, Message> {
        // --- Image preview section ---
        // Show the wallpaper image if we have one, or a placeholder message
        let preview_content: Element<_> = if let Some(path) = &self.image_path {
            container(
                widget::image(path)
                    .content_fit(ContentFit::Contain)  // Scale to fit, maintain aspect ratio
                    .height(Length::Fixed(280.0))       // Fixed height for consistent layout
            )
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into()
        } else {
            // No image yet — show a helpful message
            container(text::body("Click 'Fetch Today's Wallpaper' to get started"))
                .padding(60)
                .width(Length::Fill)
                .center_x(Length::Fill)
                .into()
        };

        // Show image title and copyright, or em-dash (—) if no image has been fetched
        let image_title = self.current_image.as_ref()
            .map(|img| img.title.clone())
            .unwrap_or_else(|| "\u{2014}".to_string());   // \u{2014} = em-dash "—"

        let image_copyright = self.current_image.as_ref()
            .map(|img| img.copyright.clone())
            .unwrap_or_else(|| "\u{2014}".to_string());

        // --- Build the page layout using COSMIC settings widgets ---
        // settings::section() creates a card-style grouped section
        // settings::item() creates a label+widget row within a section
        let page_title = text::title1("Bing Daily Wallpaper");

        let wallpaper_section = settings::section()
            .title("Today's Wallpaper")
            .add(
                container(preview_content)
                    .width(Length::Fill)
                    .padding(12)
                    .class(cosmic::theme::Container::Card)
            )
            .add(
                row()
                    .spacing(16)
                    .push(text::body("Title"))
                    .push(cosmic::widget::horizontal_space())
                    .push(text::body(image_title))
            )
            .add(
                row()
                    .spacing(16)
                    .push(text::body("Copyright"))
                    .push(cosmic::widget::horizontal_space())
                    .push(text::caption(image_copyright))
            )
            .add(
                settings::item(
                    "Status",
                    text::caption(self.status_message.clone()),
                )
            );

        // --- Timer status for the settings section ---
        // Determine toggle state and description text from the current timer status
        let timer_enabled = matches!(&self.timer_status, TimerStatus::Installed { .. });
        let timer_description = match &self.timer_status {
            TimerStatus::Checking => "Checking...".to_string(),
            TimerStatus::NotInstalled => "Disabled".to_string(),
            TimerStatus::Installed { next_run } => format!("Next: {next_run}"),
            TimerStatus::Error(e) => format!("Error: {e}"),
        };

        let settings_section = settings::section()
            .title("Settings")
            .add(
                settings::item(
                    "Region",
                    dropdown(&self.market_names, Some(self.selected_market_idx), Message::MarketSelected)
                        .width(Length::Fixed(200.0)),
                )
            )
            .add(
                settings::flex_item(
                    "Daily Update",
                    row()
                        .spacing(12)
                        .align_y(cosmic::iced::Alignment::Center)
                        .push(text::caption(timer_description))
                        .push(
                            toggler(timer_enabled)
                                .on_toggle(|enabled| {
                                    if enabled {
                                        Message::InstallTimer
                                    } else {
                                        Message::UninstallTimer
                                    }
                                })
                        ),
                )
            );

        // --- Action buttons ---
        // on_press_maybe: passes None to disable the button while loading
        let fetch_btn = button::suggested("Fetch Today's Wallpaper")
            .on_press_maybe(if self.is_loading { None } else { Some(Message::FetchToday) });

        let history_btn = button::standard("History")
            .on_press(Message::ShowHistory);

        let actions_section = settings::section()
            .title("Actions")
            .add(
                settings::item_row(vec![
                    fetch_btn.into(),
                    history_btn.into(),
                ])
            );

        // --- Assemble the full page layout ---
        // settings::view_column creates a vertically stacked layout with proper spacing
        let content = settings::view_column(vec![
            page_title.into(),
            wallpaper_section.into(),
            settings_section.into(),
            actions_section.into(),
        ]);

        // Wrap in a scrollable container with max width for readability
        widget::scrollable(
            container(
                container(content)
                    .max_width(800)     // Don't stretch too wide on large monitors
            )
            .width(Length::Fill)
            .center_x(Length::Fill)      // Center the content horizontally
            .padding(16)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Builds the history view showing all downloaded wallpapers.
    ///
    /// Each wallpaper is shown as a card with:
    /// - Thumbnail preview (160x90)
    /// - Date and filename
    /// - "Apply" button to set it as wallpaper
    /// - "Delete" button with confirmation step
    fn view_history(&self) -> Element<'_, Message> {
        // --- Header with back button and refresh ---
        let title_row = row()
            .spacing(12)
            .align_y(cosmic::iced::Alignment::Center)
            .push(
                button::icon(widget::icon::from_name("go-previous-symbolic"))
                    .on_press(Message::ShowMain)   // Back arrow → return to main view
            )
            .push(text::title3("Downloaded Wallpapers"))
            .push(cosmic::widget::horizontal_space())  // Push refresh to the right
            .push(
                button::icon(widget::icon::from_name("view-refresh-symbolic"))
                    .on_press(Message::RefreshHistory)  // Rescan wallpaper directory
            );

        // --- History list ---
        let history_content: Element<_> = if self.history.is_empty() {
            container(text::body("No wallpapers downloaded yet"))
                .padding(40)
                .center_x(Length::Fill)
                .into()
        } else {
            let mut history_column = column().spacing(12).padding(10);

            // Build a card for each wallpaper in the history
            for item in &self.history {
                // Clone paths since they need to be moved into button closures
                let item_path = item.path.clone();
                let delete_path = item.path.clone();

                // Small thumbnail preview of the wallpaper
                let preview = widget::image(item.path.to_string_lossy().to_string())
                    .content_fit(ContentFit::Cover)   // Fill the area, crop if needed
                    .width(Length::Fixed(160.0))
                    .height(Length::Fixed(90.0));

                // Date and filename info
                let info = column()
                    .spacing(4)
                    .push(text::body(item.date.clone()))
                    .push(text::caption(item.filename.clone()));

                // "Apply" button — sets this wallpaper as the current desktop background
                let apply_btn = button::suggested("Apply")
                    .on_press(Message::ApplyHistoryWallpaper(item_path));

                // Delete button with two-step confirmation to prevent accidental deletion.
                // First click shows "Confirm" and "Cancel" buttons instead.
                let is_pending = self.pending_delete.as_ref() == Some(&item.path);
                let delete_btn: Element<_> = if is_pending {
                    // Show confirmation buttons for this specific item
                    row()
                        .spacing(8)
                        .push(button::destructive("Confirm").on_press(Message::ConfirmDeleteHistoryItem))
                        .push(button::standard("Cancel").on_press(Message::CancelDeleteHistoryItem))
                        .into()
                } else {
                    // Normal delete button
                    button::destructive("Delete")
                        .on_press(Message::RequestDeleteHistoryItem(delete_path))
                        .into()
                };

                // Assemble the row: [preview | info | spacer | apply | delete]
                let item_row = row()
                    .spacing(16)
                    .align_y(cosmic::iced::Alignment::Center)
                    .push(preview)
                    .push(info)
                    .push(cosmic::widget::horizontal_space())  // Push buttons to the right
                    .push(apply_btn)
                    .push(delete_btn);

                // Wrap in a card-style container
                let item_container = container(item_row)
                    .padding(12)
                    .class(cosmic::theme::Container::Card);

                history_column = history_column.push(item_container);
            }

            scrollable(history_column)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        let status = text::body(self.status_message.clone());

        let content = column()
            .spacing(16)
            .padding(20)
            .push(title_row)
            .push(widget::divider::horizontal::default())
            .push(history_content)
            .push(status);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

/// Scans the wallpaper directory for downloaded image files and returns them as HistoryItems.
///
/// This reads the directory, filters for image files (.jpg, .jpeg, .png),
/// extracts the date from each filename, and returns them sorted newest-first.
///
/// # Arguments
/// * `wallpaper_dir` - Path to the wallpaper storage directory (e.g., "~/Pictures/BingWallpapers")
fn scan_history(wallpaper_dir: &str) -> Vec<HistoryItem> {
    let dir = std::path::Path::new(wallpaper_dir);
    if !dir.exists() {
        return Vec::new();
    }

    let mut items: Vec<HistoryItem> = std::fs::read_dir(dir)
        .ok()
        .into_iter()       // Convert Option to Iterator (allows .flatten() to work)
        .flatten()         // Unwrap the Ok variants from the directory entries
        .filter_map(|entry| entry.ok())  // Skip any entries that failed to read
        .filter(|entry| {
            // Only include image files
            entry.path().extension()
                .map(|ext| ext == "jpg" || ext == "jpeg" || ext == "png")
                .unwrap_or(false)
        })
        .map(|entry| {
            let path = entry.path();
            let filename = path.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            // Extract date from filename like "bing-en-US-2026-02-05.jpg" → "2026-02-05"
            let date = extract_date_from_filename(&filename);

            HistoryItem { path, filename, date }
        })
        .collect();

    // Sort by date, newest first (reverse alphabetical works for YYYY-MM-DD format)
    items.sort_by(|a, b| b.date.cmp(&a.date));
    items
}

/// Checks the current timer status by querying the panel applet via D-Bus.
///
/// If the applet is running, we ask it directly for the timer state.
/// If the applet is not running (D-Bus connection fails), we fall back to
/// reading the timer state file from disk.
async fn check_timer_status() -> TimerStatus {
    match WallpaperClient::connect().await {
        Ok(client) => {
            // Applet is running — query via D-Bus
            match client.get_timer_enabled().await {
                Ok(enabled) => {
                    if enabled {
                        // Get the next scheduled run time for display
                        let next_run = match client.get_timer_next_run().await {
                            Ok(time) if !time.is_empty() => time,
                            _ => "Scheduled".to_string(),
                        };
                        TimerStatus::Installed { next_run }
                    } else {
                        TimerStatus::NotInstalled
                    }
                }
                Err(e) => TimerStatus::Error(format!("D-Bus error: {e}"))
            }
        }
        Err(_) => {
            // Applet not running — fall back to reading the state file directly.
            // This allows the settings window to show timer state even without the applet.
            let state = crate::timer::TimerState::load();
            if state.enabled {
                TimerStatus::Installed { next_run: "Applet not running".to_string() }
            } else {
                TimerStatus::NotInstalled
            }
        }
    }
}

/// Enables the daily auto-update timer via D-Bus (or falls back to state file).
///
/// Tries to tell the applet to enable the timer via D-Bus. If the applet
/// isn't running, writes directly to the timer state file so the setting
/// is picked up when the applet next starts.
async fn install_timer() -> Result<(), String> {
    match WallpaperClient::connect().await {
        Ok(client) => {
            client.set_timer_enabled(true).await
                .map_err(|e| format!("Failed to enable timer: {e}"))
        }
        Err(_) => {
            // Fallback: write directly to the state file
            let mut state = crate::timer::TimerState::load();
            state.enabled = true;
            state.save()?;
            Ok(())
        }
    }
}

/// Disables the daily auto-update timer via D-Bus (or falls back to state file).
async fn uninstall_timer() -> Result<(), String> {
    match WallpaperClient::connect().await {
        Ok(client) => {
            client.set_timer_enabled(false).await
                .map_err(|e| format!("Failed to disable timer: {e}"))
        }
        Err(_) => {
            // Fallback: write directly to the state file
            let mut state = crate::timer::TimerState::load();
            state.enabled = false;
            state.save()?;
            Ok(())
        }
    }
}

/// Applies a wallpaper to the COSMIC desktop.
///
/// This is an async wrapper around the sync `service::apply_cosmic_wallpaper()` function.
/// We use `spawn_blocking` to run the sync function on a thread pool thread,
/// preventing it from blocking the async executor (which would freeze the UI).
async fn apply_cosmic_wallpaper(image_path: &str) -> Result<(), String> {
    let path = image_path.to_string();
    tokio::task::spawn_blocking(move || {
        crate::service::apply_cosmic_wallpaper(&path)
    })
    .await
    .map_err(|e| format!("Task join error: {e}"))?
}

/// Public async wrapper for headless wallpaper application.
/// Used by main.rs in --fetch mode (no GUI, just fetch and apply).
pub async fn apply_wallpaper_headless(image_path: &str) -> Result<(), String> {
    apply_cosmic_wallpaper(image_path).await
}

/// Entry point for running the settings window.
///
/// Called from main.rs when `--settings` is passed on the command line.
/// Sets up the window with default size and minimum dimensions, then
/// runs the COSMIC application event loop.
pub fn run_settings() -> cosmic::iced::Result {
    let settings = cosmic::app::Settings::default()
        .size(cosmic::iced::Size::new(850.0, 750.0))    // Default window size
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(600.0)    // Prevent window from being too narrow
                .min_height(550.0)   // Prevent window from being too short
        );
    cosmic::app::run::<SettingsApp>(settings, ())
}

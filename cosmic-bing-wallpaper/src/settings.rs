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

use cosmic::app::Core;
use cosmic::iced::{Length, ContentFit};
use cosmic::widget::{self, button, column, container, row, text, dropdown, scrollable, settings, toggler};
use cosmic::{Action, Application, Element, Task};
use std::path::PathBuf;

use crate::bing::{BingImage, fetch_bing_image_info, download_image};
use crate::config::{Config, MARKETS};
use crate::dbus_client::WallpaperClient;
use crate::service::{is_flatpak, cleanup_old_wallpapers, extract_date_from_filename};

/// Unique application identifier for the settings window.
const APP_ID: &str = "io.github.reality2_roycdavies.cosmic-bing-wallpaper.settings";

/// Main settings application state struct.
pub struct SettingsApp {
    /// COSMIC core state (window management, theming, etc.)
    core: Core,
    /// User configuration (market, wallpaper directory, etc.)
    config: Config,
    /// Metadata for the currently fetched Bing image
    current_image: Option<BingImage>,
    /// Local filesystem path to the downloaded image
    image_path: Option<String>,
    /// Status message displayed to the user
    status_message: String,
    /// True when an async operation is in progress (disables buttons)
    is_loading: bool,
    /// List of previously downloaded wallpapers
    history: Vec<HistoryItem>,
    /// Index of selected market in the dropdown
    selected_market_idx: usize,
    /// Current view (Main or History)
    view_mode: ViewMode,
    /// Pre-computed market names for dropdown
    market_names: Vec<String>,
    /// Current status of the auto-update timer
    timer_status: TimerStatus,
    /// Path of wallpaper pending deletion (for confirmation)
    pending_delete: Option<PathBuf>,
}

/// Represents a wallpaper in the download history.
#[derive(Debug, Clone)]
pub struct HistoryItem {
    pub path: PathBuf,
    pub filename: String,
    pub date: String,
}

/// Current view/screen of the application.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ViewMode {
    #[default]
    Main,
    History,
}

/// Status of the auto-update timer.
#[derive(Debug, Clone, Default)]
pub enum TimerStatus {
    #[default]
    Checking,
    NotInstalled,
    Installed { next_run: String },
    Error(String),
}

/// All possible messages for the settings window.
#[derive(Debug, Clone)]
pub enum Message {
    // === Image Fetching ===
    FetchToday,
    FetchedImageInfo(Result<BingImage, String>),
    DownloadedImage(Result<String, String>),

    // === Wallpaper Application ===
    ApplyHistoryWallpaper(PathBuf),
    AppliedWallpaper(Result<(), String>),

    // === UI Navigation ===
    MarketSelected(usize),
    ShowHistory,
    ShowMain,
    RefreshHistory,
    RequestDeleteHistoryItem(PathBuf),
    ConfirmDeleteHistoryItem,
    CancelDeleteHistoryItem,

    // === Timer Management ===
    CheckTimerStatus,
    TimerStatusChecked(TimerStatus),
    InstallTimer,
    TimerInstalled(Result<(), String>),
    UninstallTimer,
    TimerUninstalled(Result<(), String>),

    // === State Sync ===
    SyncCurrentWallpaper,
    CurrentWallpaperSynced(Option<String>),
}

impl Application for SettingsApp {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Action<Self::Message>>) {
        let config = Config::load();

        let selected_market_idx = MARKETS
            .iter()
            .position(|m| m.code == config.market)
            .unwrap_or(0);

        let history = scan_history(&config.wallpaper_dir);
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

        // Trigger startup actions
        let timer_task = Task::perform(async {}, |_| Action::App(Message::CheckTimerStatus));
        let sync_task = Task::perform(async {}, |_| Action::App(Message::SyncCurrentWallpaper));

        let timer_enabled = crate::timer::TimerState::load().enabled;
        if timer_enabled && app.config.fetch_on_startup {
            let fetch_task = Task::perform(async {}, |_| Action::App(Message::FetchToday));
            (app, Task::batch([sync_task, fetch_task, timer_task]))
        } else {
            (app, Task::batch([sync_task, timer_task]))
        }
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        vec![]
    }

    fn view(&self) -> Element<'_, Self::Message> {
        match self.view_mode {
            ViewMode::Main => self.view_main(),
            ViewMode::History => self.view_history(),
        }
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        cosmic::iced::time::every(std::time::Duration::from_secs(5))
            .map(|_| Message::CheckTimerStatus)
    }

    fn update(&mut self, message: Self::Message) -> Task<Action<Self::Message>> {
        match message {
            Message::FetchToday => {
                self.status_message = "Fetching image info...".to_string();
                self.is_loading = true;
                let market = self.config.market.clone();

                Task::perform(
                    async move { fetch_bing_image_info(&market).await },
                    |result| Action::App(Message::FetchedImageInfo(result)),
                )
            }

            Message::FetchedImageInfo(result) => {
                match result {
                    Ok(image) => {
                        self.current_image = Some(image.clone());
                        self.status_message = "Downloading image...".to_string();
                        let dir = self.config.wallpaper_dir.clone();
                        let market = self.config.market.clone();

                        Task::perform(
                            async move { download_image(&image, &dir, &market).await },
                            |result| Action::App(Message::DownloadedImage(result)),
                        )
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {}", e);
                        self.is_loading = false;
                        Task::none()
                    }
                }
            }

            Message::DownloadedImage(result) => {
                match result {
                    Ok(path) => {
                        self.image_path = Some(path.clone());

                        let deleted = cleanup_old_wallpapers(&self.config.wallpaper_dir, self.config.keep_days);
                        if deleted > 0 {
                            self.status_message = format!(
                                "Downloaded ({} old cleaned up). Applying...",
                                deleted
                            );
                        } else {
                            self.status_message = "Downloaded. Applying wallpaper...".to_string();
                        }

                        self.history = scan_history(&self.config.wallpaper_dir);

                        Task::perform(
                            async move { apply_cosmic_wallpaper(&path).await },
                            |result| Action::App(Message::AppliedWallpaper(result)),
                        )
                    }
                    Err(e) => {
                        self.is_loading = false;
                        self.status_message = format!("Error: {}", e);
                        Task::none()
                    }
                }
            }

            Message::ApplyHistoryWallpaper(path) => {
                self.apply_wallpaper_from_path(path.to_string_lossy().to_string())
            }

            Message::AppliedWallpaper(result) => {
                self.is_loading = false;
                match result {
                    Ok(()) => {
                        self.status_message = "Wallpaper applied!".to_string();
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {}", e);
                    }
                }
                Task::none()
            }

            Message::MarketSelected(idx) => {
                if idx < MARKETS.len() {
                    self.selected_market_idx = idx;
                    self.config.market = MARKETS[idx].code.to_string();
                    let _ = self.config.save();
                }
                Task::none()
            }

            Message::ShowHistory => {
                self.view_mode = ViewMode::History;
                self.status_message = String::new();
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

            Message::RequestDeleteHistoryItem(path) => {
                let filename = path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("this image")
                    .to_string();
                self.pending_delete = Some(path);
                self.status_message = format!("Delete {}? Click 'Confirm' to delete or 'Cancel' to keep.", filename);
                Task::none()
            }

            Message::ConfirmDeleteHistoryItem => {
                if let Some(path) = self.pending_delete.take() {
                    if let Err(e) = std::fs::remove_file(&path) {
                        self.status_message = format!("Failed to delete: {}", e);
                    } else {
                        self.history = scan_history(&self.config.wallpaper_dir);
                        self.status_message = "Image deleted".to_string();
                    }
                }
                Task::none()
            }

            Message::CancelDeleteHistoryItem => {
                self.pending_delete = None;
                self.status_message = "Delete cancelled".to_string();
                Task::none()
            }

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
                        self.status_message = format!("Failed to enable Daily Update: {}", e);
                    }
                }
                Task::perform(
                    async { check_timer_status().await },
                    |status| Action::App(Message::TimerStatusChecked(status)),
                )
            }

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
                        self.status_message = format!("Failed to disable Daily Update: {}", e);
                    }
                }
                Task::perform(
                    async { check_timer_status().await },
                    |status| Action::App(Message::TimerStatusChecked(status)),
                )
            }

            Message::SyncCurrentWallpaper => {
                Task::perform(
                    async {
                        match WallpaperClient::connect().await {
                            Ok(client) => {
                                match client.get_current_wallpaper_path().await {
                                    Ok(path) if !path.is_empty() => Some(path),
                                    _ => None,
                                }
                            }
                            Err(_) => None,
                        }
                    },
                    |path| Action::App(Message::CurrentWallpaperSynced(path)),
                )
            }

            Message::CurrentWallpaperSynced(path) => {
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

impl SettingsApp {
    fn apply_wallpaper_from_path(&mut self, path: String) -> Task<Action<Message>> {
        self.status_message = "Applying wallpaper...".to_string();
        self.is_loading = true;

        Task::perform(
            async move { apply_cosmic_wallpaper(&path).await },
            |result| Action::App(Message::AppliedWallpaper(result)),
        )
    }

    fn view_main(&self) -> Element<'_, Message> {
        let preview_content: Element<_> = if let Some(path) = &self.image_path {
            container(
                widget::image(path)
                    .content_fit(ContentFit::Contain)
                    .height(Length::Fixed(280.0))
            )
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into()
        } else {
            container(text::body("Click 'Fetch Today's Wallpaper' to get started"))
                .padding(60)
                .width(Length::Fill)
                .center_x(Length::Fill)
                .into()
        };

        let image_title = self.current_image.as_ref()
            .map(|img| img.title.clone())
            .unwrap_or_else(|| "\u{2014}".to_string());

        let image_copyright = self.current_image.as_ref()
            .map(|img| img.copyright.clone())
            .unwrap_or_else(|| "\u{2014}".to_string());

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

        let timer_enabled = matches!(&self.timer_status, TimerStatus::Installed { .. });
        let timer_description = match &self.timer_status {
            TimerStatus::Checking => "Checking...".to_string(),
            TimerStatus::NotInstalled => "Disabled".to_string(),
            TimerStatus::Installed { next_run } => format!("Next: {}", next_run),
            TimerStatus::Error(e) => format!("Error: {}", e),
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

        let content = settings::view_column(vec![
            page_title.into(),
            wallpaper_section.into(),
            settings_section.into(),
            actions_section.into(),
        ]);

        widget::scrollable(
            container(
                container(content)
                    .max_width(800)
            )
            .width(Length::Fill)
            .center_x(Length::Fill)
            .padding(16)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_history(&self) -> Element<'_, Message> {
        let title_row = row()
            .spacing(12)
            .align_y(cosmic::iced::Alignment::Center)
            .push(
                button::icon(widget::icon::from_name("go-previous-symbolic"))
                    .on_press(Message::ShowMain)
            )
            .push(text::title3("Downloaded Wallpapers"))
            .push(cosmic::widget::horizontal_space())
            .push(
                button::icon(widget::icon::from_name("view-refresh-symbolic"))
                    .on_press(Message::RefreshHistory)
            );

        let history_content: Element<_> = if self.history.is_empty() {
            container(text::body("No wallpapers downloaded yet"))
                .padding(40)
                .center_x(Length::Fill)
                .into()
        } else {
            let mut history_column = column().spacing(12).padding(10);

            for item in &self.history {
                let item_path = item.path.clone();
                let delete_path = item.path.clone();

                let preview = widget::image(item.path.to_string_lossy().to_string())
                    .content_fit(ContentFit::Cover)
                    .width(Length::Fixed(160.0))
                    .height(Length::Fixed(90.0));

                let info = column()
                    .spacing(4)
                    .push(text::body(item.date.clone()))
                    .push(text::caption(item.filename.clone()));

                let apply_btn = button::suggested("Apply")
                    .on_press(Message::ApplyHistoryWallpaper(item_path));

                let is_pending = self.pending_delete.as_ref() == Some(&item.path);
                let delete_btn: Element<_> = if is_pending {
                    row()
                        .spacing(8)
                        .push(button::destructive("Confirm").on_press(Message::ConfirmDeleteHistoryItem))
                        .push(button::standard("Cancel").on_press(Message::CancelDeleteHistoryItem))
                        .into()
                } else {
                    button::destructive("Delete")
                        .on_press(Message::RequestDeleteHistoryItem(delete_path))
                        .into()
                };

                let item_row = row()
                    .spacing(16)
                    .align_y(cosmic::iced::Alignment::Center)
                    .push(preview)
                    .push(info)
                    .push(cosmic::widget::horizontal_space())
                    .push(apply_btn)
                    .push(delete_btn);

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

/// Scan wallpaper directory for history items
fn scan_history(wallpaper_dir: &str) -> Vec<HistoryItem> {
    let dir = std::path::Path::new(wallpaper_dir);
    if !dir.exists() {
        return Vec::new();
    }

    let mut items: Vec<HistoryItem> = std::fs::read_dir(dir)
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
            let filename = path.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let date = extract_date_from_filename(&filename);

            HistoryItem { path, filename, date }
        })
        .collect();

    items.sort_by(|a, b| b.date.cmp(&a.date));
    items
}

/// Check timer status via D-Bus (communicates with applet)
async fn check_timer_status() -> TimerStatus {
    match WallpaperClient::connect().await {
        Ok(client) => {
            match client.get_timer_enabled().await {
                Ok(enabled) => {
                    if enabled {
                        let next_run = match client.get_timer_next_run().await {
                            Ok(time) if !time.is_empty() => time,
                            _ => "Scheduled".to_string(),
                        };
                        TimerStatus::Installed { next_run }
                    } else {
                        TimerStatus::NotInstalled
                    }
                }
                Err(e) => TimerStatus::Error(format!("D-Bus error: {}", e))
            }
        }
        Err(_) => {
            // Applet not running - check timer state file directly
            let state = crate::timer::TimerState::load();
            if state.enabled {
                TimerStatus::Installed { next_run: "Applet not running".to_string() }
            } else {
                TimerStatus::NotInstalled
            }
        }
    }
}

/// Enable timer via D-Bus
async fn install_timer() -> Result<(), String> {
    match WallpaperClient::connect().await {
        Ok(client) => {
            client.set_timer_enabled(true).await
                .map_err(|e| format!("Failed to enable timer: {}", e))
        }
        Err(_) => {
            let mut state = crate::timer::TimerState::load();
            state.enabled = true;
            state.save()?;
            Ok(())
        }
    }
}

/// Disable timer via D-Bus
async fn uninstall_timer() -> Result<(), String> {
    match WallpaperClient::connect().await {
        Ok(client) => {
            client.set_timer_enabled(false).await
                .map_err(|e| format!("Failed to disable timer: {}", e))
        }
        Err(_) => {
            let mut state = crate::timer::TimerState::load();
            state.enabled = false;
            state.save()?;
            Ok(())
        }
    }
}

/// Run a host command, using flatpak-spawn when in Flatpak sandbox
async fn run_host_command(cmd: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    if is_flatpak() {
        let mut spawn_args = vec!["--host", cmd];
        spawn_args.extend(args);
        tokio::process::Command::new("flatpak-spawn")
            .args(&spawn_args)
            .output()
            .await
    } else {
        tokio::process::Command::new(cmd)
            .args(args)
            .output()
            .await
    }
}

/// Spawn a host command in background
async fn spawn_host_command(cmd: &str) -> std::io::Result<tokio::process::Child> {
    if is_flatpak() {
        tokio::process::Command::new("flatpak-spawn")
            .args(["--host", cmd])
            .spawn()
    } else {
        tokio::process::Command::new(cmd)
            .spawn()
    }
}

/// Apply wallpaper to COSMIC desktop
async fn apply_cosmic_wallpaper(image_path: &str) -> Result<(), String> {
    let config_path = dirs::home_dir()
        .ok_or("Could not find home directory")?
        .join(".config/cosmic/com.system76.CosmicBackground/v1/all");

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

    let _ = run_host_command("pkill", &["-x", "cosmic-bg"]).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    spawn_host_command("cosmic-bg").await
        .map_err(|e| format!("Failed to start cosmic-bg: {}", e))?;

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let check = run_host_command("pgrep", &["-x", "cosmic-bg"]).await;
    match check {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err("cosmic-bg failed to start - wallpaper may not have been applied".to_string())
    }
}

/// Public wrapper for headless wallpaper application.
pub async fn apply_wallpaper_headless(image_path: &str) -> Result<(), String> {
    apply_cosmic_wallpaper(image_path).await
}

/// Run the settings application
pub fn run_settings() -> cosmic::iced::Result {
    let settings = cosmic::app::Settings::default()
        .size(cosmic::iced::Size::new(850.0, 750.0))
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(600.0)
                .min_height(550.0)
        );
    cosmic::app::run::<SettingsApp>(settings, ())
}

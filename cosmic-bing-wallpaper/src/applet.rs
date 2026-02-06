//! COSMIC Panel Applet Module
//!
//! Implements the Bing Wallpaper manager as a native COSMIC panel applet.
//! The applet lives in the panel, shows a popup on click with quick controls,
//! and can launch a separate settings window.
//!
//! ## Architecture
//!
//! The applet process:
//! - Runs the D-Bus service for wallpaper operations
//! - Manages the internal timer (daily wallpaper fetch)
//! - Shows a panel icon with popup for quick controls
//! - Launches the settings window via --settings
//!
//! The settings window connects to this applet via D-Bus.

use cosmic::app::{Core, Task};
use cosmic::iced::window::Id;
use cosmic::iced::{Length, Rectangle};
use cosmic::iced_runtime::core::window;
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget::{self, text};
use cosmic::Element;

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::service::{is_flatpak, ServiceState, WallpaperService, SERVICE_NAME, OBJECT_PATH};
use crate::timer::InternalTimer;

/// Application ID (must match desktop entry)
const APP_ID: &str = "io.github.reality2_roycdavies.cosmic-bing-wallpaper";

/// Commands sent from applet UI to background service thread
enum ServiceCommand {
    FetchWallpaper,
    SetTimerEnabled(bool),
}

/// Events sent from background service thread to applet UI
#[derive(Debug)]
enum ServiceEvent {
    TimerState { enabled: bool, next_run: String },
    FetchStarted,
    FetchComplete(Result<String, String>),
}

/// Messages for the applet
#[derive(Debug, Clone)]
pub enum Message {
    /// Poll background service for events
    PollEvents,
    /// Popup closed by compositor
    PopupClosed(Id),
    /// Surface action (popup create/destroy)
    Surface(cosmic::surface::Action),
    /// User clicked "Fetch Today's Wallpaper"
    FetchWallpaper,
    /// User toggled the timer
    ToggleTimer,
    /// User clicked "Settings..."
    OpenSettings,
}

/// The COSMIC panel applet
pub struct BingWallpaperApplet {
    core: Core,

    // Popup state
    popup: Option<Id>,

    // Service state (synced from background thread)
    timer_enabled: bool,
    next_run: String,
    is_fetching: bool,
    fetch_status: String,

    // Communication channels with background service
    cmd_tx: std::sync::mpsc::Sender<ServiceCommand>,
    event_rx: std::sync::mpsc::Receiver<ServiceEvent>,
}

impl cosmic::Application for BingWallpaperApplet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();

        // Load initial timer state
        let initial_state = crate::timer::TimerState::load();
        let timer_enabled = initial_state.enabled;

        // Start background service thread with D-Bus service and timer
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(run_background_service(cmd_rx, event_tx));
        });

        let applet = Self {
            core,
            popup: None,
            timer_enabled,
            next_run: String::new(),
            is_fetching: false,
            fetch_status: "Ready".to_string(),
            cmd_tx,
            event_rx,
        };

        (applet, Task::none())
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::PollEvents => {
                // Drain all pending events from the background service
                while let Ok(event) = self.event_rx.try_recv() {
                    match event {
                        ServiceEvent::TimerState { enabled, next_run } => {
                            self.timer_enabled = enabled;
                            self.next_run = next_run;
                        }
                        ServiceEvent::FetchStarted => {
                            self.is_fetching = true;
                            self.fetch_status = "Fetching...".to_string();
                        }
                        ServiceEvent::FetchComplete(result) => {
                            self.is_fetching = false;
                            match result {
                                Ok(msg) => self.fetch_status = msg,
                                Err(e) => self.fetch_status = format!("Error: {}", e),
                            }
                        }
                    }
                }
            }

            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }

            Message::Surface(action) => {
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(action),
                ));
            }

            Message::FetchWallpaper => {
                let _ = self.cmd_tx.send(ServiceCommand::FetchWallpaper);
                self.is_fetching = true;
                self.fetch_status = "Fetching...".to_string();
            }

            Message::ToggleTimer => {
                let new_state = !self.timer_enabled;
                let _ = self.cmd_tx.send(ServiceCommand::SetTimerEnabled(new_state));
                self.timer_enabled = new_state;
            }

            Message::OpenSettings => {
                std::thread::spawn(|| {
                    let result = if is_flatpak() {
                        std::process::Command::new("flatpak-spawn")
                            .args([
                                "--host",
                                "flatpak",
                                "run",
                                "io.github.reality2_roycdavies.cosmic-bing-wallpaper",
                                "--settings",
                            ])
                            .spawn()
                    } else {
                        let exe = std::env::current_exe()
                            .unwrap_or_else(|_| "cosmic-bing-wallpaper".into());
                        std::process::Command::new(exe).arg("--settings").spawn()
                    };
                    if let Err(e) = result {
                        eprintln!("Failed to launch settings: {}", e);
                    }
                });
            }
        }

        Task::none()
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        // Poll background service events every 500ms
        cosmic::iced::time::every(std::time::Duration::from_millis(500))
            .map(|_| Message::PollEvents)
    }

    fn view(&self) -> Element<'_, Message> {
        // Panel icon
        let icon: Element<Message> = widget::icon::from_name("io.github.reality2_roycdavies.cosmic-bing-wallpaper-symbolic")
            .symbolic(true)
            .into();

        // Create applet button with click-to-toggle-popup
        let have_popup = self.popup;
        let btn = self
            .core
            .applet
            .button_from_element(icon, true)
            .on_press_with_rectangle(move |offset, bounds| {
                if let Some(id) = have_popup {
                    Message::Surface(destroy_popup(id))
                } else {
                    Message::Surface(app_popup::<BingWallpaperApplet>(
                        move |state: &mut BingWallpaperApplet| {
                            let new_id = Id::unique();
                            state.popup = Some(new_id);

                            let popup_width = 300u32;
                            let popup_height = 260u32;

                            let mut popup_settings = state.core.applet.get_popup_settings(
                                state.core.main_window_id().unwrap(),
                                new_id,
                                Some((popup_width, popup_height)),
                                None,
                                None,
                            );
                            popup_settings.positioner.anchor_rect = Rectangle {
                                x: (bounds.x - offset.x) as i32,
                                y: (bounds.y - offset.y) as i32,
                                width: bounds.width as i32,
                                height: bounds.height as i32,
                            };
                            popup_settings
                        },
                        Some(Box::new(|state: &BingWallpaperApplet| {
                            Element::from(state.core.applet.popup_container(
                                state.popup_content(),
                            ))
                            .map(cosmic::Action::App)
                        })),
                    ))
                }
            });

        let tooltip = if self.timer_enabled {
            format!("Bing Wallpaper (ON)")
        } else {
            format!("Bing Wallpaper (OFF)")
        };

        Element::from(self.core.applet.applet_tooltip::<Message>(
            btn,
            tooltip,
            self.popup.is_some(),
            |a| Message::Surface(a),
            None,
        ))
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        // Popup content is rendered via the closure in app_popup, not here
        "".into()
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}

impl BingWallpaperApplet {
    /// Build the popup content widget
    fn popup_content(&self) -> widget::Column<'_, Message> {
        use cosmic::iced::widget::{column, container, horizontal_space, row, Space};
        use cosmic::iced::{Alignment, Color};

        // Title
        let title_row = row![
            text::body("Bing Wallpaper"),
            horizontal_space(),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // Timer status
        let timer_label = if self.timer_enabled {
            format!("Daily Update: ON")
        } else {
            format!("Daily Update: OFF")
        };
        let next_run_text = if self.timer_enabled && !self.next_run.is_empty() {
            format!("Next: {}", self.next_run)
        } else {
            String::new()
        };

        let status_section = if next_run_text.is_empty() {
            column![text::body(timer_label)].spacing(2)
        } else {
            column![text::body(timer_label), text::caption(next_run_text)].spacing(2)
        };

        // Fetch status
        let fetch_text = text::caption(&self.fetch_status);

        // Fetch button
        let fetch_btn: Element<Message> = if self.is_fetching {
            widget::button::standard("Fetching...").into()
        } else {
            widget::button::suggested("Fetch Today's Wallpaper")
                .on_press(Message::FetchWallpaper)
                .into()
        };

        // Timer toggle
        let timer_toggle_row = row![
            text::body("Daily Update"),
            horizontal_space(),
            widget::toggler(self.timer_enabled).on_toggle(|_| Message::ToggleTimer),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // Settings button
        let settings_row = row![
            horizontal_space(),
            widget::button::standard("Settings...").on_press(Message::OpenSettings),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // Divider helper
        let divider = || {
            container(Space::new(Length::Fill, Length::Fixed(1.0))).style(
                |theme: &cosmic::Theme| {
                    let cosmic = theme.cosmic();
                    container::Style {
                        background: Some(cosmic::iced::Background::Color(
                            Color::from(cosmic.palette.neutral_5),
                        )),
                        ..Default::default()
                    }
                },
            )
        };

        column![
            title_row,
            divider(),
            status_section,
            fetch_text,
            fetch_btn,
            divider(),
            timer_toggle_row,
            divider(),
            settings_row,
        ]
        .spacing(8)
        .padding(12)
    }
}

/// Background service running D-Bus and timer
async fn run_background_service(
    cmd_rx: std::sync::mpsc::Receiver<ServiceCommand>,
    event_tx: std::sync::mpsc::Sender<ServiceEvent>,
) {
    // Create the internal timer
    let timer = Arc::new(InternalTimer::new());
    let mut timer_rx = timer.start();

    // Create shared state
    let state = Arc::new(RwLock::new(ServiceState::new(timer.clone())));

    // Start D-Bus service
    let service = WallpaperService::new(state.clone());
    let _dbus_conn = match zbus::connection::Builder::session()
        .and_then(|b| b.name(SERVICE_NAME))
        .and_then(|b| b.serve_at(OBJECT_PATH, service))
    {
        Ok(builder) => match builder.build().await {
            Ok(conn) => {
                eprintln!("D-Bus service running at {} on {}", OBJECT_PATH, SERVICE_NAME);
                Some(conn)
            }
            Err(e) => {
                eprintln!("Failed to build D-Bus connection: {}", e);
                None
            }
        },
        Err(e) => {
            eprintln!("Failed to configure D-Bus: {}", e);
            None
        }
    };

    // Spawn timer event handler
    let state_for_timer = state.clone();
    let event_tx_timer = event_tx.clone();
    let _timer_handle = tokio::spawn(async move {
        while let Some(()) = timer_rx.recv().await {
            eprintln!("Timer fired - fetching wallpaper...");
            let _ = event_tx_timer.send(ServiceEvent::FetchStarted);

            let result = do_fetch_and_apply(&state_for_timer).await;
            let _ = event_tx_timer.send(ServiceEvent::FetchComplete(result));
        }
    });

    // Main event loop
    loop {
        // Check for commands from the applet UI
        if let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                ServiceCommand::FetchWallpaper => {
                    let _ = event_tx.send(ServiceEvent::FetchStarted);
                    let state_clone = state.clone();
                    let event_tx_clone = event_tx.clone();
                    tokio::spawn(async move {
                        let result = do_fetch_and_apply(&state_clone).await;
                        let _ = event_tx_clone.send(ServiceEvent::FetchComplete(result));
                    });
                }
                ServiceCommand::SetTimerEnabled(enabled) => {
                    timer.set_enabled(enabled);
                }
            }
        }

        // Send periodic timer state updates
        let enabled = timer.is_enabled();
        let next_run = timer.next_run_string().await;
        let _ = event_tx.send(ServiceEvent::TimerState { enabled, next_run });

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}

/// Fetch today's wallpaper and apply it
async fn do_fetch_and_apply(state: &Arc<RwLock<ServiceState>>) -> Result<String, String> {
    // Reload config from disk to get latest settings
    let fresh_config = Config::load();
    let (market, wallpaper_dir, keep_days) = (
        fresh_config.market.clone(),
        fresh_config.wallpaper_dir.clone(),
        fresh_config.keep_days,
    );

    // Update state with fresh config
    {
        let mut s = state.write().await;
        s.config = fresh_config;
    }

    // Fetch image info
    let image = crate::bing::fetch_bing_image_info(&market)
        .await
        .map_err(|e| format!("Failed to fetch: {}", e))?;

    eprintln!("Found: {}", image.title);

    // Download image
    let path = crate::bing::download_image(&image, &wallpaper_dir, &market)
        .await
        .map_err(|e| format!("Failed to download: {}", e))?;

    eprintln!("Downloaded to: {}", path);

    // Clean up old wallpapers
    crate::service::cleanup_old_wallpapers(&wallpaper_dir, keep_days);

    // Apply wallpaper
    crate::service::apply_cosmic_wallpaper(&path)
        .map_err(|e| format!("Failed to apply: {}", e))?;

    // Record fetch for timer state
    {
        let s = state.read().await;
        s.timer.record_fetch();
    }

    // Send notification
    let _ = std::process::Command::new("notify-send")
        .args([
            "-i",
            "preferences-desktop-wallpaper",
            "Bing Wallpaper",
            &format!("Applied: {}", image.title),
        ])
        .spawn();

    Ok(format!("Applied: {}", image.title))
}

/// Run the applet
pub fn run_applet() -> cosmic::iced::Result {
    cosmic::applet::run::<BingWallpaperApplet>(())
}

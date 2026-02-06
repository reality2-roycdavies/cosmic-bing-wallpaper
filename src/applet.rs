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

// --- COSMIC toolkit imports ---
// Core = shared app state from the framework; Task = async operations to run
use cosmic::app::{Core, Task};
// Id = unique identifier for popup windows
use cosmic::iced::window::Id;
// Length = sizing for UI elements; Rectangle = positioning for popup placement
use cosmic::iced::{Length, Rectangle};
// window module needed for on_close_requested signature
use cosmic::iced_runtime::core::window;
// app_popup/destroy_popup = create and remove flyout popup surfaces
use cosmic::surface::action::{app_popup, destroy_popup};
// widget = UI building blocks (buttons, text, togglers, etc.)
use cosmic::widget::{self, text};
// Element = the generic UI element type returned by view functions
use cosmic::Element;

// --- Standard library and async imports ---
// Arc = thread-safe reference counting for sharing data between threads
use std::sync::Arc;
// RwLock = async read-write lock for shared mutable state
use tokio::sync::RwLock;

// --- Internal modules ---
use crate::config::Config;
use crate::service::{is_flatpak, ServiceState, WallpaperService, SERVICE_NAME, OBJECT_PATH};
use crate::timer::InternalTimer;

/// Application ID (must match desktop entry)
const APP_ID: &str = "io.github.reality2_roycdavies.cosmic-bing-wallpaper";

/// Commands sent from the applet UI thread to the background service thread.
///
/// The applet UI and background service run on different threads.
/// We use channels (like a message queue) to communicate between them.
/// The UI sends commands, and the background service processes them.
enum ServiceCommand {
    /// Request the background service to fetch and apply today's wallpaper
    FetchWallpaper,
    /// Tell the background service to enable or disable the daily timer
    SetTimerEnabled(bool),
}

/// Events sent from the background service thread back to the applet UI.
///
/// These update the UI state (e.g., showing "Fetching..." or the result).
#[derive(Debug)]
enum ServiceEvent {
    /// Periodic update of timer state (sent every 500ms so UI stays in sync)
    TimerState { enabled: bool, next_run: String },
    /// A wallpaper fetch has started
    FetchStarted,
    /// A wallpaper fetch completed (Ok = success message, Err = error message)
    FetchComplete(Result<String, String>),
}

/// All possible user interactions and system events in the applet.
///
/// In the COSMIC/Iced framework, the UI is updated through a "message" system:
/// 1. User clicks a button → a Message is created
/// 2. The `update()` function receives the Message and modifies app state
/// 3. The `view()` function re-renders the UI based on the new state
///
/// This is called the "Model-View-Update" (MVU) pattern, also known as
/// "The Elm Architecture". It makes the UI predictable and easy to reason about.
#[derive(Debug, Clone)]
pub enum Message {
    /// Timer tick: check for new events from the background service thread.
    /// This fires every 500ms via the `subscription()` method, keeping the
    /// UI in sync with the background service's state (timer status, fetch results).
    PollEvents,
    /// The compositor (window manager) closed our popup window.
    /// We need to update our state to reflect that the popup is no longer visible.
    PopupClosed(Id),
    /// Internal message for creating/destroying popup surfaces.
    /// Wraps the low-level surface action that COSMIC needs to manage windows.
    Surface(cosmic::surface::Action),
    /// User clicked the "Fetch Today's Wallpaper" button in the popup.
    /// This sends a command to the background service thread to start fetching.
    FetchWallpaper,
    /// User toggled the daily auto-update switch in the popup.
    /// Sends a command to enable/disable the timer in the background service.
    ToggleTimer,
    /// User clicked the "Settings..." button in the popup.
    /// This spawns a new process: `cosmic-bing-wallpaper --settings`
    OpenSettings,
}

/// The main COSMIC panel applet struct.
///
/// This holds all the state for the applet. In COSMIC/Iced, the entire UI is
/// derived from this state — there's no hidden mutable state elsewhere.
///
/// The applet has two main parts:
/// 1. **UI thread** (this struct): Renders the panel icon and popup, handles user clicks
/// 2. **Background thread**: Runs the D-Bus service and timer, does network operations
///
/// They communicate through channels (like message queues):
/// - `cmd_tx` sends commands from UI → background (e.g., "fetch wallpaper")
/// - `event_rx` receives events from background → UI (e.g., "fetch complete")
pub struct BingWallpaperApplet {
    /// Core COSMIC framework state (manages windows, themes, panel integration)
    core: Core,

    /// The ID of the currently open popup, or None if no popup is shown.
    /// When the user clicks the panel icon, we create a popup and store its ID here.
    /// When they click again (or click elsewhere), we destroy the popup and set this to None.
    popup: Option<Id>,

    // --- State synced from the background service thread ---
    // These values are updated every 500ms by PollEvents

    /// Whether the daily auto-update timer is currently enabled
    timer_enabled: bool,
    /// Human-readable string for the next scheduled run (e.g., "Fri Feb 07 08:00")
    next_run: String,
    /// True while a wallpaper fetch is in progress (used to disable the fetch button)
    is_fetching: bool,
    /// Status text shown in the popup (e.g., "Ready", "Fetching...", "Applied: ...")
    fetch_status: String,

    // --- Communication channels with the background service thread ---

    /// Sender half of the command channel: UI thread sends commands to the background thread.
    /// Uses std::sync::mpsc (not tokio) because the UI thread is single-threaded.
    cmd_tx: std::sync::mpsc::Sender<ServiceCommand>,
    /// Receiver half of the event channel: UI thread receives events from the background thread.
    /// Polled every 500ms via the PollEvents subscription.
    event_rx: std::sync::mpsc::Receiver<ServiceEvent>,
}

/// Implementation of the COSMIC Application trait.
///
/// This is the heart of the applet. COSMIC requires us to implement several methods:
/// - `init()`: Called once when the applet starts — sets up state and launches background thread
/// - `update()`: Called whenever a Message is received — modifies state based on user actions
/// - `view()`: Called after every update — builds the UI from current state
/// - `subscription()`: Returns background tasks that produce Messages on a schedule
impl cosmic::Application for BingWallpaperApplet {
    /// SingleThreadExecutor is used for panel applets (lightweight, no multi-threading overhead).
    /// Full windows would use `cosmic::executor::Default` instead.
    type Executor = cosmic::SingleThreadExecutor;
    /// No startup flags needed — the applet reads its config from disk
    type Flags = ();
    /// The Message enum defined above
    type Message = Message;

    /// Must match the .desktop file name for COSMIC to find this applet
    const APP_ID: &'static str = APP_ID;

    // Required accessors for the COSMIC framework to read/write core state
    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    /// Called once when the applet is first loaded into the panel.
    ///
    /// This is where we:
    /// 1. Create communication channels between UI and background threads
    /// 2. Load the saved timer state from disk
    /// 3. Spawn the background thread that runs D-Bus service + timer
    /// 4. Return the initial applet state
    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        // Create two channels for bidirectional communication:
        // cmd_tx/cmd_rx: UI → background (commands like "fetch wallpaper")
        // event_tx/event_rx: background → UI (events like "fetch complete")
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();

        // Load the timer's enabled/disabled state from the config file on disk
        let initial_state = crate::timer::TimerState::load();
        let timer_enabled = initial_state.enabled;

        // Spawn a separate OS thread for the background service.
        // We can't use tokio::spawn here because the COSMIC applet uses its own
        // async executor (iced), not tokio. The background thread creates its own
        // tokio runtime for async operations (D-Bus, HTTP requests, timer).
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(run_background_service(cmd_rx, event_tx));
        });

        // Build the initial applet state with all defaults
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

        // Task::none() means no async startup tasks — the background thread handles everything
        (applet, Task::none())
    }

    /// Called when the compositor wants to close a window/popup.
    /// We convert this into a Message so it goes through our normal update flow.
    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    /// The core update function — handles all Messages and modifies state accordingly.
    ///
    /// This is called every time a Message is produced (from user clicks, timers, etc.).
    /// It's the ONLY place where state changes happen — keeping logic centralized
    /// and predictable.
    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::PollEvents => {
                // Drain ALL pending events from the background service.
                // try_recv() is non-blocking: returns Ok(event) if available, Err if empty.
                // We use a while loop to process all queued events at once.
                while let Ok(event) = self.event_rx.try_recv() {
                    match event {
                        ServiceEvent::TimerState { enabled, next_run } => {
                            // Update our local copy of the timer state for display
                            self.timer_enabled = enabled;
                            self.next_run = next_run;
                        }
                        ServiceEvent::FetchStarted => {
                            // Show loading state in the popup
                            self.is_fetching = true;
                            self.fetch_status = "Fetching...".to_string();
                        }
                        ServiceEvent::FetchComplete(result) => {
                            // Show the result (success or error) in the popup
                            self.is_fetching = false;
                            match result {
                                Ok(msg) => self.fetch_status = msg,
                                Err(e) => self.fetch_status = format!("Error: {e}"),
                            }
                        }
                    }
                }
            }

            Message::PopupClosed(id) => {
                // Only clear our popup state if this is OUR popup being closed
                // (there might be other windows in the system)
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }

            Message::Surface(action) => {
                // Forward surface actions (popup create/destroy) to the COSMIC framework.
                // This is the pattern required by COSMIC to manage popup windows.
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(action),
                ));
            }

            Message::FetchWallpaper => {
                // Send a command to the background thread to start fetching.
                // We also set is_fetching=true immediately for instant UI feedback
                // (the user sees "Fetching..." right away, not after the 500ms poll).
                let _ = self.cmd_tx.send(ServiceCommand::FetchWallpaper);
                self.is_fetching = true;
                self.fetch_status = "Fetching...".to_string();
            }

            Message::ToggleTimer => {
                // Toggle the timer state and notify the background thread.
                // We update the local state immediately for instant UI feedback.
                let new_state = !self.timer_enabled;
                let _ = self.cmd_tx.send(ServiceCommand::SetTimerEnabled(new_state));
                self.timer_enabled = new_state;
            }

            Message::OpenSettings => {
                // Launch the settings window as a separate process.
                // We spawn a new thread to avoid blocking the UI while the process starts.
                std::thread::spawn(|| {
                    // In Flatpak, we need to use flatpak-spawn to escape the sandbox
                    // and launch a new Flatpak instance with the --settings flag.
                    let result = if is_flatpak() {
                        std::process::Command::new("flatpak-spawn")
                            .args([
                                "--host",     // Run on the host, not inside our sandbox
                                "flatpak",
                                "run",
                                "io.github.reality2_roycdavies.cosmic-bing-wallpaper",
                                "--settings",
                            ])
                            .spawn()
                    } else {
                        // Native install: just run our own executable with --settings
                        let exe = std::env::current_exe()
                            .unwrap_or_else(|_| "cosmic-bing-wallpaper".into());
                        std::process::Command::new(exe).arg("--settings").spawn()
                    };
                    if let Err(e) = result {
                        eprintln!("Failed to launch settings: {e}");
                    }
                });
            }
        }

        Task::none()
    }

    /// Returns background subscriptions that produce Messages on a schedule.
    ///
    /// Subscriptions are like timers that run in the background and periodically
    /// produce Messages. Here we poll the background service every 500ms to
    /// keep the UI in sync with timer state and fetch progress.
    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        cosmic::iced::time::every(std::time::Duration::from_millis(500))
            .map(|_| Message::PollEvents)
    }

    /// Builds the panel icon widget that appears in the COSMIC panel bar.
    ///
    /// This is the small icon you see in the panel. When clicked, it toggles
    /// a popup flyout with controls. The icon also shows a tooltip on hover.
    fn view(&self) -> Element<'_, Message> {
        // Load the symbolic icon from the system icon theme.
        // The icon name must match an installed .svg file.
        let icon: Element<Message> = widget::icon::from_name("io.github.reality2_roycdavies.cosmic-bing-wallpaper-symbolic")
            .symbolic(true)   // Use symbolic (monochrome) variant for panel
            .into();

        // Wrap the icon in a button that toggles the popup on click.
        // `on_press_with_rectangle` gives us the click position and button bounds,
        // which we need to position the popup correctly near the panel icon.
        let have_popup = self.popup;
        let btn = self
            .core
            .applet
            .button_from_element(icon, true)   // true = active/selected style when popup is open
            .on_press_with_rectangle(move |offset, bounds| {
                if let Some(id) = have_popup {
                    // Popup is already open → close it
                    Message::Surface(destroy_popup(id))
                } else {
                    // No popup → create one
                    // app_popup takes two closures:
                    // 1. A setup closure that creates popup settings (position, size)
                    // 2. A render closure that builds the popup's UI content
                    Message::Surface(app_popup::<BingWallpaperApplet>(
                        move |state: &mut BingWallpaperApplet| {
                            // Generate a unique ID for this popup window
                            let new_id = Id::unique();
                            state.popup = Some(new_id);

                            // Set popup dimensions
                            let popup_width = 300u32;
                            let popup_height = 260u32;

                            // Calculate popup position relative to the panel icon
                            let mut popup_settings = state.core.applet.get_popup_settings(
                                state.core.main_window_id().unwrap(),
                                new_id,
                                Some((popup_width, popup_height)),
                                None,
                                None,
                            );
                            // Position the popup's anchor at the icon's location
                            popup_settings.positioner.anchor_rect = Rectangle {
                                x: (bounds.x - offset.x) as i32,
                                y: (bounds.y - offset.y) as i32,
                                width: bounds.width as i32,
                                height: bounds.height as i32,
                            };
                            popup_settings
                        },
                        // Render closure: builds the popup content each time it needs updating
                        Some(Box::new(|state: &BingWallpaperApplet| {
                            Element::from(state.core.applet.popup_container(
                                state.popup_content(),
                            ))
                            .map(cosmic::Action::App)
                        })),
                    ))
                }
            });

        // Tooltip text shown when hovering over the panel icon
        let tooltip = if self.timer_enabled {
            "Bing Wallpaper (ON)".to_string()
        } else {
            "Bing Wallpaper (OFF)".to_string()
        };

        // Wrap the button with tooltip support
        Element::from(self.core.applet.applet_tooltip::<Message>(
            btn,
            tooltip,
            self.popup.is_some(),    // Don't show tooltip when popup is open
            |a| Message::Surface(a), // Convert surface actions to our Message type
            None,                    // No additional tooltip options
        ))
    }

    /// Required by COSMIC for popup windows, but we render our popup via
    /// the closure in `app_popup()` instead. Returns empty content.
    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        "".into()
    }

    /// Apply the COSMIC applet visual style (transparent background, panel-aware theming)
    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}

/// Helper methods for building the popup UI
impl BingWallpaperApplet {
    /// Builds the popup flyout content shown when the user clicks the panel icon.
    ///
    /// The popup layout (top to bottom):
    /// ┌──────────────────────────────┐
    /// │ Bing Wallpaper               │  ← Title
    /// │ ──────────────────────────── │  ← Divider
    /// │ Daily Update: ON/OFF         │  ← Timer status
    /// │ Next: Fri Feb 07 08:00       │  ← Next run time (if enabled)
    /// │ Ready / Fetching... / Error  │  ← Fetch status
    /// │ [Fetch Today's Wallpaper]    │  ← Fetch button
    /// │ ──────────────────────────── │  ← Divider
    /// │ Daily Update     [toggle]    │  ← Timer on/off switch
    /// │ ──────────────────────────── │  ← Divider
    /// │              [Settings...]   │  ← Opens settings window
    /// └──────────────────────────────┘
    fn popup_content(&self) -> widget::Column<'_, Message> {
        use cosmic::iced::widget::{column, container, horizontal_space, row, Space};
        use cosmic::iced::{Alignment, Color};

        // --- Title row ---
        let title_row = row![
            text::body("Bing Wallpaper"),
            horizontal_space(),    // Push content to the left
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // --- Timer status section ---
        let timer_label = if self.timer_enabled {
            "Daily Update: ON"
        } else {
            "Daily Update: OFF"
        };
        // Show next scheduled run time only when timer is active
        let next_run_text = if self.timer_enabled && !self.next_run.is_empty() {
            format!("Next: {}", self.next_run)
        } else {
            String::new()
        };

        // Conditionally include the "Next: ..." line
        let status_section = if next_run_text.is_empty() {
            column![text::body(timer_label)].spacing(2)
        } else {
            column![text::body(timer_label), text::caption(next_run_text)].spacing(2)
        };

        // --- Fetch status and button ---
        let fetch_text = text::caption(&self.fetch_status);

        // Show a disabled "Fetching..." button during fetch, or an active button otherwise.
        // "suggested" style = primary/accent color, "standard" = neutral color.
        let fetch_btn: Element<Message> = if self.is_fetching {
            widget::button::standard("Fetching...").into()  // No on_press = disabled
        } else {
            widget::button::suggested("Fetch Today's Wallpaper")
                .on_press(Message::FetchWallpaper)
                .into()
        };

        // --- Timer toggle row ---
        let timer_toggle_row = row![
            text::body("Daily Update"),
            horizontal_space(),    // Push toggle to the right
            widget::toggler(self.timer_enabled).on_toggle(|_| Message::ToggleTimer),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // --- Settings button row (right-aligned) ---
        let settings_row = row![
            horizontal_space(),    // Push button to the right
            widget::button::standard("Settings...").on_press(Message::OpenSettings),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // --- Visual divider (thin horizontal line) ---
        // Creates a 1px high colored bar using the theme's neutral color palette
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

        // --- Assemble the complete popup layout ---
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
        .spacing(8)    // 8px gap between each widget
        .padding(12)   // 12px padding around the entire popup
    }
}

/// The background service that runs on a separate OS thread.
///
/// This function is the "main loop" for the background thread. It:
/// 1. Creates and starts the internal timer (for daily wallpaper fetches)
/// 2. Registers the D-Bus service (so the settings window can communicate with us)
/// 3. Listens for commands from the UI thread and timer events
/// 4. Sends status updates back to the UI thread every 500ms
///
/// # Arguments
/// * `cmd_rx` - Receives commands from the UI thread (fetch, toggle timer)
/// * `event_tx` - Sends events back to the UI thread (fetch results, timer state)
///
/// # Threading Model
/// This runs on its own OS thread with its own tokio runtime, separate from
/// the COSMIC/iced UI thread. Communication happens through std::sync::mpsc channels.
async fn run_background_service(
    cmd_rx: std::sync::mpsc::Receiver<ServiceCommand>,
    event_tx: std::sync::mpsc::Sender<ServiceEvent>,
) {
    // --- Set up the internal timer ---
    // The timer handles daily scheduled fetches and catch-up after boot.
    // Arc (Atomic Reference Counted) allows sharing the timer between multiple async tasks.
    let timer = Arc::new(InternalTimer::new());
    // start() returns a channel receiver that fires when the timer triggers
    let mut timer_rx = timer.start();

    // --- Create shared state ---
    // ServiceState holds the config and current wallpaper info.
    // Arc<RwLock<...>> allows multiple async tasks to read/write the state safely.
    let state = Arc::new(RwLock::new(ServiceState::new(timer.clone())));

    // --- Register the D-Bus service ---
    // D-Bus is a Linux inter-process communication (IPC) system.
    // We register our service so the settings window can call methods on it
    // (e.g., "fetch wallpaper", "get timer status").
    let service = WallpaperService::new(state.clone());
    let _dbus_conn = match zbus::connection::Builder::session()
        .and_then(|b| b.name(SERVICE_NAME))         // Claim our service name on the bus
        .and_then(|b| b.serve_at(OBJECT_PATH, service))  // Serve our interface at this path
    {
        Ok(builder) => match builder.build().await {
            Ok(conn) => {
                eprintln!("D-Bus service running at {OBJECT_PATH} on {SERVICE_NAME}");
                Some(conn)  // Keep the connection alive (dropped = service stops)
            }
            Err(e) => {
                eprintln!("Failed to build D-Bus connection: {e}");
                None  // Continue without D-Bus (applet still works, settings can't connect)
            }
        },
        Err(e) => {
            eprintln!("Failed to configure D-Bus: {e}");
            None
        }
    };

    // --- Spawn a task to handle timer events ---
    // When the timer fires (daily at 08:00 or on catch-up), this task
    // automatically fetches and applies today's wallpaper.
    let state_for_timer = state.clone();
    let event_tx_timer = event_tx.clone();
    let _timer_handle = tokio::spawn(async move {
        // timer_rx.recv() blocks until the timer fires, then returns Some(())
        while let Some(()) = timer_rx.recv().await {
            eprintln!("Timer fired - fetching wallpaper...");
            let _ = event_tx_timer.send(ServiceEvent::FetchStarted);

            let result = do_fetch_and_apply(&state_for_timer).await;
            let _ = event_tx_timer.send(ServiceEvent::FetchComplete(result));
        }
    });

    // --- Main event loop ---
    // Runs forever, checking for UI commands and sending status updates.
    loop {
        // Check for commands from the applet UI (non-blocking)
        if let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                ServiceCommand::FetchWallpaper => {
                    // Notify UI that fetch has started
                    let _ = event_tx.send(ServiceEvent::FetchStarted);
                    // Spawn the fetch as a separate async task so it doesn't block
                    // this loop (fetching can take several seconds for network I/O)
                    let state_clone = state.clone();
                    let event_tx_clone = event_tx.clone();
                    tokio::spawn(async move {
                        let result = do_fetch_and_apply(&state_clone).await;
                        let _ = event_tx_clone.send(ServiceEvent::FetchComplete(result));
                    });
                }
                ServiceCommand::SetTimerEnabled(enabled) => {
                    // Update the timer state (persisted to disk by set_enabled)
                    timer.set_enabled(enabled);
                }
            }
        }

        // Send the current timer state to the UI thread so the popup
        // can display the correct toggle state and next run time
        let enabled = timer.is_enabled();
        let next_run = timer.next_run_string().await;
        let _ = event_tx.send(ServiceEvent::TimerState { enabled, next_run });

        // Sleep 500ms before the next iteration to avoid busy-waiting
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}

/// Performs the complete wallpaper fetch-and-apply workflow.
///
/// This is the core operation of the app. It:
/// 1. Reloads config from disk (in case the user changed market/directory in settings)
/// 2. Calls the Bing API to get today's image metadata
/// 3. Downloads the image to the wallpaper directory
/// 4. Removes old wallpapers beyond the keep_days limit
/// 5. Writes the COSMIC background config file to apply the wallpaper
/// 6. Records the fetch time (so the timer knows not to catch up again today)
/// 7. Sends a desktop notification
///
/// # Returns
/// * `Ok(message)` - Success message like "Applied: Mountain Sunrise"
/// * `Err(message)` - Error message describing what went wrong
async fn do_fetch_and_apply(state: &Arc<RwLock<ServiceState>>) -> Result<String, String> {
    // Reload config from disk to pick up any changes made in the settings window
    // (the settings window writes config.json directly, not via D-Bus)
    let fresh_config = Config::load();
    let (market, wallpaper_dir, keep_days) = (
        fresh_config.market.clone(),
        fresh_config.wallpaper_dir.clone(),
        fresh_config.keep_days,
    );

    // Update the shared state with the fresh config
    {
        let mut s = state.write().await;
        s.config = fresh_config;
    }

    // Step 1: Ask Bing's API for today's image info (title, URL, copyright)
    let image = crate::bing::fetch_bing_image_info(&market)
        .await
        .map_err(|e| format!("Failed to fetch: {e}"))?;

    eprintln!("Found: {}", image.title);

    // Step 2: Download the actual image file to the wallpaper directory
    // (skips download if the file already exists from a previous fetch today)
    let path = crate::bing::download_image(&image, &wallpaper_dir, &market)
        .await
        .map_err(|e| format!("Failed to download: {e}"))?;

    eprintln!("Downloaded to: {path}");

    // Step 3: Remove wallpapers older than keep_days to save disk space
    crate::service::cleanup_old_wallpapers(&wallpaper_dir, keep_days);

    // Step 4: Apply the wallpaper by writing the COSMIC background config
    // and restarting the cosmic-bg process
    crate::service::apply_cosmic_wallpaper(&path)
        .map_err(|e| format!("Failed to apply: {e}"))?;

    // Step 5: Record this fetch so the timer's catch-up logic knows we're done for today
    {
        let s = state.read().await;
        s.timer.record_fetch();
    }

    // Step 6: Show a desktop notification to inform the user
    let _ = std::process::Command::new("notify-send")
        .args([
            "-i",
            "preferences-desktop-wallpaper",   // Icon for the notification
            "Bing Wallpaper",                   // Notification title
            &format!("Applied: {}", image.title), // Notification body
        ])
        .spawn();

    Ok(format!("Applied: {}", image.title))
}

/// Entry point for running the panel applet.
///
/// Called from main.rs when no command-line arguments are provided.
/// `cosmic::applet::run` handles all the COSMIC panel integration:
/// - Registering with the panel
/// - Creating the applet window
/// - Running the event loop
/// - Applying the correct theme and sizing
pub fn run_applet() -> cosmic::iced::Result {
    cosmic::applet::run::<BingWallpaperApplet>(())
}

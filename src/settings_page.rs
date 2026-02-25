//! Embeddable settings page for cosmic-bing-wallpaper
//!
//! Provides a config-focused settings UI that can be embedded in
//! cosmic-applet-settings. For the full wallpaper manager (preview,
//! fetch, history), use the standalone `cosmic-bing-wallpaper --settings`.

use cosmic::iced::Length;
use cosmic::widget::{button, dropdown, settings, text, toggler};
use cosmic::Element;

use crate::config::{Config, MARKETS};
use crate::timer::TimerState;

pub struct State {
    pub config: Config,
    pub selected_market_idx: usize,
    pub market_names: Vec<String>,
    pub timer_enabled: bool,
    pub status_message: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    MarketSelected(usize),
    FetchOnStartupToggled(bool),
    TimerToggled(bool),
    OpenFullSettings,
}

pub fn init() -> State {
    let config = Config::load();
    let selected_market_idx = MARKETS
        .iter()
        .position(|m| m.code == config.market)
        .unwrap_or(0);
    let market_names: Vec<String> = MARKETS.iter().map(|m| m.name.to_string()).collect();
    let timer_enabled = TimerState::load().enabled;

    State {
        config,
        selected_market_idx,
        market_names,
        timer_enabled,
        status_message: String::new(),
    }
}

pub fn update(state: &mut State, message: Message) {
    match message {
        Message::MarketSelected(idx) => {
            if idx < MARKETS.len() {
                state.selected_market_idx = idx;
                state.config.market = MARKETS[idx].code.to_string();
                match state.config.save() {
                    Ok(()) => state.status_message = "Region updated".to_string(),
                    Err(e) => state.status_message = format!("Error: {e}"),
                }
            }
        }
        Message::FetchOnStartupToggled(val) => {
            state.config.fetch_on_startup = val;
            match state.config.save() {
                Ok(()) => state.status_message = "Setting updated".to_string(),
                Err(e) => state.status_message = format!("Error: {e}"),
            }
        }
        Message::TimerToggled(val) => {
            let mut timer_state = TimerState::load();
            timer_state.enabled = val;
            match timer_state.save() {
                Ok(()) => {
                    state.timer_enabled = val;
                    state.status_message = if val {
                        "Daily update enabled".to_string()
                    } else {
                        "Daily update disabled".to_string()
                    };
                }
                Err(e) => state.status_message = format!("Error: {e}"),
            }
        }
        Message::OpenFullSettings => {
            std::thread::spawn(|| {
                let _ = std::process::Command::new("cosmic-bing-wallpaper")
                    .arg("--settings")
                    .spawn();
            });
        }
    }
}

pub fn view(state: &State) -> Element<'_, Message> {
    let page_title = text::title1("Bing Wallpaper Settings");

    let settings_section = settings::section()
        .title("Settings")
        .add(settings::item(
            "Region",
            dropdown(
                &state.market_names,
                Some(state.selected_market_idx),
                Message::MarketSelected,
            )
            .width(Length::Fixed(200.0)),
        ))
        .add(settings::item(
            "Fetch on startup",
            toggler(state.config.fetch_on_startup)
                .on_toggle(Message::FetchOnStartupToggled),
        ))
        .add(settings::item(
            "Daily auto-update",
            toggler(state.timer_enabled)
                .on_toggle(Message::TimerToggled),
        ));

    let actions_section = settings::section()
        .title("Wallpaper Manager")
        .add(settings::item_row(vec![
            button::suggested("Open Wallpaper Manager")
                .on_press(Message::OpenFullSettings)
                .into(),
        ]));

    let mut content_items: Vec<Element<'_, Message>> = vec![
        page_title.into(),
        text::caption("Configure Bing daily wallpaper. Use the Wallpaper Manager for preview, fetch, and history.").into(),
        settings_section.into(),
        actions_section.into(),
    ];

    if !state.status_message.is_empty() {
        content_items.push(text::body(&state.status_message).into());
    }

    settings::view_column(content_items).into()
}

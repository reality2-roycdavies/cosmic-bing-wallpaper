//! CLI settings protocol for cosmic-applet-settings hub integration.
//!
//! Supports image preview, wallpaper history list, fetch/apply/delete actions,
//! and periodic refresh for live timer status updates.

use crate::config::{Config, MARKETS};
use crate::timer::TimerState;
use chrono::{Local, NaiveTime};
use std::path::Path;

const SCHEDULED_MINUTE: u32 = 0;

pub fn describe() {
    let config = Config::load();
    let timer_state = TimerState::load();

    let market_options: Vec<serde_json::Value> = MARKETS
        .iter()
        .map(|m| serde_json::json!({"value": m.code, "label": m.name}))
        .collect();

    // Find the most recent wallpaper image and its metadata
    let most_recent = find_most_recent_wallpaper(&config.wallpaper_dir);
    let preview_path = most_recent
        .as_deref()
        .unwrap_or("");
    let copyright = most_recent
        .as_deref()
        .and_then(read_wallpaper_copyright)
        .unwrap_or_default();
    let current_date = Local::now().format("%Y-%m-%d").to_string();
    let timer_status = compute_timer_status_string(&config, &timer_state);
    let history = build_history_list(&config.wallpaper_dir);

    // Build the "Current Wallpaper" items — copyright only shown if available
    let mut current_items = vec![
        serde_json::json!({
            "type": "image",
            "key": "preview",
            "label": "",
            "value": preview_path,
            "height": 280.0
        }),
    ];
    if !copyright.is_empty() {
        current_items.push(serde_json::json!({
            "type": "info",
            "key": "copyright",
            "label": "Copyright",
            "value": copyright
        }));
    }
    current_items.push(serde_json::json!({
        "type": "info",
        "key": "current_date",
        "label": "Date",
        "value": current_date
    }));
    current_items.push(serde_json::json!({
        "type": "info",
        "key": "timer_status",
        "label": "Timer Status",
        "value": timer_status
    }));

    let schema = serde_json::json!({
        "title": "Bing Wallpaper",
        "description": "Fetches and applies Microsoft Bing's daily wallpaper.",
        "refresh_interval": 10,
        "sections": [
            {
                "title": "Current Wallpaper",
                "items": current_items
            },
            {
                "title": "Settings",
                "items": [
                    {
                        "type": "select",
                        "key": "market",
                        "label": "Region",
                        "value": config.market,
                        "options": market_options
                    },
                    {
                        "type": "toggle",
                        "key": "fetch_on_startup",
                        "label": "Fetch on Startup",
                        "value": config.fetch_on_startup
                    },
                    {
                        "type": "toggle",
                        "key": "timer_enabled",
                        "label": "Daily Auto-Update",
                        "value": timer_state.enabled
                    },
                    {
                        "type": "slider",
                        "key": "scheduled_hour",
                        "label": "Update Time (Hour)",
                        "value": config.scheduled_hour,
                        "min": 0.0,
                        "max": 23.0,
                        "step": 1.0,
                        "unit": ":00",
                        "visible_when": {"key": "timer_enabled", "equals": true}
                    }
                ]
            },
            {
                "title": "Wallpaper History",
                "actions": [
                    {"id": "fetch", "label": "Fetch Today's Wallpaper", "style": "suggested"}
                ],
                "items": [
                    {
                        "type": "list",
                        "key": "history",
                        "label": "",
                        "value": null,
                        "list_items": history
                    }
                ]
            }
        ],
        "actions": []
    });

    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}

pub fn set(key: &str, value: &str) {
    match key {
        "market" => {
            let mut config = Config::load();
            match serde_json::from_str::<String>(value) {
                Ok(v) => {
                    if MARKETS.iter().any(|m| m.code == v) {
                        config.market = v;
                        match config.save() {
                            Ok(()) => print_response(true, "Updated market"),
                            Err(e) => print_response(false, &format!("Save failed: {e}")),
                        }
                    } else {
                        print_response(false, &format!("Unknown market: {}", v));
                    }
                }
                Err(e) => print_response(false, &format!("Invalid value: {e}")),
            }
        }
        "fetch_on_startup" => {
            let mut config = Config::load();
            match serde_json::from_str::<bool>(value) {
                Ok(v) => {
                    config.fetch_on_startup = v;
                    match config.save() {
                        Ok(()) => print_response(true, "Updated fetch on startup"),
                        Err(e) => print_response(false, &format!("Save failed: {e}")),
                    }
                }
                Err(e) => print_response(false, &format!("Invalid boolean: {e}")),
            }
        }
        "timer_enabled" => {
            let mut timer_state = TimerState::load();
            match serde_json::from_str::<bool>(value) {
                Ok(v) => {
                    timer_state.enabled = v;
                    match timer_state.save() {
                        Ok(()) => print_response(true, "Updated timer state"),
                        Err(e) => print_response(false, &format!("Save failed: {e}")),
                    }
                }
                Err(e) => print_response(false, &format!("Invalid boolean: {e}")),
            }
        }
        "scheduled_hour" => {
            let mut config = Config::load();
            match serde_json::from_str::<f64>(value) {
                Ok(v) => {
                    let hour = (v as u32).min(23);
                    config.scheduled_hour = hour;
                    match config.save() {
                        Ok(()) => print_response(true, &format!("Update time set to {:02}:00", hour)),
                        Err(e) => print_response(false, &format!("Save failed: {e}")),
                    }
                }
                Err(e) => print_response(false, &format!("Invalid number: {e}")),
            }
        }
        _ => print_response(false, &format!("Unknown key: {key}")),
    }
}

pub fn action(id: &str, item_id: Option<&str>) {
    match id {
        "fetch" => {
            let config = Config::load();
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    print_response(false, &format!("Failed to create runtime: {e}"));
                    return;
                }
            };

            rt.block_on(async {
                // Direct fetch — also saves metadata sidecar for copyright display
                match crate::bing::fetch_bing_image_info(&config.market).await {
                    Ok(image) => {
                        match crate::bing::download_image(
                            &image,
                            &config.wallpaper_dir,
                            &config.market,
                        )
                        .await
                        {
                            Ok(path) => {
                                match crate::service::apply_cosmic_wallpaper(&path) {
                                    Ok(()) => {
                                        print_response(true, "Wallpaper fetched and applied")
                                    }
                                    Err(e) => {
                                        print_response(false, &format!("Apply failed: {e}"))
                                    }
                                }
                            }
                            Err(e) => print_response(false, &format!("Download failed: {e}")),
                        }
                    }
                    Err(e) => print_response(false, &format!("Fetch failed: {e}")),
                }
            });
        }

        "apply_history" => {
            if let Some(path) = item_id {
                if Path::new(path).exists() {
                    match crate::service::apply_cosmic_wallpaper(path) {
                        Ok(()) => print_response(true, "Wallpaper applied"),
                        Err(e) => print_response(false, &format!("Apply failed: {e}")),
                    }
                } else {
                    print_response(false, &format!("File not found: {path}"));
                }
            } else {
                print_response(false, "No wallpaper path specified");
            }
        }

        "delete_history" => {
            if let Some(path) = item_id {
                match std::fs::remove_file(path) {
                    Ok(()) => print_response(true, "Wallpaper deleted"),
                    Err(e) => print_response(false, &format!("Delete failed: {e}")),
                }
            } else {
                print_response(false, "No wallpaper path specified");
            }
        }

        "open_wallpaper_manager" => {
            use std::process::Command;
            match Command::new("cosmic-bing-wallpaper")
                .arg("--settings-standalone")
                .spawn()
            {
                Ok(_) => print_response(true, "Opened wallpaper manager"),
                Err(e) => print_response(false, &format!("Failed to open: {e}")),
            }
        }

        _ => print_response(false, &format!("Unknown action: {id}")),
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn print_response(ok: bool, message: &str) {
    let resp = serde_json::json!({"ok": ok, "message": message});
    println!("{}", resp);
}

/// Find the most recently modified wallpaper image in the wallpaper directory.
fn find_most_recent_wallpaper(wallpaper_dir: &str) -> Option<String> {
    let dir = Path::new(wallpaper_dir);
    if !dir.is_dir() {
        return None;
    }

    let mut best: Option<(std::time::SystemTime, String)> = None;

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jpg")
                || path.extension().and_then(|e| e.to_str()) == Some("png")
            {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        let path_str = path.to_string_lossy().to_string();
                        if best.as_ref().map_or(true, |(t, _)| modified > *t) {
                            best = Some((modified, path_str));
                        }
                    }
                }
            }
        }
    }

    best.map(|(_, p)| p)
}

/// Build the history list of wallpaper images sorted newest first.
fn build_history_list(wallpaper_dir: &str) -> Vec<serde_json::Value> {
    let dir = Path::new(wallpaper_dir);
    if !dir.is_dir() {
        return Vec::new();
    }

    let mut entries: Vec<(std::time::SystemTime, String, String)> = Vec::new();

    if let Ok(dir_entries) = std::fs::read_dir(dir) {
        for entry in dir_entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "jpg" && ext != "png" {
                continue;
            }

            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            let path_str = path.to_string_lossy().to_string();

            entries.push((modified, path_str, filename));
        }
    }

    // Sort newest first
    entries.sort_by(|a, b| b.0.cmp(&a.0));

    entries
        .into_iter()
        .map(|(modified, path, filename)| {
            // Extract date from filename if possible (bing-MARKET-YYYY-MM-DD.jpg)
            let title = extract_date_from_filename(&filename)
                .unwrap_or_else(|| {
                    // Fallback: format the modification time
                    let dt: chrono::DateTime<Local> = modified.into();
                    dt.format("%Y-%m-%d").to_string()
                });

            serde_json::json!({
                "id": path,
                "image": path,
                "title": title,
                "subtitle": filename,
                "actions": [
                    {"id": "apply_history", "label": "Apply", "style": "suggested"},
                    {
                        "id": "delete_history",
                        "label": "Delete",
                        "style": "destructive",
                        "confirm": format!("Delete {}?", filename)
                    }
                ]
            })
        })
        .collect()
}

/// Extract a date string from a bing wallpaper filename like "bing-en-US-2026-02-28.jpg".
fn extract_date_from_filename(filename: &str) -> Option<String> {
    // Look for YYYY-MM-DD pattern
    let stem = filename.strip_suffix(".jpg")
        .or_else(|| filename.strip_suffix(".png"))
        .unwrap_or(filename);

    // Try to find date at the end: "bing-MARKET-YYYY-MM-DD"
    if stem.len() >= 10 {
        let last10 = &stem[stem.len() - 10..];
        // Check if it matches YYYY-MM-DD pattern
        if last10.len() == 10
            && last10.as_bytes()[4] == b'-'
            && last10.as_bytes()[7] == b'-'
            && last10[..4].chars().all(|c| c.is_ascii_digit())
            && last10[5..7].chars().all(|c| c.is_ascii_digit())
            && last10[8..10].chars().all(|c| c.is_ascii_digit())
        {
            return Some(last10.to_string());
        }
    }
    None
}

/// Compute timer status string for display.
fn compute_timer_status_string(config: &Config, timer_state: &TimerState) -> String {
    if !timer_state.enabled {
        return "Disabled".to_string();
    }

    let hour = config.scheduled_hour.min(23);
    let now = Local::now();
    let today_run = now.date_naive().and_time(
        NaiveTime::from_hms_opt(hour, SCHEDULED_MINUTE, 0).unwrap(),
    );
    let next = if now.naive_local() < today_run {
        today_run
    } else {
        today_run + chrono::Duration::days(1)
    };

    let next_dt = next
        .and_local_timezone(Local)
        .single()
        .unwrap_or_else(|| Local::now());

    let last_fetch = timer_state
        .last_fetch_time()
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "Never".to_string());

    format!(
        "Next: {} | Last: {}",
        next_dt.format("%a %b %d %H:%M"),
        last_fetch
    )
}

/// Read the copyright string from a wallpaper's metadata sidecar file.
fn read_wallpaper_copyright(image_path: &str) -> Option<String> {
    let meta_path = std::path::Path::new(image_path).with_extension("meta.json");
    let content = std::fs::read_to_string(meta_path).ok()?;
    let meta: serde_json::Value = serde_json::from_str(&content).ok()?;
    meta.get("copyright").and_then(|v| v.as_str()).map(|s| s.to_string())
}


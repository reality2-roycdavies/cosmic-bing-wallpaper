# Development Notes

This document captures learnings and solutions discovered while developing cosmic-bing-wallpaper for the COSMIC desktop environment. It serves as both documentation and an educational resource for developers working with similar technologies.

## Table of Contents

1. [COSMIC Panel Applet (v0.4.0+)](#cosmic-panel-applet-v040)
2. [System Tray Implementation (Historical, pre-v0.4.0)](#system-tray-implementation)
3. [D-Bus Daemon Architecture](#d-bus-daemon-architecture)
4. [Dark/Light Mode Detection](#darklight-mode-detection)
5. [Installation and Upgrades](#installation-and-upgrades)
6. [Problem-Solving Journey](#problem-solving-journey)
7. [Resources](#resources)

---

## COSMIC Panel Applet (v0.4.0+)

In v0.4.0, the application was converted from a standalone ksni system tray + GUI window architecture to a **native COSMIC panel applet**. This provides tighter desktop integration and removes several dependencies.

### Architecture

```
┌──────────────────────────────────────────────────┐
│            Panel Applet Process                   │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐ │
│  │ D-Bus Svc  │  │  Timer     │  │ Panel Icon │ │
│  │ (service)  │  │ (internal) │  │ + Popup    │ │
│  └────────────┘  └────────────┘  └────────────┘ │
└──────────────────────────────────────────────────┘
        ▲
        │ D-Bus calls
┌───────┴───────────┐
│  Settings Window  │
│  (D-Bus client)   │
└───────────────────┘
```

### Key Implementation Patterns

**Applet entry point:**
```rust
pub fn run_applet() -> cosmic::iced::Result {
    cosmic::applet::run::<BingWallpaperApplet>(())
}
```

**Executor types:**
- Applet: `type Executor = cosmic::SingleThreadExecutor`
- Settings window: `type Executor = cosmic::executor::Default`

**Background service thread:**

The D-Bus service and timer cannot run on the applet's single-threaded executor. They run in a separate `std::thread` with their own `tokio::runtime::Runtime`, communicating via `std::sync::mpsc` channels:

```rust
fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let (event_tx, event_rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(run_background_service(cmd_rx, event_tx));
    });
    // ...
}
```

**Popup rendering:**

COSMIC applets use a closure-based pattern for popup content:
```rust
Message::Surface(app_popup::<BingWallpaperApplet>(
    move |state: &mut BingWallpaperApplet| {
        // Configure popup settings
        state.popup = Some(new_id);
        popup_settings
    },
    Some(Box::new(|state: &BingWallpaperApplet| {
        // Render popup content
        Element::from(state.core.applet.popup_container(
            state.popup_content(),
        ))
        .map(cosmic::Action::App)
    })),
))
```

**Desktop file for panel registration:**
```desktop
[Desktop Entry]
NoDisplay=true
X-CosmicApplet=true
```

The `X-CosmicApplet=true` flag makes the app available in COSMIC's panel applet list. `NoDisplay=true` hides it from application launchers.

### What Was Removed

| Removed | Reason |
|---------|--------|
| `ksni` crate | Panel applets don't need SNI protocol |
| `notify` crate | Panel handles icon theming natively |
| `image` crate | No icon generation needed |
| Lockfile management | COSMIC panel manages applet lifecycle |
| Autostart logic | Panel applets auto-start with the panel |
| Theme file watching | Panel handles icon theming |
| `tray.rs`, `app.rs` | Replaced by `applet.rs`, `settings.rs` |

### COSMIC Symbolic Icons

COSMIC panel icons use SVG files with `#232323` fill (not `currentColor`). The panel system recolors these to match the current theme. Icons should be installed to:
- `hicolor/scalable/apps/` — full-color app icon
- `hicolor/symbolic/apps/` — symbolic panel icon (16x16 viewBox)

---

## System Tray Implementation

> **Note:** This section documents the pre-v0.4.0 system tray architecture using ksni. As of v0.4.0, the app uses a native COSMIC panel applet instead.

### The Challenge

COSMIC desktop uses the StatusNotifierItem (SNI) protocol for system tray icons, implemented via the `ksni` crate. We encountered several significant challenges:

#### Challenge 1: Icon Theme Lookup

**Problem:** COSMIC's StatusNotifierWatcher doesn't reliably find custom icons installed in user directories (`~/.local/share/icons/hicolor/`) during dynamic updates, even though it finds them at startup.

**What we tried:**
1. Installing icons to `~/.local/share/icons/hicolor/symbolic/apps/`
2. Running `gtk-update-icon-cache` to refresh the icon cache
3. Setting `icon_theme_path()` to point to the user icons directory
4. Using simpler icon names without periods (created symlinks)
5. Using standard system icons (worked! `appointment-soon-symbolic`, etc.)

**Discovery:** Standard system icons (like `appointment-soon-symbolic`) worked for dynamic updates, but custom icons did not. This confirmed COSMIC only searches system icon directories during dynamic lookups.

#### Challenge 2: Dynamic Icon Updates

**Problem:** When the timer state changed, we needed the icon to change immediately, but COSMIC cached the icon.

**What we tried:**
1. Calling `handle.update()` to trigger ksni refresh - icon_name changes worked for system icons
2. Changing the tray ID to include state - caused duplicate icons briefly
3. Restarting the tray via systemd - worked but caused flicker

**Discovery:** `handle.update()` does work for `icon_name()` changes when using system icons. The issue was specifically with custom icon lookup.

#### Challenge 3: Menu Label Updates

**Problem:** COSMIC doesn't properly refresh dbusmenu property update signals, so menu labels like "Daily Update: ON" don't update dynamically.

**Workaround:** Use icon changes instead of label changes to indicate state. The menu label is static ("Toggle Daily Update") and the icon shows the current state.

### The Solution: Embedded Pixmap Icons

The breakthrough came when we bypassed icon theme lookup entirely by embedding PNG icons as pixmaps:

```rust
fn icon_pixmap(&self) -> Vec<ksni::Icon> {
    // Choose icon based on state and theme
    let icon_data: &[u8] = match (self.timer_enabled, self.dark_mode) {
        (true, true) => include_bytes!("../resources/icon-on-light.png"),
        (true, false) => include_bytes!("../resources/icon-on.png"),
        (false, true) => include_bytes!("../resources/icon-off-light.png"),
        (false, false) => include_bytes!("../resources/icon-off.png"),
    };

    let img = match image::load_from_memory(icon_data) {
        Ok(img) => img.to_rgba8(),
        Err(_) => return vec![],
    };

    // Convert RGBA to ARGB (network byte order for D-Bus)
    let mut argb_data = Vec::with_capacity((img.width() * img.height() * 4) as usize);
    for pixel in img.pixels() {
        let [r, g, b, a] = pixel.0;
        argb_data.push(a);
        argb_data.push(r);
        argb_data.push(g);
        argb_data.push(b);
    }

    vec![ksni::Icon {
        width: img.width() as i32,
        height: img.height() as i32,
        data: argb_data,
    }]
}
```

**Key implementation details:**

1. **Compile-time embedding:** `include_bytes!()` embeds PNG files in the binary
2. **Image decoding:** The `image` crate decodes PNG to RGBA pixels
3. **Format conversion:** SNI/D-Bus expects ARGB, not RGBA - order matters!
4. **Multiple variants:** Four icon files for all combinations of on/off and dark/light

### Creating Icon Variants

We created light (white) versions by inverting the dark icons:

```python
from PIL import Image

img = Image.open('icon-on.png').convert('RGBA')
pixels = img.load()

for y in range(img.height):
    for x in range(img.width):
        r, g, b, a = pixels[x, y]
        # Invert RGB, preserve alpha
        pixels[x, y] = (255 - r, 255 - g, 255 - b, a)

img.save('icon-on-light.png')
```

**Important lesson:** When we tried runtime inversion (converting all dark pixels to white), both on and off icons ended up looking identical because both were mostly black pixels that all became white. Pre-creating distinct light/dark variants was essential.

---

## D-Bus Daemon Architecture

> **Note:** As of v0.4.0, the D-Bus service is embedded in the panel applet process (see [COSMIC Panel Applet](#cosmic-panel-applet-v040) above). The architecture below documents the evolution through v0.3.x.

### Why a Daemon?

The original monolithic design had problems:
- GUI and tray couldn't communicate state changes
- Timer state was duplicated and could get out of sync
- No instant synchronization between components

### Architecture Evolution

**v0.1.4:** Separate daemon process with systemd services
**v0.3.0:** D-Bus service embedded in tray process
**v0.4.0:** D-Bus service embedded in panel applet process (native COSMIC integration)

### D-Bus Interface Definition

Using `zbus` crate with procedural macros:

```rust
use zbus::interface;

struct WallpaperDaemon {
    config: Config,
}

#[interface(name = "org.cosmicbing.Wallpaper1")]
impl WallpaperDaemon {
    async fn fetch_wallpaper(&self, apply: bool) -> zbus::fdo::Result<String> {
        // Fetch from Bing API and optionally apply
    }

    async fn get_timer_enabled(&self) -> bool {
        // Check systemd timer status
    }

    async fn set_timer_enabled(&mut self, enabled: bool) -> zbus::fdo::Result<()> {
        // Enable/disable systemd timer
    }

    #[zbus(property)]
    fn wallpaper_dir(&self) -> String {
        self.config.wallpaper_dir.clone()
    }
}
```

### D-Bus Service File

For automatic daemon activation, create a service file:

```ini
# ~/.local/share/dbus-1/services/org.cosmicbing.Wallpaper1.service
[D-BUS Service]
Name=org.cosmicbing.Wallpaper1
Exec=/home/user/.local/bin/cosmic-bing-wallpaper --daemon
```

When a client calls the D-Bus interface and the daemon isn't running, D-Bus automatically starts it.

### Avoiding Tokio Runtime Conflicts

**Problem:** The tray uses synchronous callbacks, but D-Bus client is async.

**Wrong approach:**
```rust
// This panics if called from within an async context!
tokio::runtime::Runtime::new().unwrap().block_on(async { ... })
```

**Correct approach:**
```rust
fn some_sync_callback() {
    // Create a new single-threaded runtime for this call
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();

    if let Ok(rt) = rt {
        let result = rt.block_on(async {
            let client = WallpaperClient::connect().await?;
            client.some_method().await
        });
        // Handle result
    }
}
```

**Key insight:** Use `new_current_thread()` instead of `new_multi_thread()` for lighter weight, and handle the case where runtime creation fails.

---

## Dark/Light Mode Detection

### COSMIC Theme Configuration

COSMIC stores theme settings in plain text files:

```
~/.config/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark
```

Contents: `true` or `false`

### Detection Function

```rust
fn is_dark_mode() -> bool {
    // Primary: COSMIC's config file
    if let Some(config_dir) = dirs::config_dir() {
        let path = config_dir.join("cosmic/com.system76.CosmicTheme.Mode/v1/is_dark");
        if let Ok(content) = fs::read_to_string(&path) {
            return content.trim() == "true";
        }
    }

    // Fallback: freedesktop portal
    if let Ok(output) = Command::new("gdbus")
        .args([
            "call", "--session",
            "--dest", "org.freedesktop.portal.Desktop",
            "--object-path", "/org/freedesktop/portal/desktop",
            "--method", "org.freedesktop.portal.Settings.Read",
            "org.freedesktop.appearance", "color-scheme"
        ])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Portal returns: 1 = dark, 2 = light, 0 = no preference
        if stdout.contains("uint32 1") {
            return true;
        } else if stdout.contains("uint32 2") {
            return false;
        }
    }

    // Default to dark (most panels are dark)
    true
}
```

### Instant Theme Change Detection

**Initial approach:** Poll every 2 seconds - worked but had noticeable delay.

**Better approach:** Use `inotify` via the `notify` crate:

```rust
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};

fn setup_theme_watcher(tx: Sender<()>) -> Option<RecommendedWatcher> {
    let watch_dir = dirs::config_dir()?
        .join("cosmic/com.system76.CosmicTheme.Mode/v1");

    let config = NotifyConfig::default()
        .with_poll_interval(Duration::from_secs(1));

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, _>| {
            if let Ok(event) = res {
                if matches!(
                    event.kind,
                    notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                ) {
                    let _ = tx.send(());
                }
            }
        },
        config,
    ).ok()?;

    watcher.watch(&watch_dir, RecursiveMode::NonRecursive).ok()?;
    Some(watcher)
}
```

**Important:** Watch the *directory*, not the file itself. COSMIC may atomically replace the file (write to temp, rename), which wouldn't trigger a modify event on the original file path.

---

## Installation and Upgrades

### The Binary Update Problem

**Problem:** During upgrades, `cp` might prompt for overwrite confirmation, or the old binary might be locked.

**Solution:**
```bash
# Stop services first
systemctl --user stop cosmic-bing-wallpaper-daemon.service 2>/dev/null || true
systemctl --user stop cosmic-bing-wallpaper-tray.service 2>/dev/null || true
pkill -f cosmic-bing-wallpaper 2>/dev/null || true
sleep 1

# Remove old binary before copying
rm -f ~/.local/bin/cosmic-bing-wallpaper
cp target/release/cosmic-bing-wallpaper ~/.local/bin/

# Restart services if they were enabled
if systemctl --user is-enabled cosmic-bing-wallpaper-daemon.service 2>/dev/null; then
    systemctl --user start cosmic-bing-wallpaper-daemon.service
fi
```

### Systemd Service Files

**Daemon service:**
```ini
[Unit]
Description=Bing Wallpaper D-Bus daemon
After=dbus.socket
Requires=dbus.socket

[Service]
Type=simple
ExecStart=%h/.local/bin/cosmic-bing-wallpaper --daemon
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

**Tray service:**
```ini
[Unit]
Description=Bing Wallpaper system tray
After=cosmic-session.target cosmic-bing-wallpaper-daemon.service
Wants=cosmic-bing-wallpaper-daemon.service
PartOf=cosmic-session.target

[Service]
Type=simple
ExecStart=%h/.local/bin/cosmic-bing-wallpaper --tray
Restart=on-failure
RestartSec=5

[Install]
WantedBy=cosmic-session.target
```

**Key points:**
- Use `%h` for home directory (portable across users)
- Tray depends on daemon (`Wants=`, `After=`)
- Both restart on failure for resilience

---

## Problem-Solving Journey

This section documents the iterative debugging process, which is valuable for understanding how solutions were reached.

### Tray Icon Update Saga

**Iteration 1:** Tried using `icon_name()` with custom icons
- Result: Icons found at startup but not during dynamic updates
- Learning: COSMIC caches icon lookups differently for dynamic updates

**Iteration 2:** Tried standard system icons (`appointment-soon-symbolic`)
- Result: Works! Icons change dynamically
- Learning: Only system icon paths are searched during dynamic updates

**Iteration 3:** Tried forcing icon cache refresh with `gtk-update-icon-cache`
- Result: Still didn't work for custom icons
- Learning: The issue isn't the cache, it's COSMIC's lookup behavior

**Iteration 4:** Tried changing tray ID to include state
- Result: Caused brief duplicate icons
- Learning: Changing ID forces re-registration but creates visual glitch

**Iteration 5:** Tried restarting tray via systemd on toggle
- Result: Works but causes flicker/disappearance
- Learning: Process-level restart is too heavy-handed

**Iteration 6:** Implemented `icon_pixmap()` with embedded PNGs (suggested by Gemini)
- Result: Works! Icons change smoothly
- Learning: Bypassing icon theme lookup entirely is the robust solution

**Iteration 7:** Added runtime color inversion for dark mode
- Result: Both icons looked identical (all white)
- Learning: Need pre-created light/dark variants, not runtime inversion

**Iteration 8:** Created separate light icon files
- Result: Success! Different icons for each state and theme
- Learning: Sometimes the straightforward approach is best

### Theme Detection Evolution

**Iteration 1:** Polling every 2 seconds
- Result: Works but 2-second delay is noticeable
- User feedback: "Can we do it without polling?"

**Iteration 2:** Implemented inotify file watching
- Result: Instant response, no CPU overhead
- Learning: File watching is the correct pattern for config changes

---

## Resources

### Crates Used (v0.4.0+)

| Crate | Version | Purpose |
|-------|---------|---------|
| `libcosmic` | git | COSMIC toolkit with applet support |
| `zbus` | 4 | D-Bus communication |
| `tokio` | 1 | Async runtime |
| `reqwest` | 0.12 | HTTP client for Bing API |
| `serde` | 1 | JSON serialization |
| `dirs` | 6 | Standard directory paths |
| `chrono` | 0.4 | Date/time handling |

Previously also used (removed in v0.4.0): `ksni` (system tray), `notify` (file watching), `image` (PNG decoding).

### COSMIC Desktop Internals

- Theme config: `~/.config/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark`
- Background config: `~/.config/cosmic/com.system76.CosmicBackground/v1/all`
- Session target: `cosmic-session.target`

### External Documentation

- [libcosmic](https://github.com/pop-os/libcosmic) - COSMIC toolkit (applet API)
- [zbus book](https://dbus2.github.io/zbus/) - Comprehensive D-Bus guide
- [COSMIC Desktop](https://github.com/pop-os/cosmic-epoch) - Desktop environment source

---

## Important Caveat

This project, like its companion [cosmic-runkat](https://github.com/reality2-roycdavies/cosmic-runkat), is a **small, self-contained desktop application**. The rapid AI-assisted development approach demonstrated here reflects this limited scope:

- Single-purpose functionality (fetch wallpaper, apply it)
- Simple external API (Bing's public image endpoint)
- No authentication or user accounts
- No database or complex state management
- Local-only operation with minimal security considerations

**The findings should not be extrapolated to larger, more complex projects.** Applications involving distributed systems, security-sensitive operations, complex business logic, multi-team coordination, or enterprise requirements would present fundamentally different challenges.

For complex projects, expect:
- More extensive architecture and design phases
- Greater need for human domain expertise
- Security reviews requiring human judgment
- Integration challenges with existing systems
- Performance optimization through profiling
- Team coordination and formal code review

This documentation shows what AI-assisted development looks like for small desktop utilities, not a universal template for all software development.

---

## Contributing

When adding new features or fixing bugs, please update this document with any learnings that might help future developers.

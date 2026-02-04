# Development Notes

This document captures learnings and solutions discovered while developing cosmic-bing-wallpaper for the COSMIC desktop environment. It serves as both documentation and an educational resource for developers working with similar technologies.

## Table of Contents

1. [System Tray Implementation](#system-tray-implementation)
2. [D-Bus Daemon Architecture](#d-bus-daemon-architecture)
3. [Dark/Light Mode Detection](#darklight-mode-detection)
4. [Installation and Upgrades](#installation-and-upgrades)
5. [Problem-Solving Journey](#problem-solving-journey)
6. [Preparing for Flatpak Distribution](#preparing-for-flatpak-distribution)
7. [Resources](#resources)

---

## System Tray Implementation

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

### Why a Daemon?

The original monolithic design had problems:
- GUI and tray couldn't communicate state changes
- Timer state was duplicated and could get out of sync
- No instant synchronization between components

### Architecture

The application uses a **tray-with-embedded-service** architecture:

```
┌──────────────────────────────────────────────────┐
│               Tray Process                        │
│  ┌────────────┐  ┌────────┐  ┌───────────────┐  │
│  │ D-Bus Svc  │  │ Timer  │  │  Tray Icon    │  │
│  │(service.rs)│  │ (async)│  │  (ksni)       │  │
│  └────────────┘  └────────┘  └───────────────┘  │
└──────────────────────────────────────────────────┘
        ▲
        │ D-Bus calls (org.cosmicbing.Wallpaper1)
        │
┌───────┴────────────────┐
│  GUI Process           │
│  (app.rs + dbus_client)│
│  - UI rendering        │
│  - User input          │
│  - D-Bus calls to tray │
└────────────────────────┘
```

Note: The previous daemon+clients model used a separate `daemon.rs` process. This was refactored in v0.3.0 to embed the service directly in the tray process for Flatpak compatibility.

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

## Preparing for Flatpak Distribution

This section documents the journey of making both cosmic-bing-wallpaper and cosmic-runkat Flatpak-compatible for submission to Flathub. The work was done simultaneously for both applications, so the learnings apply to both.

### Interaction Summary

The Flatpak compatibility work involved several key challenges:

1. **Architecture refactoring** - Eliminating systemd dependencies that don't work in Flatpak sandboxes
2. **PID namespace conflicts** - Resolving StatusNotifierItem D-Bus name collisions between sandboxed apps
3. **Path handling** - Adapting to Flatpak's filesystem remapping
4. **Auto-start functionality** - Implementing XDG autostart for login startup
5. **UI cleanup** - Removing features that can't work in sandboxes

### Thematic Analysis

#### Theme 1: Eliminating systemd Dependencies

**The Challenge:** Our original architecture used systemd user services for:
- Running a persistent daemon process
- Timer-based scheduling (daily wallpaper updates)
- Auto-starting on login

**Why This Doesn't Work in Flatpak:** Flatpak sandboxes are isolated from the host's systemd. Apps cannot install or control systemd units from within the sandbox.

**The Solution:** Consolidate everything into the tray process:

```
Before (systemd-based):
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│  systemd timer  │───▶│     daemon      │◀───│   tray/GUI      │
│  (scheduling)   │    │  (D-Bus owner)  │    │  (D-Bus client) │
└─────────────────┘    └─────────────────┘    └─────────────────┘

After (Flatpak-compatible):
┌──────────────────────────────────────────────┐
│               Tray Process                    │
│  ┌────────────┐  ┌────────────┐  ┌────────┐ │
│  │ D-Bus Svc  │  │  Timer     │  │  Tray  │ │
│  │ (owner)    │  │ (internal) │  │  Icon  │ │
│  └────────────┘  └────────────┘  └────────┘ │
└──────────────────────────────────────────────┘
        ▲
        │ D-Bus calls
┌───────┴───────┐
│      GUI      │
│ (D-Bus client)│
└───────────────┘
```

**Implementation Details:**

1. **Internal Timer:** Created `timer.rs` with a tokio-based timer that:
   - Calculates time until next scheduled execution
   - Persists state to a JSON file
   - Fires events via an async channel

2. **Embedded Service:** The `WallpaperService` (D-Bus interface) now lives inside the tray process, not a separate daemon

3. **GUI as Client:** The GUI connects to the tray's D-Bus service to query state and trigger actions

#### Theme 2: PID Namespace Conflicts

**The Problem:** When running both cosmic-runkat and cosmic-bing-wallpaper as Flatpaks simultaneously, only one tray icon would appear. The other would silently fail.

**Root Cause Discovery:** StatusNotifierItem registers D-Bus well-known names based on PID:
```
org.kde.StatusNotifierItem-{PID}-{instance}
```

Inside Flatpak sandboxes, PID namespace isolation means every app sees itself as PID 2. Both apps tried to register:
```
org.kde.StatusNotifierItem-2-1
```

The second app to register would fail because the name was already taken.

**The Solution:** ksni 0.3 added a `disable_dbus_name()` method specifically for Flatpak:

```rust
// Detect Flatpak sandbox
let is_sandboxed = std::path::Path::new("/.flatpak-info").exists();

// Disable well-known name registration in Flatpak
let handle = tray
    .disable_dbus_name(is_sandboxed)
    .spawn()
    .await?;
```

With the well-known name disabled, ksni uses only its unique connection name (`:1.xxx`), which is guaranteed unique per D-Bus connection.

**Verification:**
```bash
# Before fix - both register same name
dbus-send ... ListNames | grep StatusNotifierItem
# org.kde.StatusNotifierItem-2-1  (conflict!)

# After fix - no well-known names, just unique connections
dbus-send ... RegisteredStatusNotifierItems
# :1.5016, :1.5024  (no conflict)
```

#### Theme 3: Flatpak Path Handling

**The Problem:** `dirs::config_dir()` returns different paths:
- Native: `~/.config/`
- Flatpak: `~/.var/app/<app-id>/config/`

Our app needs to:
1. Read COSMIC desktop config (`~/.config/cosmic/`)
2. Write app config that persists across updates
3. Write wallpapers to user-accessible location

**The Solution:** Detect Flatpak and use appropriate paths:

```rust
fn app_config_dir() -> PathBuf {
    if std::path::Path::new("/.flatpak-info").exists() {
        // In Flatpak: use the exposed host config directory
        // (requires --filesystem=~/.config/app-name:create permission)
        dirs::home_dir()
            .map(|h| h.join(".config/cosmic-bing-wallpaper"))
            .unwrap_or_else(|| PathBuf::from("/tmp/cosmic-bing-wallpaper"))
    } else {
        // Native: use standard XDG directory
        dirs::config_dir()
            .map(|d| d.join("cosmic-bing-wallpaper"))
            .unwrap_or_else(|| PathBuf::from("/tmp/cosmic-bing-wallpaper"))
    }
}
```

**Flatpak Manifest Permissions:**
```yaml
finish-args:
  # Read COSMIC theme and background config
  - --filesystem=~/.config/cosmic:rw
  # App config (persists across Flatpak updates)
  - --filesystem=~/.config/cosmic-bing-wallpaper:create
  # Wallpaper storage
  - --filesystem=~/Pictures/bing-wallpapers:create
  # Autostart entry
  - --filesystem=~/.config/autostart:create
```

#### Theme 4: XDG Autostart for Login Startup

**The Problem:** Without systemd user services, how do we start the tray on login?

**The Solution:** XDG autostart specification - place `.desktop` files in `~/.config/autostart/`:

```rust
fn ensure_autostart() {
    let autostart_dir = if is_flatpak() {
        dirs::home_dir().map(|h| h.join(".config/autostart"))
    } else {
        dirs::config_dir().map(|d| d.join("autostart"))
    };

    let desktop_file = autostart_dir?.join("app-id.desktop");

    // Don't overwrite user modifications
    if desktop_file.exists() {
        return;
    }

    let exec_cmd = if is_flatpak() {
        "flatpak run io.github.app-id --tray"
    } else {
        "app-name --tray"
    };

    let content = format!(r#"[Desktop Entry]
Type=Application
Name=App Name
Comment=Description
Exec={exec_cmd}
Icon=app-id
Terminal=false
Categories=Utility;
X-GNOME-Autostart-enabled=true
"#);

    fs::write(&desktop_file, content).ok();
}
```

**Key Design Decisions:**
1. **Self-installing:** Apps create their own autostart entries on first tray run
2. **No overwrite:** If file exists, don't replace it (respects user modifications)
3. **Platform-aware:** Different `Exec` commands for native vs Flatpak
4. **Permission required:** Manifest needs `--filesystem=~/.config/autostart:create`

#### Theme 5: UI Cleanup for Sandbox Compatibility

**The Problem:** Some features don't work in Flatpak sandboxes:
- `open::that()` can't open folders in file manager (needs portal API)
- Arbitrary file browser access is restricted

**The Solution:** Remove non-functional features rather than leaving broken UI:

1. **Removed "Open Wallpaper Folder"** from tray menu
2. **Removed "Open Folder" button** from Downloaded Wallpapers view
3. **Renamed "Open Application" to "Settings ..."** for consistency
4. **Removed `open` crate dependency** (no longer needed)

**Alternative for Future:** Could implement folder opening via portal API:
```rust
// Would use org.freedesktop.portal.OpenURI
// But adds complexity for minor feature
```

#### Theme 6: Launching Processes from Within Flatpak Sandbox

**The Problem:** The tray's "Settings..." menu item wasn't opening the GUI when running in Flatpak.

**Root Cause:** Inside a Flatpak sandbox, the `flatpak` binary isn't available. Code like this fails silently:
```rust
// WRONG - flatpak binary not in sandbox
Command::new("flatpak")
    .args(["run", "io.github.app-id"])
    .spawn()
```

**The Solution:** Use `flatpak-spawn --host` to execute commands on the host system:
```rust
// CORRECT - spawn on host via flatpak-spawn
Command::new("flatpak-spawn")
    .args(["--host", "flatpak", "run", "io.github.app-id"])
    .spawn()
```

**Related Issue: PID Namespace Isolation**

Lockfile-based process detection also breaks in Flatpak:
- Inside sandbox, process sees itself as PID 2 (or similar low number)
- Lockfile contains sandboxed PID (e.g., "2")
- Host can't validate `/proc/2` - that's a kernel thread, not the app!

**Solution:** Skip PID validation in Flatpak, rely on lockfile age instead:
```rust
if !is_flatpak() {
    // Only validate PID on native installs
    if let Ok(pid) = lockfile_content.parse::<u32>() {
        if !Path::new(&format!("/proc/{}", pid)).exists() {
            // Process dead, clean up lockfile
        }
    }
}
```

### Problem-Solving Journey: Flatpak Edition

**Day 1: The Mysterious Missing Icon**

Started by running both Flatpaks simultaneously. Only RunKat appeared. No error messages.

**Investigation:**
1. Checked both processes were running - yes
2. Checked D-Bus service registration - both registered
3. Searched for StatusNotifierItem issues - found the PID naming pattern

**Discovery:** Logged D-Bus names, saw both trying to claim `StatusNotifierItem-2-1`

**Day 2: Finding the ksni Solution**

**Iteration 1:** Tried registering with different instance numbers
- Result: ksni doesn't expose this

**Iteration 2:** Tried different tray IDs
- Result: ID doesn't affect D-Bus name registration

**Iteration 3:** Searched ksni issues and found `disable_dbus_name()` in 0.3
- Result: Success! Both icons now visible

**API Difference:**
```rust
// ksni 0.2 (async)
let handle = tray.spawn().await?;

// ksni 0.3 (async)
let handle = tray.disable_dbus_name(is_sandboxed).spawn().await?;

// ksni 0.3 (blocking - for cosmic-runkat)
let handle = BlockingTrayMethods::disable_dbus_name(tray, is_sandboxed)
    .spawn()?;
```

**Day 3: Config Path Chaos**

**Problem:** Settings weren't persisting across Flatpak runs

**Investigation:**
1. Checked where config was being written
2. Found it going to `~/.var/app/.../config/` (Flatpak sandbox)
3. But we had permission for `~/.config/app-name/`

**Solution:** Created `app_config_dir()` helper to use correct paths based on environment

**Day 4: Autostart Implementation**

**Problem:** How to start tray on login without systemd?

**Research:** XDG autostart specification

**Implementation:** Apps create their own `.desktop` files in `~/.config/autostart/`

**Gotcha:** Needed to add `--filesystem=~/.config/autostart:create` to manifest

### Key Learnings

1. **Flatpak requires architectural thinking** - Can't just package existing app; may need significant refactoring

2. **PID namespaces affect D-Bus naming** - Any D-Bus name containing PID will conflict between sandboxed apps

3. **ksni 0.3 is essential for Flatpak** - The `disable_dbus_name()` feature was added specifically for this use case

4. **Path handling must be Flatpak-aware** - `dirs::config_dir()` is remapped; use explicit paths with appropriate permissions

5. **XDG autostart replaces systemd** - Standard, portable, works across desktop environments

6. **Remove broken features** - Better to remove than leave non-functional UI elements

7. **cargo-sources.json must be regenerated** - When dependencies change, run flatpak-cargo-generator again

### Tools and Commands

**Flatpak cargo source generation:**
```bash
curl -sL "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py" -o fcg.py
python3 fcg.py Cargo.lock -o cargo-sources.json
```

**Build and install Flatpak locally:**
```bash
flatpak-builder --force-clean --user --install build-dir manifest.yml
```

**Test both apps together:**
```bash
flatpak run io.github.app1 --tray &
flatpak run io.github.app2 --tray &
```

**Check StatusNotifierItem registrations:**
```bash
dbus-send --session \
  --dest=org.kde.StatusNotifierWatcher \
  --print-reply \
  /StatusNotifierWatcher \
  org.freedesktop.DBus.Properties.Get \
  string:org.kde.StatusNotifierWatcher \
  string:RegisteredStatusNotifierItems
```

---

## Resources

### Crates Used

| Crate | Version | Purpose |
|-------|---------|---------|
| `ksni` | 0.3 | StatusNotifierItem (system tray) |
| `zbus` | 4 | D-Bus communication |
| `notify` | 6 | File system watching |
| `image` | 0.24 | PNG decoding |
| `tokio` | 1 | Async runtime |
| `dirs` | 6 | Standard directory paths |

### COSMIC Desktop Internals

- Theme config: `~/.config/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark`
- Background config: `~/.config/cosmic/com.system76.CosmicBackground/v1/all`
- Session target: `cosmic-session.target`

### External Documentation

- [ksni crate docs](https://docs.rs/ksni) - StatusNotifierItem implementation
- [zbus book](https://dbus2.github.io/zbus/) - Comprehensive D-Bus guide
- [notify crate docs](https://docs.rs/notify) - Cross-platform file watching
- [COSMIC Desktop](https://github.com/pop-os/cosmic-epoch) - Desktop environment source

### SNI Protocol Notes

- Icon data format: ARGB (Alpha, Red, Green, Blue) - not RGBA!
- Network byte order for multi-byte values
- Icons should be square (common sizes: 22x22, 24x24, 48x48, 64x64)

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

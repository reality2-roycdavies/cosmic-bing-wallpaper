# The Flatpak Journey: From Systemd to Sandbox

This document chronicles the complete journey of making cosmic-bing-wallpaper and cosmic-runkat Flatpak-compatible for distribution via Flathub and the COSMIC Store. What started as "just package it for Flatpak" became a significant architectural refactoring exercise.

**Scope:** Both applications were refactored simultaneously, so learnings apply to both.

---

## Table of Contents

1. [The Starting Point](#the-starting-point)
2. [Why Flatpak Required a Rethink](#why-flatpak-required-a-rethink)
3. [The Architecture Transformation](#the-architecture-transformation)
4. [The PID Namespace Mystery](#the-pid-namespace-mystery)
5. [Path Handling in Sandboxes](#path-handling-in-sandboxes)
6. [Login Startup Without Systemd](#login-startup-without-systemd)
7. [UI Features That Don't Work](#ui-features-that-dont-work)
8. [The Debugging Sessions](#the-debugging-sessions)
9. [Flathub Submission](#flathub-submission)
10. [Key Learnings](#key-learnings)
11. [Reference Commands](#reference-commands)

---

## The Starting Point

### cosmic-bing-wallpaper Architecture (Before)

The original architecture was heavily systemd-dependent:

```
┌─────────────────────────────────────────────────────────────────┐
│                    System Architecture                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌─────────────────┐       ┌─────────────────────────────────┐  │
│  │  systemd timer  │──────▶│         D-Bus Daemon            │  │
│  │  (scheduling)   │       │    (org.cosmicbing.Wallpaper1)  │  │
│  └─────────────────┘       │    - Timer control              │  │
│                            │    - Wallpaper fetching         │  │
│  ┌─────────────────┐       │    - Config management          │  │
│  │ systemd service │──────▶│                                 │  │
│  │ (auto-start)    │       └─────────────────────────────────┘  │
│  └─────────────────┘                     ▲                       │
│                                          │ D-Bus                 │
│                         ┌────────────────┼────────────────┐      │
│                         ▼                ▼                ▼      │
│                   ┌──────────┐    ┌──────────┐    ┌──────────┐  │
│                   │   GUI    │    │   Tray   │    │   CLI    │  │
│                   │ (app.rs) │    │(tray.rs) │    │(fetch)   │  │
│                   └──────────┘    └──────────┘    └──────────┘  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘

Files involved:
- cosmic-bing-wallpaper-daemon.service (systemd unit)
- cosmic-bing-wallpaper-tray.service (systemd unit)
- cosmic-bing-wallpaper.timer (daily schedule)
- org.cosmicbing.Wallpaper1.service (D-Bus activation)
```

### cosmic-runkat Architecture (Before)

Simpler, but also had unnecessary complexity:

```
Files that existed:
- src/daemon.rs (D-Bus service - NEVER USED)
- src/dbus_client.rs (D-Bus client - NEVER USED)
- src/tray.rs (actual tray implementation)
- src/settings.rs (libcosmic settings app)
- src/cpu.rs (CPU monitoring)
```

The daemon and client files were created during development but never integrated. The tray directly read CPU via `CpuMonitor` and settings saved config directly to file.

---

## Why Flatpak Required a Rethink

### The Sandbox Reality

Flatpak sandboxes are isolated environments. When you run a Flatpak app:

1. **Filesystem isolation**: The app sees a different filesystem. `~/.config/` becomes `~/.var/app/<app-id>/config/`
2. **Process isolation**: Apps run in their own PID namespace. Every app sees itself as PID 2.
3. **No systemd access**: Apps cannot install, enable, or control systemd units.
4. **Limited D-Bus access**: Only explicitly permitted D-Bus names are accessible.

### What Broke Immediately

| Feature | Native | Flatpak | Why It Broke |
|---------|--------|---------|--------------|
| Daily timer | systemd timer | Won't start | Can't install systemd units |
| Auto-start on login | systemd service | Won't start | Can't enable systemd services |
| Daemon process | systemd-managed | Won't start | No D-Bus activation |
| Config persistence | `~/.config/app/` | Lost on restart | Wrong path inside sandbox |
| Tray icon | Works | One app disappears | PID namespace conflict |

### The Realization

We couldn't just "package" these apps for Flatpak. We needed to fundamentally rethink the architecture to eliminate systemd dependencies while maintaining all functionality.

---

## The Architecture Transformation

### cosmic-bing-wallpaper: The Full Refactor

**Goal:** Merge daemon functionality into the tray process, replacing systemd with internal equivalents.

#### Step 1: Create Internal Timer (`timer.rs`)

The systemd timer ran daily at 08:00. We needed to replicate this in pure Rust:

```rust
pub struct InternalTimer {
    enabled: Arc<AtomicBool>,
    schedule_hour: u32,
    schedule_minute: u32,
    state_file: PathBuf,
    event_tx: mpsc::Sender<TimerEvent>,
}

impl InternalTimer {
    /// Calculate time until next scheduled run
    fn time_until_next_run(&self) -> Duration {
        let now = Local::now();
        let today_run = now.date_naive().and_hms_opt(
            self.schedule_hour,
            self.schedule_minute,
            0
        ).unwrap();

        let next_run = if now.time() >= today_run.time() {
            // Already passed today, schedule for tomorrow
            today_run + chrono::Duration::days(1)
        } else {
            today_run
        };

        let duration = (next_run - now.naive_local()).to_std()
            .unwrap_or(Duration::from_secs(86400));

        duration
    }

    /// Main timer loop
    async fn run_loop(&self) {
        while self.enabled.load(Ordering::Relaxed) {
            let wait_time = self.time_until_next_run();
            tokio::time::sleep(wait_time).await;

            if self.enabled.load(Ordering::Relaxed) {
                let _ = self.event_tx.send(TimerEvent::Fired).await;
                self.save_state();
            }
        }
    }
}
```

**Key features replicated:**
- Daily scheduling at configurable time (default 08:00)
- State persistence (remembers if enabled/disabled)
- Fires even if app was restarted (catches up on missed runs)

#### Step 2: Embed Service in Tray

The D-Bus daemon became an embedded service:

```rust
// Before: Separate daemon process
pub async fn run_daemon() -> Result<(), Box<dyn Error>> {
    let service = WallpaperService::new();
    let conn = Connection::session().await?;
    conn.object_server().at("/org/cosmicbing/Wallpaper", service).await?;
    conn.request_name("org.cosmicbing.Wallpaper1").await?;
    // ... run forever
}

// After: Embedded in tray
pub struct BingWallpaperTray {
    handle: TrayHandle<Self>,
    // Embedded service components:
    service: Arc<WallpaperService>,
    timer: Arc<InternalTimer>,
    dbus_conn: Option<Connection>,
    // ...
}

impl BingWallpaperTray {
    async fn new() -> Result<Self, Error> {
        let service = Arc::new(WallpaperService::new());
        let timer = Arc::new(InternalTimer::new());

        // Start D-Bus service in this process
        let conn = Connection::session().await.ok();
        if let Some(ref conn) = conn {
            conn.object_server()
                .at("/org/cosmicbing/Wallpaper", service.clone())
                .await?;
            // Don't fail if name already taken (another instance)
            let _ = conn.request_name("org.cosmicbing.Wallpaper1").await;
        }

        // Start timer in background task
        tokio::spawn(timer.clone().run_loop());

        // ... rest of tray setup
    }
}
```

#### Step 3: Rename daemon.rs → service.rs

The file was renamed to reflect its new role as a reusable service component, not a standalone daemon:

```
Before:
  src/daemon.rs    - Standalone D-Bus daemon process
  src/tray.rs      - Thin D-Bus client

After:
  src/service.rs   - D-Bus service (embedded or standalone)
  src/tray.rs      - Tray + embedded service + timer
```

#### Step 4: Remove systemd Code

Deleted all systemd-related functionality:

```rust
// DELETED from service.rs:

fn install_timer() -> Result<(), Error> {
    let service_content = format!(r#"[Unit]
Description=Bing Wallpaper Timer
...
"#);
    fs::write(systemd_path.join("cosmic-bing-wallpaper.timer"), service_content)?;
    Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()?;
    // ...
}

fn uninstall_timer() -> Result<(), Error> {
    Command::new("systemctl")
        .args(["--user", "disable", "--now", "cosmic-bing-wallpaper.timer"])
        .status()?;
    // ...
}
```

### cosmic-runkat: The Cleanup

cosmic-runkat was simpler - we just removed dead code:

```bash
# Files deleted:
rm src/daemon.rs      # D-Bus daemon - never used
rm src/dbus_client.rs # D-Bus client - never used
rmdir systemd/        # Empty directory

# Code removed from main.rs:
- "--daemon" / "-d" mode parsing
- mod daemon declaration
- mod dbus_client declaration
```

The tray already worked correctly without the daemon - it read CPU directly and watched config files for changes.

---

## The PID Namespace Mystery

### The Symptom

After initial Flatpak packaging, running both apps simultaneously produced a strange result:

```bash
$ flatpak run io.github.reality2_roycdavies.cosmic-runkat --tray &
$ flatpak run io.github.reality2_roycdavies.cosmic-bing-wallpaper --tray &

# Result: Only ONE tray icon visible. No error messages.
```

Both processes were running. Both registered with D-Bus successfully. But only one icon appeared.

### The Investigation

We examined the StatusNotifierItem D-Bus interface:

```bash
$ dbus-send --session \
    --dest=org.kde.StatusNotifierWatcher \
    --print-reply \
    /StatusNotifierWatcher \
    org.freedesktop.DBus.Properties.Get \
    string:org.kde.StatusNotifierWatcher \
    string:RegisteredStatusNotifierItems
```

This showed only one item registered. Checking D-Bus names:

```bash
$ dbus-send --session --print-reply \
    --dest=org.freedesktop.DBus \
    /org/freedesktop/DBus \
    org.freedesktop.DBus.ListNames | grep StatusNotifier

# Output: "org.kde.StatusNotifierItem-2-1"
```

### The Root Cause

StatusNotifierItem protocol requires each tray to register a well-known D-Bus name:

```
org.kde.StatusNotifierItem-{PID}-{instance}
```

Inside Flatpak sandboxes, **PID namespace isolation** means every app sees itself as PID 2 (PID 1 is the init system, PID 2 is the app).

Both apps tried to register:
```
org.kde.StatusNotifierItem-2-1
```

The second app's registration silently failed because the name was already taken.

### The Solution

ksni 0.3 (released specifically for this use case) added `disable_dbus_name()`:

```rust
// Detect if we're in a Flatpak sandbox
fn is_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

// In tray initialization:
let is_sandboxed = is_flatpak();

// Async version (cosmic-bing-wallpaper)
let handle = tray
    .disable_dbus_name(is_sandboxed)
    .spawn()
    .await?;

// Blocking version (cosmic-runkat)
let handle = BlockingTrayMethods::disable_dbus_name(tray, is_sandboxed)
    .spawn()?;
```

With the well-known name disabled, each tray uses only its unique D-Bus connection name (`:1.xxx`), which is guaranteed unique per connection.

### Verification

```bash
# After fix:
$ dbus-send ... RegisteredStatusNotifierItems

# Now shows unique connection names:
# ":1.5016"  (cosmic-runkat)
# ":1.5024"  (cosmic-bing-wallpaper)

# Both icons visible!
```

---

## Path Handling in Sandboxes

### The Problem

Inside Flatpak, standard directory functions return sandbox-local paths:

```rust
// Native app:
dirs::config_dir() → ~/.config/

// Flatpak app:
dirs::config_dir() → ~/.var/app/io.github.app-id/config/
```

This broke several things:
1. Config files created inside sandbox weren't visible to native runs
2. Config files from native runs weren't found by Flatpak
3. Wallpapers saved to sandbox weren't accessible in file manager

### The Solution

Created Flatpak-aware path helpers:

```rust
/// Returns the app's config directory, accounting for Flatpak sandboxing
pub fn app_config_dir() -> Option<PathBuf> {
    if is_flatpak() {
        // In Flatpak: use explicit host path
        // This works because we have --filesystem permission for this path
        dirs::home_dir().map(|h| h.join(".config/cosmic-bing-wallpaper"))
    } else {
        // Native: use standard XDG path
        dirs::config_dir().map(|d| d.join("cosmic-bing-wallpaper"))
    }
}

/// Check if running inside Flatpak sandbox
pub fn is_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}
```

### Flatpak Manifest Permissions

The manifest must explicitly grant access to host paths:

```yaml
finish-args:
  # Read/write COSMIC desktop config (for wallpaper setting)
  - --filesystem=~/.config/cosmic:rw

  # App's own config (survives Flatpak updates)
  - --filesystem=~/.config/cosmic-bing-wallpaper:create

  # Wallpaper storage directory
  - --filesystem=~/Pictures/bing-wallpapers:create

  # Autostart entry
  - --filesystem=~/.config/autostart:create

  # For CPU monitoring (cosmic-runkat only)
  - --filesystem=host:ro  # Needed for /proc/stat access
```

**Key insight:** Use `:create` permission for directories the app should create if missing. Use `:ro` for read-only access. Use `:rw` for read-write access to existing directories.

---

## Login Startup Without Systemd

### The Problem

Previously, apps started on login via systemd user services:

```ini
# ~/.config/systemd/user/cosmic-bing-wallpaper-tray.service
[Unit]
Description=Bing Wallpaper Tray
After=cosmic-session.target

[Service]
ExecStart=%h/.local/bin/cosmic-bing-wallpaper --tray

[Install]
WantedBy=cosmic-session.target
```

This doesn't work in Flatpak - apps can't install or control systemd units.

### The Solution: XDG Autostart

The [XDG Autostart specification](https://specifications.freedesktop.org/autostart-spec/autostart-spec-latest.html) provides a desktop-agnostic way to start applications on login.

Apps create `.desktop` files in `~/.config/autostart/`:

```rust
fn ensure_autostart() {
    // Get autostart directory
    let autostart_dir = if is_flatpak() {
        // Flatpak: use explicit host path
        dirs::home_dir().map(|h| h.join(".config/autostart"))
    } else {
        // Native: use XDG path
        dirs::config_dir().map(|d| d.join("autostart"))
    };

    let Some(autostart_dir) = autostart_dir else { return };

    let desktop_file = autostart_dir.join(
        "io.github.reality2_roycdavies.cosmic-bing-wallpaper.desktop"
    );

    // Don't overwrite - respect user modifications
    if desktop_file.exists() {
        return;
    }

    // Ensure directory exists
    let _ = fs::create_dir_all(&autostart_dir);

    // Different Exec command for Flatpak vs native
    let exec_cmd = if is_flatpak() {
        "flatpak run io.github.reality2_roycdavies.cosmic-bing-wallpaper --tray"
    } else {
        "cosmic-bing-wallpaper --tray"
    };

    let content = format!(r#"[Desktop Entry]
Type=Application
Name=Bing Wallpaper
Comment=Daily Bing wallpaper for COSMIC desktop
Exec={exec_cmd}
Icon=io.github.reality2_roycdavies.cosmic-bing-wallpaper
Terminal=false
Categories=Utility;
X-GNOME-Autostart-enabled=true
NoDisplay=true
"#);

    let _ = fs::write(&desktop_file, content);
}
```

**Called on first tray startup:**
```rust
fn main() {
    match args.get(1).map(|s| s.as_str()) {
        Some("--tray") | Some("-t") => {
            ensure_autostart();  // Create autostart entry if needed
            run_tray()?;
        }
        // ...
    }
}
```

### Design Decisions

1. **Self-installing:** Apps create their own autostart entries rather than requiring user setup
2. **No overwrite:** If file exists, don't replace (respects user modifications)
3. **Platform-aware:** Different `Exec` for native vs Flatpak
4. **NoDisplay=true:** Hides from application launchers (it's a background service)
5. **Permission required:** Manifest needs `--filesystem=~/.config/autostart:create`

---

## UI Features That Don't Work

### The Problem

Some features relied on capabilities that don't work in Flatpak:

1. **"Open Wallpaper Folder"** - Used `open::that()` to launch file manager
2. **"Open Folder" button** - Same issue in history view

Inside Flatpak, `open::that()` fails because:
- The file manager is outside the sandbox
- There's no portal setup for folder opening

### The Solution: Remove Rather Than Break

Instead of leaving broken UI elements, we removed them:

```rust
// REMOVED from tray menu:
StandardItem {
    label: "Open Wallpaper Folder".into(),
    activate: Box::new(|_| {
        // This doesn't work in Flatpak
        let _ = open::that(&config.wallpaper_dir);
    }),
    ..Default::default()
}

// REMOVED from GUI:
button::text("Open Folder").on_press(Message::OpenWallpaperFolder)
```

Also renamed "Open Application" to "Settings ..." for consistency with cosmic-runkat.

### Alternative: Portal API

For future enhancement, folder opening could use the portal API:

```rust
// Could use org.freedesktop.portal.OpenURI
// But adds complexity for a minor convenience feature
```

---

## The Debugging Sessions

### Day 1: Initial Flatpak Build

**Goal:** Get basic Flatpak builds working.

**Challenges:**
1. Generated `cargo-sources.json` with flatpak-cargo-generator
2. Discovered libcosmic git dependencies need special handling
3. Fixed manifest SDK version (25.08)

**Result:** Both apps built and ran individually.

### Day 2: The Disappearing Icon

**Goal:** Run both Flatpaks simultaneously.

**Symptom:** Only one tray icon appeared.

**Investigation:**
```bash
# Check processes
ps aux | grep cosmic
# Both running ✓

# Check D-Bus registrations
dbus-send ... ListNames | grep StatusNotifier
# Only one: org.kde.StatusNotifierItem-2-1

# AHA! Both have PID 2 inside sandbox
cat /proc/self/status | grep ^Pid
# Pid: 2
```

**Solution:** Found ksni 0.3's `disable_dbus_name()`.

### Day 3: Config Not Persisting

**Goal:** Settings should survive app restarts.

**Symptom:** Settings reset on every launch.

**Investigation:**
```bash
# Where is config being written?
strace -e openat flatpak run ... 2>&1 | grep config

# Found: ~/.var/app/.../config/cosmic-bing-wallpaper/
# But we granted: ~/.config/cosmic-bing-wallpaper/
```

**Solution:** Created `app_config_dir()` that uses explicit host paths in Flatpak.

### Day 4: Autostart Not Working

**Goal:** Tray should start on login.

**Symptom:** Had to manually start after each login.

**Investigation:**
1. systemd services don't work in Flatpak ✗
2. XDG autostart should work ✓
3. But app wasn't creating the file
4. Missing filesystem permission!

**Solution:** Added `--filesystem=~/.config/autostart:create` to manifest.

### Day 5: Final Testing

**Verification checklist:**
- [ ] Both Flatpaks run simultaneously → ✓ Two tray icons visible
- [ ] Timer enables/disables → ✓ State persists
- [ ] Settings persist across restarts → ✓ Config in correct location
- [ ] Autostart on login → ✓ XDG autostart works
- [ ] Native and Flatpak share config → ✓ Same ~/.config/app/ path

---

## Flathub Submission

### App ID Requirements

Flathub requires specific app ID formats:

- **GitHub:** `io.github.USERNAME.REPONAME`
- **GitLab:** `io.gitlab.USERNAME.REPONAME`

Our app IDs:
- `io.github.reality2_roycdavies.cosmic-runkat`
- `io.github.reality2_roycdavies.cosmic-bing-wallpaper`

**Note:** Underscores in GitHub usernames become underscores in app ID.

### Linter Exceptions

Flathub's linter flagged some issues:

```bash
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest manifest.yml
```

**Issues requiring exceptions:**

1. **`finish-args-arbitrary-dbus-access`** - We use session bus for StatusNotifierItem
   - Exception: Required for system tray functionality

2. **`finish-args-host-ro-filesystem-access`** (cosmic-runkat only) - We need `/proc/stat`
   - Exception: Required for CPU monitoring functionality

### Manifest Template

```yaml
app-id: io.github.reality2_roycdavies.cosmic-bing-wallpaper
runtime: org.freedesktop.Platform
runtime-version: '25.08'
sdk: org.freedesktop.Sdk
sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable
command: cosmic-bing-wallpaper

finish-args:
  - --share=ipc
  - --share=network
  - --socket=fallback-x11
  - --socket=wayland
  - --socket=session-bus
  - --filesystem=~/.config/cosmic:rw
  - --filesystem=~/.config/cosmic-bing-wallpaper:create
  - --filesystem=~/Pictures/bing-wallpapers:create
  - --filesystem=~/.config/autostart:create

modules:
  - name: cosmic-bing-wallpaper
    buildsystem: simple
    build-options:
      append-path: /usr/lib/sdk/rust-stable/bin
      env:
        CARGO_HOME: /run/build/cosmic-bing-wallpaper/cargo
    build-commands:
      - cargo --offline fetch --manifest-path Cargo.toml --verbose
      - cargo --offline build --release --verbose
      - install -Dm755 target/release/cosmic-bing-wallpaper /app/bin/
      - install -Dm644 resources/*.desktop /app/share/applications/
      - install -Dm644 resources/*.metainfo.xml /app/share/metainfo/
      - install -Dm644 resources/*.svg /app/share/icons/hicolor/scalable/apps/
    sources:
      - type: dir
        path: .
      - cargo-sources.json
```

---

## Key Learnings

### Architectural Learnings

1. **Design for portability from the start** - Avoiding systemd dependencies would have saved significant refactoring

2. **Embedded > Distributed for desktop apps** - A single process with embedded components is simpler than multiple communicating processes

3. **XDG standards work everywhere** - Autostart specification is supported by COSMIC, GNOME, KDE, and others

4. **Remove broken features** - Better to remove functionality than leave broken UI

### Technical Learnings

1. **PID namespaces affect D-Bus naming** - Any D-Bus name containing PID will conflict between sandboxed apps

2. **ksni 0.3 is essential for Flatpak** - The `disable_dbus_name()` method exists specifically for this

3. **Path functions are remapped** - `dirs::config_dir()` returns different paths in Flatpak

4. **Explicit paths with permissions** - Use host paths directly and grant explicit permissions

5. **Regenerate cargo-sources.json** - Must regenerate when dependencies change

### Process Learnings

1. **Test both apps together** - PID conflict only appeared when running simultaneously

2. **Check actual file paths** - `strace` revealed where config was really being written

3. **Read sandbox documentation** - Understanding Flatpak's isolation model was essential

---

## Reference Commands

### Flatpak Development

```bash
# Generate cargo sources
curl -sL "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py" -o fcg.py
python3 fcg.py Cargo.lock -o cargo-sources.json

# Build and install locally
flatpak-builder --force-clean --user --install build-dir manifest.yml

# Run
flatpak run io.github.reality2_roycdavies.cosmic-bing-wallpaper --tray

# Uninstall
flatpak uninstall io.github.reality2_roycdavies.cosmic-bing-wallpaper

# Lint manifest
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest manifest.yml
```

### Debugging

```bash
# Check StatusNotifierItem registrations
dbus-send --session \
  --dest=org.kde.StatusNotifierWatcher \
  --print-reply \
  /StatusNotifierWatcher \
  org.freedesktop.DBus.Properties.Get \
  string:org.kde.StatusNotifierWatcher \
  string:RegisteredStatusNotifierItems

# List D-Bus names
dbus-send --session --print-reply \
  --dest=org.freedesktop.DBus \
  /org/freedesktop/DBus \
  org.freedesktop.DBus.ListNames | grep -E "(StatusNotifier|cosmicbing|runkat)"

# Check if running in Flatpak
ls /.flatpak-info 2>/dev/null && echo "In Flatpak" || echo "Native"

# Watch file operations
strace -e openat,stat -f flatpak run ... 2>&1 | grep -E "(config|autostart)"
```

---

## Conclusion

What started as "package for Flatpak" became a significant architectural exercise. The key transformation was moving from a systemd-dependent multi-process architecture to a self-contained single-process design with embedded components.

The result is more portable and arguably simpler:
- Single process to manage
- No external dependencies on systemd
- Works identically on any Linux desktop
- Easier to understand and debug

The trade-off is that the tray must be running for the timer to work. In a systemd setup, the timer would fire even if no user was logged in. In practice, for desktop applications like these, this is rarely a concern - if no user is logged in, there's no one to see the wallpaper anyway.

---

*This document was created as part of the AI-assisted development process for cosmic-bing-wallpaper and cosmic-runkat. The complete Flatpak preparation was done in collaboration between Dr. Roy C. Davies and Claude using Claude Code.*

# Development Journey: cosmic-bing-wallpaper

This document chronicles the development of cosmic-bing-wallpaper from initial concept to release, created collaboratively between [Dr. Roy C. Davies](https://roycdavies.github.io) and Claude (Anthropic's AI). It's intended as an educational resource for developers interested in building desktop applications for the COSMIC desktop environment.

## The Initial Idea

**Goal:** Create a simple application that fetches the daily Bing wallpaper and sets it as the desktop background on the COSMIC desktop environment (System76's new Rust-based desktop for Pop!_OS).

**Why this project?**
- Demonstrates practical COSMIC/libcosmic development
- Solves a real user need (many people enjoy Bing's daily photography)
- Shows how to integrate a GUI app with system services
- Provides a template for similar wallpaper/background apps

## Technology Stack

### Core Technologies

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Language | **Rust** | COSMIC is written in Rust; native integration |
| GUI Toolkit | **libcosmic** | Official COSMIC UI toolkit (wraps Iced) |
| Async Runtime | **Tokio** | Required by reqwest; battle-tested |
| HTTP Client | **reqwest** | Popular, async, good JSON support |
| System Tray | **ksni** | StatusNotifierItem protocol for Linux trays |

### Architecture Pattern

The app follows the **Model-View-Update (MVU)** pattern, which is native to Iced/libcosmic:

```
User Action → Message → Update (modify state) → View (render UI)
```

This is similar to Elm architecture and provides:
- Predictable state management
- Easy debugging (all state changes go through `update()`)
- Clear separation between logic and presentation

## Code Review & Bug Fixes

When Claude first reviewed the codebase, several issues were identified:

### 1. Invalid Rust Edition (Critical)

**Problem:** `Cargo.toml` specified `edition = "2024"` which doesn't exist.

```toml
# Before (broken)
edition = "2024"

# After (fixed)
edition = "2021"
```

**Lesson:** Always verify Cargo.toml settings. Rust editions are released every 3 years (2015, 2018, 2021, 2024 expected later).

### 2. Hardcoded Paths in Systemd Service

**Problem:** Service file used absolute path `/home/roycdavies/...`

```ini
# Before (only works for one user)
ExecStart=/home/roycdavies/.local/bin/cosmic-bing-wallpaper

# After (works for any user)
ExecStart=%h/.local/bin/cosmic-bing-wallpaper
```

**Lesson:** Use systemd specifiers (`%h` = home directory, `%U` = user ID) for portable service files.

**Additional pitfall:** When generating service files from shell scripts, be careful with heredocs:

```bash
# WRONG - $HOME expands at creation time, hardcoding the path
cat > service.file << EOF
ExecStart=$HOME/.local/bin/app
EOF

# CORRECT - use quoted heredoc to prevent expansion
cat > service.file << 'EOF'
ExecStart=%h/.local/bin/app
EOF
```

The quoted `'EOF'` prevents shell variable expansion, preserving the `%h` specifier for systemd to interpret at runtime.

### 3. Filename Pattern Mismatch

**Problem:** Shell script saved files as `bing-YYYY-MM-DD.jpg`, but Rust app expected `bing-{market}-YYYY-MM-DD.jpg`.

**Fix:** Aligned both to use the market-prefixed format for consistency.

**Lesson:** When multiple components interact, document and verify naming conventions.

### 4. Date Extraction Bug

**Problem:** `scan_history()` was extracting dates incorrectly from filenames.

```rust
// Before (wrong - took wrong substring)
let date = filename.get(5..15)?;

// After (correct - last 10 chars before extension)
let date = stem.chars().rev().take(10).collect::<String>()
    .chars().rev().collect::<String>();
```

**Lesson:** String manipulation is error-prone. Write tests for parsing functions.

## Feature Development

### Phase 1: Basic Functionality
- GUI window with wallpaper history grid
- Fetch button to download today's wallpaper
- Apply button to set selected wallpaper
- Market selection (different regions have different images)

### Phase 2: CLI Mode
Added `--fetch-and-apply` flag for headless operation:

```rust
match args[1].as_str() {
    "--fetch-and-apply" | "-f" => {
        // Run without GUI
        fetch_and_apply_wallpaper().await?;
        return Ok(());
    }
    // ...
}
```

### Phase 3: System Tray (v0.1.1)

Added background tray icon using ksni crate:

```rust
impl Tray for BingWallpaperTray {
    fn icon_name(&self) -> String {
        "io.github.cosmic-bing-wallpaper-symbolic".to_string()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            // Fetch Today's Wallpaper
            // Open Application
            // Open Folder
            // Quit
        ]
    }
}
```

**Challenge:** COSMIC uses Wayland, which doesn't have a native tray protocol. The ksni crate uses D-Bus StatusNotifierItem protocol, which COSMIC supports.

### Phase 4: Autostart Issues (v0.1.2)

**Problem:** XDG autostart (`~/.config/autostart/*.desktop`) didn't work on COSMIC.

**Root cause:** COSMIC manages its session via systemd, not traditional XDG autostart.

**Solution:** Create systemd user services that start with `cosmic-session.target`:

```ini
[Unit]
After=cosmic-session.target
PartOf=cosmic-session.target

[Install]
WantedBy=cosmic-session.target
```

### Phase 5: Daily Update Integration (v0.1.3)

Added tray menu toggle for daily updates:

```rust
fn is_timer_enabled() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-enabled", "cosmic-bing-wallpaper.timer"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
```

The tray now shows "Daily Update: On/Off" and clicking toggles the systemd timer.

### Phase 6: D-Bus Daemon Architecture (v0.1.4)

**Problem:** The tray and GUI couldn't communicate state changes. When you toggled the timer in the GUI, the tray icon didn't update. Each component managed its own state, leading to synchronization issues.

**Solution:** Refactored to a daemon+clients architecture:

```
                    D-Bus (org.cosmicbing.Wallpaper1)
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
   GUI Client            Daemon              Tray Client
   (app.rs)           (daemon.rs)            (tray.rs)
```

**New files created:**
- `daemon.rs` - D-Bus service with `org.cosmicbing.Wallpaper1` interface
- `dbus_client.rs` - Client proxy for GUI and tray to call daemon

**D-Bus interface using zbus:**

```rust
#[interface(name = "org.cosmicbing.Wallpaper1")]
impl WallpaperDaemon {
    async fn fetch_wallpaper(&self, apply: bool) -> zbus::fdo::Result<String>;
    async fn get_timer_enabled(&self) -> bool;
    async fn set_timer_enabled(&mut self, enabled: bool) -> zbus::fdo::Result<()>;
    #[zbus(property)]
    fn wallpaper_dir(&self) -> String;
}
```

**Challenge:** Calling async D-Bus methods from synchronous tray callbacks.

**Wrong approach:**
```rust
// Panics if called from within an async context!
tokio::runtime::Runtime::new().unwrap().block_on(async { ... })
```

**Correct approach:**
```rust
let rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build();
if let Ok(rt) = rt {
    rt.block_on(async {
        let client = WallpaperClient::connect().await?;
        client.some_method().await
    })
}
```

### Phase 7: Theme-Aware Tray Icons (v0.1.4)

**Problem:** COSMIC's StatusNotifierWatcher doesn't reliably find custom icons installed in user directories during dynamic updates, even though it finds them at startup.

**What we tried:**
1. Using `icon_name()` with custom icons - found at startup but not during updates
2. Using standard system icons (`appointment-soon-symbolic`) - worked for dynamic updates
3. Restarting tray via systemd - worked but caused flicker

**Breakthrough:** Suggested by Gemini - use `icon_pixmap()` to embed icons directly:

```rust
fn icon_pixmap(&self) -> Vec<ksni::Icon> {
    let icon_data: &[u8] = match (self.timer_enabled, self.dark_mode) {
        (true, true) => include_bytes!("../resources/icon-on-light-24.png"),
        (true, false) => include_bytes!("../resources/icon-on-24.png"),
        (false, true) => include_bytes!("../resources/icon-off-light-24.png"),
        (false, false) => include_bytes!("../resources/icon-off-24.png"),
    };

    let img = image::load_from_memory(icon_data)?.to_rgba8();

    // CRITICAL: Convert RGBA to ARGB - SNI expects ARGB, not RGBA!
    let mut argb_data = Vec::new();
    for pixel in img.pixels() {
        let [r, g, b, a] = pixel.0;
        argb_data.extend_from_slice(&[a, r, g, b]);  // ARGB order
    }

    vec![ksni::Icon { width, height, data: argb_data }]
}
```

**Theme detection:** Read COSMIC's config file directly:

```rust
fn is_dark_mode() -> bool {
    let path = dirs::config_dir()?.join("cosmic/com.system76.CosmicTheme.Mode/v1/is_dark");
    fs::read_to_string(&path).map(|s| s.trim() == "true").unwrap_or(true)
}
```

**Instant theme updates:** Using `notify` crate for inotify file watching instead of polling:

```rust
let mut watcher = RecommendedWatcher::new(move |event| {
    if matches!(event.kind, Modify(_) | Create(_)) {
        tx.send(()).ok();
    }
}, config)?;
watcher.watch(&theme_config_dir, RecursiveMode::NonRecursive)?;
```

**Important:** Watch the *directory*, not the file. COSMIC may atomically replace the file (write to temp, rename), which wouldn't trigger a modify event on the original file path.

### Phase 8: Colored Status Indicators (v0.1.5)

**Problem:** At small panel sizes (levels 1-3), the tick/cross indicators were too small to see clearly.

**Initial attempt:** Created multiple icon sizes (16px through 64px) with bolder indicators for smaller sizes. COSMIC appears to just scale one icon rather than selecting the appropriate size.

**Final solution:** Single 24px icon with colored indicators:
- **Green tick** for timer enabled (ON state)
- **Red cross** for timer disabled (OFF state)
- **White background circle** behind indicator for contrast at all sizes

```python
# Creating icons with PIL
if is_on:
    draw.line([...], fill=(0, 180, 0, 255), width=2)  # Green tick
else:
    draw.line([...], fill=(200, 0, 0, 255), width=2)  # Red cross
```

**External state sync:** The tray now polls timer state every ~1 second to catch changes made by the GUI or other tools:

```rust
timer_check_counter += 1;
if timer_check_counter >= 20 {  // 20 × 50ms = 1 second
    timer_check_counter = 0;
    let current = is_timer_enabled();
    if current != last_timer_enabled {
        handle.update(|tray| tray.timer_enabled = current);
    }
}
```

## Testing Performed

### Manual Testing

Throughout development, we performed extensive manual testing:

1. **Build verification**
   - `cargo build` - debug builds for fast iteration
   - `cargo build --release` - release builds before tagging

2. **GUI testing**
   - Launch app, verify window appears correctly
   - Test all buttons (Fetch, Apply, market dropdown)
   - Verify wallpaper grid displays history correctly

3. **CLI testing**
   ```bash
   ./cosmic-bing-wallpaper --fetch-and-apply
   # Verified: wallpaper downloads and applies
   ```

4. **System tray testing**
   ```bash
   ./cosmic-bing-wallpaper --tray &
   # Verified: tray icon appears
   # Tested: all menu items work
   ```

5. **Autostart testing**
   - Log out and log in
   - Verify tray icon appears automatically
   - **Found bug:** XDG autostart doesn't work on COSMIC
   - **Fixed:** Switched to systemd services

6. **Systemd service testing**
   ```bash
   systemctl --user status cosmic-bing-wallpaper-tray.service
   systemctl --user status cosmic-bing-wallpaper.timer
   # Verified: services start correctly
   ```

7. **Timer toggle testing**
   - Click "Daily Update" in tray menu
   - Verify timer enables/disables via `systemctl --user is-enabled`

8. **AppImage testing**
   - Build AppImage with `just build-appimage`
   - Run `./install-appimage.sh --with-tray`
   - Verify app appears in launcher
   - Verify tray starts on login

### What We Didn't Test (Future Work)

- Automated unit tests (would add with `#[cfg(test)]`)
- Integration tests for Bing API changes
- Multi-monitor wallpaper behavior
- Different COSMIC versions

## Packaging & Release

### AppImage Creation

We used `appimagetool` to create portable Linux packages:

```bash
# Build structure
AppDir/
├── AppRun
├── cosmic-bing-wallpaper.desktop
├── io.github.cosmic-bing-wallpaper.svg
└── usr/
    └── bin/
        └── cosmic-bing-wallpaper
```

### Icon Naming Convention

COSMIC uses Freedesktop icon naming. We created:
- `io.github.cosmic-bing-wallpaper.svg` - Full color app icon
- `io.github.cosmic-bing-wallpaper-symbolic.svg` - Monochrome tray icon

The `symbolic` suffix tells COSMIC to use theme-appropriate coloring.

### Release Process

1. Update version in `Cargo.toml`
2. Update `Cargo.lock`: `cargo update --package cosmic-bing-wallpaper`
3. Commit: `git commit -m "Bump version to X.Y.Z"`
4. Tag: `git tag -a vX.Y.Z -m "Release description"`
5. Push: `git push && git push origin vX.Y.Z`
6. Create GitHub release: `gh release create vX.Y.Z --title "..." --notes "..."`

## Lessons Learned

1. **COSMIC is different** - It's not GNOME or KDE. Don't assume XDG standards work; check COSMIC-specific docs.

2. **Systemd is your friend** - For COSMIC, systemd user services are more reliable than desktop autostart files.

3. **Test early autostart** - This was the most common failure mode. Test login/logout cycles early.

4. **Symbolic icons matter** - System trays need `-symbolic` variants for proper theme integration.

5. **Iterate with releases** - We went through v0.1.0 → v0.1.5 fixing real-world issues. Ship early, fix fast.

6. **Document as you go** - This file exists because we tracked decisions throughout development.

7. **Embedded pixmaps bypass icon lookup issues** - When icon theme lookup fails, embedding icons directly in the binary via `include_bytes!()` and `icon_pixmap()` is the robust solution.

8. **Watch directories, not files** - When watching for config changes, watch the parent directory. Atomic file replacement (write temp, rename) won't trigger events on the original file.

9. **RGBA vs ARGB matters** - D-Bus StatusNotifierItem expects ARGB byte order, not RGBA. Getting this wrong produces corrupted icons.

10. **State sync requires architecture** - When multiple components need to share state (GUI, tray), a daemon+clients model with D-Bus provides cleaner synchronization than polling or file-based communication.

11. **Color beats shape at small sizes** - For tray icons at small panel sizes, colored indicators (green tick/red cross) are more visible than monochrome shape differences.

## Project Structure (Final)

```
cosmic-bing-wallpaper/
├── README.md                 # User-facing documentation
├── DEVELOPMENT.md            # This file (high-level journey)
├── THEMATIC-ANALYSIS.md      # Patterns in AI-human collaboration
├── RETROSPECTIVE.md          # What worked, what didn't
├── install-appimage.sh       # AppImage installer script
├── transcripts/              # Conversation transcripts
│   ├── CONVERSATION-PART1-CREATION.md
│   ├── CONVERSATION-PART2-REFINEMENT.md
│   ├── CONVERSATION-PART3-CODE-REVIEW.md
│   └── CONVERSATION-PART4-ARCHITECTURE.md
└── cosmic-bing-wallpaper/    # Main Rust project
    ├── Cargo.toml
    ├── CHANGELOG.md          # Version history
    ├── DEVELOPMENT.md        # Technical learnings (detailed)
    ├── justfile              # Build/install recipes
    ├── src/
    │   ├── main.rs           # Entry point, CLI handling
    │   ├── app.rs            # GUI application (MVU pattern)
    │   ├── bing.rs           # Bing API client
    │   ├── config.rs         # Configuration management
    │   ├── daemon.rs         # D-Bus daemon service
    │   ├── dbus_client.rs    # D-Bus client proxy
    │   └── tray.rs           # System tray with theme-aware icons
    ├── resources/
    │   ├── *.desktop         # Desktop entry files
    │   ├── *.svg             # Application icons
    │   └── icon-*.png        # Tray icons (on/off, dark/light)
    └── appimage/
        └── build-appimage.sh # AppImage build script
```

## Tools Used

- **Claude Code** (Anthropic's CLI) - AI pair programming
- **Gemini** (Google) - Suggested pixmap approach for tray icons
- **Rust/Cargo** - Build system
- **just** - Task runner (like make, but simpler)
- **gh** - GitHub CLI for releases
- **appimagetool** - AppImage creation
- **systemctl** - Systemd service management
- **Python/PIL** - Icon generation and manipulation
- **D-Bus** - Inter-process communication for daemon architecture

---

*This project was developed as a collaboration between [Dr. Roy C. Davies](https://roycdavies.github.io) and Claude, demonstrating how AI can assist with real-world software development while the human maintains creative control and makes final decisions.*

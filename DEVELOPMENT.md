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

5. **Iterate with releases** - We went through v0.1.0 → v0.1.3 fixing real-world issues. Ship early, fix fast.

6. **Document as you go** - This file exists because we tracked decisions throughout development.

## Project Structure (Final)

```
cosmic-bing-wallpaper/
├── README.md                 # User-facing documentation
├── DEVELOPMENT.md            # This file
├── install-appimage.sh       # AppImage installer script
└── cosmic-bing-wallpaper/    # Main Rust project
    ├── Cargo.toml
    ├── justfile              # Build/install recipes
    ├── src/
    │   ├── main.rs           # Entry point, CLI handling
    │   ├── app.rs            # GUI application (MVU pattern)
    │   ├── wallpaper.rs      # Bing API and wallpaper logic
    │   └── tray.rs           # System tray implementation
    ├── resources/
    │   ├── *.desktop         # Desktop entry files
    │   └── *.svg             # Application icons
    └── appimage/
        └── build-appimage.sh # AppImage build script
```

## Tools Used

- **Claude Code** (Anthropic's CLI) - AI pair programming
- **Rust/Cargo** - Build system
- **just** - Task runner (like make, but simpler)
- **gh** - GitHub CLI for releases
- **appimagetool** - AppImage creation
- **systemctl** - Systemd service management

---

*This project was developed as a collaboration between [Dr. Roy C. Davies](https://roycdavies.github.io) and Claude, demonstrating how AI can assist with real-world software development while the human maintains creative control and makes final decisions.*

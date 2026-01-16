# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.5] - 2026-01-17

### Changed

- **Tray Icon Indicators**: Switched to colored indicators for better visibility
  - Green tick for timer enabled (auto-update ON)
  - Red cross for timer disabled (auto-update OFF)
  - White background circle behind indicator for contrast at all sizes
  - Original mountain/landscape icon design preserved
  - High-quality 24px icons scaled from 64px using LANCZOS resampling

---

## [0.1.4] - 2026-01-16

### Added

- **D-Bus Daemon Architecture**: Complete refactoring to daemon+clients model
  - New `daemon.rs` implementing `org.cosmicbing.Wallpaper1` D-Bus service
  - New `dbus_client.rs` for client-side D-Bus proxy
  - Tray now communicates with daemon via D-Bus for instant synchronization
  - D-Bus service file for automatic daemon activation

- **Theme-Aware Tray Icons**: System tray icons adapt to dark/light mode
  - Light (white) icons for dark mode panels
  - Dark (black) icons for light mode panels
  - Instant theme detection via inotify file watching (no polling)
  - Icons embedded as pixmaps to bypass COSMIC icon theme lookup issues

- **Improved Tray Toggle Feedback**: Timer on/off state now visually reflected
  - Different icons for enabled vs disabled states
  - Smooth icon transitions without tray restart

- **File System Watching**: Using `notify` crate for instant theme change detection
  - Watches COSMIC theme config file directly
  - No CPU overhead from polling

### Changed

- **Tray Service Management**: Now managed via systemd user services
  - `cosmic-bing-wallpaper-daemon.service` for background D-Bus daemon
  - `cosmic-bing-wallpaper-tray.service` for system tray
  - Proper service dependencies and restart handling

- **Installation Scripts**: Improved upgrade handling
  - Services stopped before binary update
  - Old binary removed before copying new one (avoids overwrite prompts)
  - Services automatically restarted after upgrade

### Technical Details

#### D-Bus Interface

The daemon exposes these methods via `org.cosmicbing.Wallpaper1`:

```
Methods:
  FetchWallpaper(apply: bool) -> String
  GetTimerEnabled() -> bool
  SetTimerEnabled(enabled: bool)
  GetWallpaperDir() -> String
  GetCurrentWallpaper() -> String
```

#### Theme Detection

COSMIC stores theme preference at:
```
~/.config/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark
```

Values: `true` for dark mode, `false` for light mode.

#### Icon Pixmap Format

SNI protocol expects ARGB format (not RGBA):
```rust
// Convert RGBA to ARGB
for pixel in img.pixels() {
    let [r, g, b, a] = pixel.0;
    argb_data.extend_from_slice(&[a, r, g, b]);
}
```

### Dependencies Added

- `zbus = "4"` - D-Bus library for daemon/client communication
- `notify = "6"` - File system watching for theme changes
- `image = "0.24"` - PNG decoding for embedded icons

### Known Issues

- COSMIC's StatusNotifierWatcher doesn't reliably find custom icons in user directories during dynamic updates (workaround: embedded pixmaps)
- Menu labels in dbusmenu don't refresh dynamically in COSMIC (workaround: use icon changes for state indication)

---

## [0.1.3] - 2026-01-16

### Added
- Initial system tray implementation using ksni crate
- Timer toggle from tray menu
- Open application and wallpaper folder from tray

### Fixed
- Various bug fixes from code review

---

## [0.1.2] - 2026-01-15

### Added
- GUI application with libcosmic
- History browser for past wallpapers
- Region selector for 21 Bing markets
- Systemd timer integration

---

## [0.1.1] - 2026-01-15

### Added
- AppImage packaging
- Desktop entry and icons

---

## [0.1.0] - 2026-01-15

### Added
- Initial release
- Shell script for basic functionality
- Core Bing API integration

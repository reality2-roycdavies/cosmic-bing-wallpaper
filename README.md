# Bing Wallpaper for COSMIC Desktop

A daily Bing wallpaper manager for the [COSMIC desktop environment](https://system76.com/cosmic) on Linux. Automatically fetches Microsoft Bing's beautiful "Image of the Day" and sets it as your desktop wallpaper.

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Rust](https://img.shields.io/badge/rust-2021-orange.svg)
![COSMIC](https://img.shields.io/badge/desktop-COSMIC-purple.svg)

This project was built collaboratively using AI-assisted development with [Claude Code](https://claude.ai/code). See [docs/AI-ASSISTED-DEVELOPMENT.md](docs/AI-ASSISTED-DEVELOPMENT.md) for the full story, lessons learned, and conversation transcripts.

---

## Overview

This project includes both a simple shell script for quick use and a native COSMIC panel applet with full settings integration.

## Features

### Panel Applet (v0.4.0+)
- **Native COSMIC Integration**: Lives directly in the COSMIC panel as a native applet
- **Quick Popup**: Click the panel icon for instant access to controls
- **Fetch Wallpaper**: Download and apply today's Bing image with one click
- **Timer Toggle**: Enable/disable daily automatic updates from the popup
- **Status Display**: Shows timer state and next scheduled update

### Inline Settings (via cosmic-applet-settings hub)
- **Image Preview**: See the current wallpaper directly in the settings page
- **Copyright Display**: Shows photographer attribution from Bing metadata
- **History Browser**: Scrollable list of previously downloaded wallpapers with Apply/Delete buttons
- **Delete Confirmation**: Destructive actions require confirmation before executing
- **Region Selector**: Choose from 21 Bing markets (US, UK, Germany, Japan, etc.)
- **Configurable Update Time**: Set the daily auto-update hour (0-23)
- **Live Status**: Timer status refreshes automatically every 10 seconds
- **One-click Fetch**: Fetch today's wallpaper button above the history list

### Standalone Settings Window
- Full settings window with image preview and history browser
- Can be launched independently via `--settings-standalone`

### Shell Script
- Lightweight alternative for automation
- Can be run via cron or systemd timer
- No dependencies beyond curl and Python 3

## Screenshots

![Inline Settings](screenshots/settings-inline.png)

*Bing Wallpaper settings in the cosmic-applet-settings hub — image preview, copyright, timer status, region selector, and wallpaper history with Apply/Delete actions.*

![Main Window](screenshots/Screenshot_2026-02-06_20-51-29.png)

*Panel applet popup and standalone settings window.*

![History Browser](screenshots/Screenshot_2026-02-06_20-52-11.png)

*Browse and re-apply previously downloaded wallpapers.*

## Installation

### From Source

#### Prerequisites

- [Rust](https://rustup.rs/) toolchain (1.75+)
- [just](https://github.com/casey/just) command runner
- System libraries:

```bash
# Debian/Ubuntu/Pop!_OS
sudo apt install libwayland-dev libxkbcommon-dev libssl-dev pkg-config just

# Fedora
sudo dnf install wayland-devel libxkbcommon-devel openssl-devel just

# Arch
sudo pacman -S wayland libxkbcommon openssl just
```

#### Build and Install

```bash
# Clone the repository
git clone https://github.com/reality2-roycdavies/cosmic-bing-wallpaper.git
cd cosmic-bing-wallpaper

# Build release binary
just build-release

# Install binary, desktop entry, and icons to ~/.local
just install-local
```

#### Other just commands

```bash
just build-debug       # Debug build
just run               # Build debug and run
just run-release       # Build release and run
just check             # Run clippy checks
just fmt               # Format code
just clean             # Clean build artifacts
just uninstall-local   # Remove installed files
```

Then add the applet to your panel via **Panel Settings > Applets**.

### Uninstalling

```bash
just uninstall-local
```

## CLI Usage

```bash
# Run as COSMIC panel applet (default, no arguments)
cosmic-bing-wallpaper

# Open settings (via hub or standalone)
cosmic-bing-wallpaper --settings

# Open standalone settings window directly
cosmic-bing-wallpaper --settings-standalone

# Fetch and apply wallpaper (one-shot, no GUI)
cosmic-bing-wallpaper --fetch

# Settings hub protocol (used by cosmic-applet-settings)
cosmic-bing-wallpaper --settings-describe
cosmic-bing-wallpaper --settings-set market '"en-US"'
cosmic-bing-wallpaper --settings-action fetch
cosmic-bing-wallpaper --settings-action apply_history /path/to/wallpaper.jpg
cosmic-bing-wallpaper --settings-action delete_history /path/to/wallpaper.jpg

# Show version or help
cosmic-bing-wallpaper --version
cosmic-bing-wallpaper --help
```

## Automatic Daily Updates

### From the Panel Applet or Settings

1. Click the applet icon in the panel, or open Settings
2. Toggle "Daily Auto-Update" to enable automatic updates
3. Optionally adjust the update hour (defaults to 08:00)
4. The wallpaper will automatically update daily at the configured time

The timer runs within the applet process — no systemd services required. The applet starts automatically with the COSMIC panel.

## Configuration

Configuration is stored at `~/.config/cosmic-bing-wallpaper/config.json`:

| Option | Description | Default |
|--------|-------------|---------|
| `wallpaper_dir` | Directory where images are saved | `~/Pictures/BingWallpapers/` |
| `market` | Regional Bing market code (e.g., "en-US") | `en-US` |
| `auto_update` | Whether the daily update timer is enabled | `false` |
| `keep_days` | Days to keep old wallpapers before cleanup (0 = keep forever) | `30` |
| `fetch_on_startup` | Automatically fetch today's image when app starts | `true` |
| `scheduled_hour` | Hour of day (0-23) for the daily auto-update timer | `8` |

Timer state is stored separately at `~/.config/cosmic-bing-wallpaper/timer_state.json`.

Downloaded wallpapers are saved with a `.meta.json` sidecar containing the Bing copyright, title, date, and market metadata.

## Supported Regions

| Region | Code | Region | Code |
|--------|------|--------|------|
| Australia | en-AU | Japan | ja-JP |
| Brazil | pt-BR | Netherlands | nl-NL |
| Canada | en-CA | New Zealand | en-NZ |
| China | zh-CN | Norway | nb-NO |
| Denmark | da-DK | Poland | pl-PL |
| Finland | fi-FI | Russia | ru-RU |
| France | fr-FR | South Korea | ko-KR |
| Germany | de-DE | Spain | es-ES |
| India | en-IN | Sweden | sv-SE |
| Italy | it-IT | United Kingdom | en-GB |
| | | United States | en-US |

## Project Structure

```
cosmic-bing-wallpaper/
├── Cargo.toml                         # Rust dependencies
├── README.md                          # This file
├── LICENSE                            # MIT License
├── src/
│   ├── main.rs                        # Entry point (applet/settings/fetch/CLI protocol)
│   ├── applet.rs                      # COSMIC panel applet with popup
│   ├── settings.rs                    # Standalone settings window (full UI)
│   ├── settings_cli.rs               # CLI settings protocol (--settings-describe/set/action)
│   ├── settings_page.rs              # Legacy embeddable settings page
│   ├── bing.rs                        # Bing API client + metadata sidecar
│   ├── config.rs                      # Configuration & markets
│   ├── service.rs                     # D-Bus service + wallpaper operations
│   ├── timer.rs                       # Internal timer for daily updates (configurable hour)
│   └── dbus_client.rs                 # D-Bus client proxy (for settings)
└── resources/
    ├── *.desktop                      # Desktop entry (X-CosmicApplet)
    ├── *.svg                          # Application icons
    └── *.metainfo.xml                 # AppStream metadata
```

## How It Works

### Bing API

The app fetches images from Microsoft Bing's Homepage Image Archive API:
```
https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1&mkt=en-US
```

This returns JSON with the daily image URL, title, and copyright information.

### COSMIC Desktop Integration

Wallpapers are applied by:
1. Writing configuration to `~/.config/cosmic/com.system76.CosmicBackground/v1/all`
2. Using COSMIC's RON (Rusty Object Notation) format
3. Restarting the `cosmic-bg` process to load the new wallpaper

### Settings Hub Integration

The applet integrates with [cosmic-applet-settings](https://github.com/reality2-roycdavies/cosmic-applet-settings) via a CLI protocol:

1. **Describe**: `--settings-describe` outputs a JSON schema with image preview, settings controls, and history list
2. **Set**: `--settings-set` updates configuration values
3. **Action**: `--settings-action` handles fetch, apply, and delete operations (with optional item ID for per-item actions)
4. **Refresh**: The hub re-runs describe every 10 seconds to update timer status and reflect new wallpapers

## Development

### Technology Stack
- **Rust** - Systems programming language
- **libcosmic** - COSMIC desktop toolkit (based on iced) with applet support
- **tokio** - Async runtime for non-blocking operations
- **reqwest** - HTTP client for API calls
- **serde** - JSON serialization/deserialization
- **zbus** - D-Bus IPC for applet/settings communication
- **chrono** - Date/time handling for timer and metadata

### Architecture

The application uses a **panel applet** architecture with an embedded D-Bus service (v0.4.0+):

```
┌──────────────────────────────────────────────────┐
│            Panel Applet Process                   │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐ │
│  │ D-Bus Svc  │  │  Timer     │  │ Panel Icon │ │
│  │ (service)  │  │ (internal) │  │ + Popup    │ │
│  └────────────┘  └────────────┘  └────────────┘ │
└──────────────────────────────────────────────────┘
        ▲                                  ▲
        │ D-Bus calls                      │ CLI protocol
┌───────┴───────────┐        ┌─────────────┴──────────┐
│  Settings Window  │        │  cosmic-applet-settings │
│  (D-Bus client)   │        │  (settings hub)         │
└───────────────────┘        └────────────────────────┘
```

#### Components

| Component | File | Purpose |
|-----------|------|---------|
| **Applet** | `applet.rs` | Native COSMIC panel applet with popup. Embeds D-Bus service and timer. |
| **Settings** | `settings.rs` | Standalone settings window with image preview, history browser, region selector. |
| **Settings CLI** | `settings_cli.rs` | CLI protocol for inline settings via the hub (describe/set/action). |
| **Service** | `service.rs` | D-Bus service managing wallpaper operations. |
| **Timer** | `timer.rs` | Internal async timer for daily updates with configurable hour. |
| **D-Bus Client** | `dbus_client.rs` | Proxy for settings window to communicate with applet. |

#### Key Design Points

- The applet runs as a native COSMIC panel applet (auto-starts with the panel)
- D-Bus service and timer run in a background thread within the applet process
- The settings window is a separate process launched via `--settings`
- The settings hub renders inline settings via the CLI protocol — no separate window needed
- No systemd, autostart, or lockfile management needed — COSMIC panel handles lifecycle

### Technical Documentation

See [DEVELOPMENT.md](DEVELOPMENT.md) for detailed technical learnings including:
- Panel applet implementation (v0.4.0+)
- D-Bus service architecture
- COSMIC desktop internals

## Related COSMIC Applets

This is part of a suite of custom applets for the COSMIC desktop, configurable via the [unified settings app](https://github.com/reality2-roycdavies/cosmic-applet-settings):

| Applet | Description |
|--------|-------------|
| **[cosmic-applet-settings](https://github.com/reality2-roycdavies/cosmic-applet-settings)** | Unified settings app for the applet suite |
| **[cosmic-runkat](https://github.com/reality2-roycdavies/cosmic-runkat)** | Animated running cat CPU indicator for the panel |
| **[cosmic-pie-menu](https://github.com/reality2-roycdavies/cosmic-pie-menu)** | Radial/pie menu app launcher with gesture support |
| **[cosmic-tailscale](https://github.com/reality2-roycdavies/cosmic-tailscale)** | Tailscale VPN status and control applet |
| **[cosmic-hotspot](https://github.com/reality2-roycdavies/cosmic-hotspot)** | WiFi hotspot toggle applet |

### Other Related Projects

| Project | Description |
|---------|-------------|
| **[cosmic-konnect](https://github.com/reality2-roycdavies/cosmic-konnect)** | Device connectivity and sync between Linux and Android |
| **[cosmic-konnect-android](https://github.com/reality2-roycdavies/cosmic-konnect-android)** | Android companion app for Cosmic Konnect |

## License

MIT License - See [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## Acknowledgments

- [System76](https://system76.com) for the COSMIC desktop environment
- [Microsoft Bing](https://bing.com) for the beautiful daily images
- [Anthropic](https://anthropic.com) and Claude for AI-assisted development

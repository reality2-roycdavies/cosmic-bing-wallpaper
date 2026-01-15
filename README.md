# Bing Wallpaper for COSMIC Desktop

A daily Bing wallpaper manager for the [COSMIC desktop environment](https://system76.com/cosmic) on Linux. Automatically fetches Microsoft Bing's beautiful "Image of the Day" and sets it as your desktop wallpaper.

This project includes both a simple shell script for quick use and a full native COSMIC GUI application.

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Rust](https://img.shields.io/badge/rust-2021-orange.svg)
![COSMIC](https://img.shields.io/badge/desktop-COSMIC-purple.svg)

## Features

### GUI Application
- **Image Preview**: See today's Bing image before applying
- **History Browser**: Browse and re-apply previously downloaded wallpapers
- **Region Selector**: Choose from 21 Bing markets (US, UK, Germany, Japan, etc.)
- **One-click Apply**: Set any image as your desktop wallpaper instantly
- **Auto-Update Timer**: Install/uninstall systemd timer directly from the app
- **Status Display**: Shows next scheduled update time

### Shell Script
- Lightweight alternative for automation
- Can be run via cron or systemd timer
- No dependencies beyond curl and Python 3

## Screenshots

*Coming soon*

## Quick Start

### Option 1: AppImage (Easiest)

Download the latest AppImage from the [Releases](../../releases) page:

```bash
# Download the AppImage and installer
wget https://github.com/reality2-roycdavies/cosmic-bing-wallpaper/releases/latest/download/cosmic-bing-wallpaper-x86_64.AppImage
wget https://github.com/reality2-roycdavies/cosmic-bing-wallpaper/releases/latest/download/install-appimage.sh

# Run the installer (integrates with app launcher)
chmod +x install-appimage.sh
./install-appimage.sh cosmic-bing-wallpaper-x86_64.AppImage
```

The installer will:
- Create `~/Apps` directory if needed
- Copy the AppImage there
- Extract and install the icon
- Create a `.desktop` file so it appears in your application launcher

**Or run directly without installing:**
```bash
chmod +x cosmic-bing-wallpaper-x86_64.AppImage
./cosmic-bing-wallpaper-x86_64.AppImage
```

### Option 2: Build from Source

#### Prerequisites

**COSMIC Desktop**: This app requires the COSMIC desktop environment (Pop!_OS 24.04+ or another COSMIC-enabled distribution).

**Build Dependencies** (Pop!_OS/Ubuntu/Debian):
```bash
sudo apt update
sudo apt install -y \
    build-essential \
    cargo \
    cmake \
    git \
    just \
    libexpat1-dev \
    libfontconfig-dev \
    libfreetype-dev \
    libxkbcommon-dev \
    pkg-config
```

**Build Dependencies** (Fedora):
```bash
sudo dnf install -y \
    cargo \
    cmake \
    expat-devel \
    fontconfig-devel \
    freetype-devel \
    gcc \
    git \
    just \
    libxkbcommon-devel \
    pkg-config
```

**Build Dependencies** (Arch Linux):
```bash
sudo pacman -S --needed \
    base-devel \
    cargo \
    cmake \
    expat \
    fontconfig \
    freetype2 \
    git \
    just \
    libxkbcommon \
    pkg-config
```

#### Clone and Build

```bash
# Clone the repository
git clone https://github.com/reality2-roycdavies/cosmic-bing-wallpaper.git
cd cosmic-bing-wallpaper/cosmic-bing-wallpaper

# Build release version
cargo build --release

# Run the application
./target/release/cosmic-bing-wallpaper
```

#### Install Locally

```bash
# Using just (recommended)
just install-local

# Or manually copy to your PATH
cp target/release/cosmic-bing-wallpaper ~/.local/bin/
```

### Option 3: Shell Script Only

If you just want a simple script without the GUI:

```bash
# Clone or download the script
git clone https://github.com/reality2-roycdavies/cosmic-bing-wallpaper.git
cd cosmic-bing-wallpaper

# Make executable and run
chmod +x bing-wallpaper.sh
./bing-wallpaper.sh
```

Edit the script to change your preferred region (default is `en-US`).

## Automatic Daily Updates

### From the GUI App

1. Open the application
2. Scroll to the "Daily Auto-Update" section
3. Click **"Install Auto-Update Timer"**
4. The wallpaper will automatically update daily at 8:00 AM

### Manual Timer Installation

```bash
cd cosmic-bing-wallpaper/cosmic-bing-wallpaper/systemd

# Install the timer
./install-timer.sh

# Check status
systemctl --user status cosmic-bing-wallpaper.timer

# Uninstall
./uninstall-timer.sh
```

## Configuration

### GUI App
Configuration is stored at `~/.config/cosmic-bing-wallpaper/config.json`:
- **Wallpaper directory**: Where images are saved (default: `~/Pictures/BingWallpapers/`)
- **Market**: Which regional Bing market to use
- **Auto-update**: Whether the timer is enabled

### Shell Script
Edit the variables at the top of `bing-wallpaper.sh`:
- `WALLPAPER_DIR`: Where to save images
- `MARKET`: Bing region code (e.g., `en-US`, `de-DE`, `ja-JP`)

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
├── README.md                          # This file
├── LICENSE                            # MIT License
├── bing-wallpaper.sh                  # Standalone shell script
└── cosmic-bing-wallpaper/             # COSMIC GUI application
    ├── Cargo.toml                     # Rust dependencies
    ├── justfile                       # Build automation
    ├── install.sh                     # Installation script
    ├── src/
    │   ├── main.rs                    # Entry point (GUI/CLI modes)
    │   ├── app.rs                     # COSMIC app (UI + state)
    │   ├── bing.rs                    # Bing API client
    │   └── config.rs                  # Configuration & markets
    ├── resources/
    │   ├── *.desktop                  # Desktop entry file
    │   ├── *.svg                      # Application icon
    │   └── *.appdata.xml              # AppStream metadata
    ├── systemd/
    │   ├── cosmic-bing-wallpaper.service
    │   ├── cosmic-bing-wallpaper.timer
    │   ├── install-timer.sh
    │   └── uninstall-timer.sh
    ├── appimage/
    │   └── build-appimage.sh          # AppImage build script
    └── i18n/                          # Internationalization files
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

## Building an AppImage

```bash
cd cosmic-bing-wallpaper/cosmic-bing-wallpaper/appimage
./build-appimage.sh
```

The AppImage will be created in `appimage/build/`.

## Development

### Technology Stack
- **Rust** - Systems programming language
- **libcosmic** - COSMIC desktop GUI toolkit (based on iced)
- **tokio** - Async runtime for non-blocking operations
- **reqwest** - HTTP client for API calls
- **serde** - JSON serialization/deserialization

### Architecture
The GUI app follows the Model-View-Update (MVU) pattern:
- **Model** (`BingWallpaper`): Application state
- **View** (`view_main`, `view_history`): UI rendering
- **Update** (`update`): Message handling and state transitions

### Build Commands
```bash
just build-release    # Build optimized binary
just install-local    # Install to ~/.local/bin
just run-release      # Build and run
just check            # Run clippy lints
just fmt              # Format code
```

## About This Project

This project was created collaboratively with [Claude](https://claude.ai) (Anthropic's AI assistant) using [Claude Code](https://claude.ai/code). It demonstrates AI-assisted development of:

- Native Linux desktop applications using Rust
- Integration with the COSMIC desktop environment
- Shell scripts for system automation
- Systemd timer configuration
- AppImage packaging

## License

MIT License - See [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## Acknowledgments

- [System76](https://system76.com) for the COSMIC desktop environment
- [Microsoft Bing](https://bing.com) for the beautiful daily images
- [Anthropic](https://anthropic.com) and Claude for AI-assisted development

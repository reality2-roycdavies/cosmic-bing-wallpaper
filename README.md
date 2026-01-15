# Bing Wallpaper for COSMIC Desktop

A daily Bing wallpaper manager for the [COSMIC desktop environment](https://system76.com/cosmic) on Linux. Automatically fetches Microsoft Bing's beautiful "Image of the Day" and sets it as your desktop wallpaper.

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Rust](https://img.shields.io/badge/rust-2021-orange.svg)
![COSMIC](https://img.shields.io/badge/desktop-COSMIC-purple.svg)

---

## About This Project

> **This project is an educational showcase of AI-assisted software development.**

This application was created collaboratively by [Dr. Roy C. Davies](https://roycdavies.github.io) and [Claude](https://claude.ai) (Anthropic's AI) using [Claude Code](https://claude.ai/code). From initial idea to fully functional released application—complete with GUI, system tray, systemd integration, and AppImage packaging—in just a few hours of conversation.

**The experiment:** The rule was that the human would write *no code at all*—not even comments, not even git commands. Every line of code, every commit, every file edit was performed by Claude. The human's role was purely to direct, question, test, and decide.

### Developer Reflection

*From Dr. Roy C. Davies:*

> I have learned very little about the actual mechanics of what the app does. Claude wrote the Rust code, the systemd service files, the build scripts. I directed, reviewed, tested, and made decisions—but I couldn't reproduce this from scratch without AI assistance.
>
> **Does that matter?** Perhaps not. Software development has always been about standing on the shoulders of giants. AI assistance is simply the next evolution.
>
> **Key insight:** While Claude is clever and solves problems well, **it still requires someone to ask the right questions**. The AI doesn't know what you want until you articulate it. It doesn't know something is broken until you test it and report back.
>
> **Testing has become even more important than before.** When you don't fully understand the code, your ability to verify it works correctly becomes your primary contribution. Knowing *what* to test, *how* to test it, and recognising when something isn't quite right—these skills are now more valuable than ever.

### What the Thematic Analysis Revealed

A thematic analysis of our conversation transcripts (also performed by Claude, but only after I asked for it) identified key patterns:

| Theme | Finding |
|-------|---------|
| **Human as Quality Gate** | Every significant bug was discovered through my testing, not by Claude |
| **Iterative Debugging** | Problems like image centering took 8+ attempts; autostart took 5 iterations |
| **Platform Knowledge Gaps** | Claude had general knowledge but missed COSMIC-specific details |
| **The Cost of Abstraction** | The "last 20%" (packaging, icons, autostart) consumed disproportionate effort |
| **Organic Scope Evolution** | The project grew from shell script → GUI → tray → systemd through dialogue |

The emerging model of AI-assisted development:

| Role | AI | Human |
|------|:---:|:-----:|
| Write code | ✓ | |
| Fix compilation errors | ✓ | |
| Propose solutions | ✓ | |
| Test in real environment | | ✓ |
| Recognise incorrect behaviour | | ✓ |
| Make final decisions | | ✓ |
| Know when to stop | | ✓ |

The human becomes an **editor, tester, and director**—roles that require understanding *what* software should do, even without knowing *how* to implement it.

*See [THEMATIC-ANALYSIS.md](THEMATIC-ANALYSIS.md) for the complete analysis.*

### Educational Resources

| Resource | Description |
|----------|-------------|
| [DEVELOPMENT.md](DEVELOPMENT.md) | Technical journey from concept to release |
| [THEMATIC-ANALYSIS.md](THEMATIC-ANALYSIS.md) | Analysis of patterns in AI-human collaboration |
| [transcripts/](transcripts/) | Complete conversation logs (raw JSONL + readable Markdown) |

---

## Overview

This project includes both a simple shell script for quick use and a full native COSMIC GUI application.

## Features

### GUI Application
- **Image Preview**: See today's Bing image before applying
- **History Browser**: Browse and re-apply previously downloaded wallpapers
- **Region Selector**: Choose from 21 Bing markets (US, UK, Germany, Japan, etc.)
- **One-click Apply**: Set any image as your desktop wallpaper instantly
- **Auto-Update Timer**: Install/uninstall systemd timer directly from the app
- **Status Display**: Shows next scheduled update time

### System Tray Mode
- **Background Operation**: Run quietly in the system tray
- **Quick Access**: Right-click menu for common actions
- **Fetch Wallpaper**: Download today's image without opening the full app
- **Open App**: Launch the full GUI when needed

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
- Extract and install the icons
- Create a `.desktop` file so it appears in your application launcher

**Install with system tray autostart:**
```bash
./install-appimage.sh --with-tray cosmic-bing-wallpaper-x86_64.AppImage
```

This also sets up the tray icon to start automatically on login.

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
    libdbus-1-dev \
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
    dbus-devel \
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
    dbus \
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

## System Tray Mode

Run the app in the background with a system tray icon for quick access:

```bash
# From installed binary
cosmic-bing-wallpaper --tray

# Or from AppImage
./cosmic-bing-wallpaper-x86_64.AppImage --tray
```

The tray icon provides a right-click menu with:
- **Fetch Today's Wallpaper**: Download and apply the latest Bing image
- **Open Application**: Launch the full GUI window
- **Open Wallpaper Folder**: Browse downloaded images
- **Quit**: Exit the tray application

### Auto-start Tray on Login

**Using just (recommended for source builds):**
```bash
just install-with-tray
```

This installs the app and sets up the tray to start automatically on login.

**Manual setup:**
```bash
mkdir -p ~/.config/autostart
cat > ~/.config/autostart/cosmic-bing-wallpaper-tray.desktop << EOF
[Desktop Entry]
Name=Bing Wallpaper Tray
Exec=cosmic-bing-wallpaper --tray
Type=Application
X-GNOME-Autostart-enabled=true
EOF
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
    │   ├── main.rs                    # Entry point (GUI/CLI/Tray modes)
    │   ├── app.rs                     # COSMIC app (UI + state)
    │   ├── bing.rs                    # Bing API client
    │   ├── config.rs                  # Configuration & markets
    │   └── tray.rs                    # System tray implementation
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

## License

MIT License - See [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## Acknowledgments

- [System76](https://system76.com) for the COSMIC desktop environment
- [Microsoft Bing](https://bing.com) for the beautiful daily images
- [Anthropic](https://anthropic.com) and Claude for AI-assisted development

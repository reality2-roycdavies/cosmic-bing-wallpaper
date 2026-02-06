# Bing Wallpaper for COSMIC Desktop

A daily Bing wallpaper manager for the [COSMIC desktop environment](https://system76.com/cosmic) on Linux. Automatically fetches Microsoft Bing's beautiful "Image of the Day" and sets it as your desktop wallpaper.

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Rust](https://img.shields.io/badge/rust-2021-orange.svg)
![COSMIC](https://img.shields.io/badge/desktop-COSMIC-purple.svg)

> **Just want the app?** Skip to [Quick Start](#quick-start) for installation instructions.

---

## About This Project

> **This project is an educational showcase of AI-assisted software development.**

This application was created collaboratively by [Dr. Roy C. Davies](https://roycdavies.github.io) and [Claude](https://claude.ai) (Anthropic's AI) using [Claude Code](https://claude.ai/code). From initial idea to fully functional released application—complete with GUI, system tray, and AppImage packaging—in approximately **8 hours of active work** spread across four sessions. An additional **~4 hours** were later spent refactoring for Flatpak compatibility (see [FLATPAK-JOURNEY.md](docs/FLATPAK-JOURNEY.md)).

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

### Skills Required for AI-Assisted Development

**What skills does a human need to replicate this project?**

The human needs *technical literacy* but not *programming expertise*. Think of it as the difference between being able to read a map versus being a cartographer.

| Skill Category | Required | Not Required |
|---------------|----------|--------------|
| **Programming** | Ability to read code and understand *what* it does | Writing code, knowing syntax, understanding libraries |
| **Technical concepts** | Understanding of files, paths, processes, services | Deep knowledge of any specific technology |
| **Linux** | Comfort with terminal, basic commands (`cd`, `ls`, `cargo build`) | System administration, shell scripting |
| **Architecture** | Grasp of how software components fit together | Design patterns, framework internals |
| **Testing** | Methodical approach: try things, observe, report clearly | Automated testing, debugging tools |
| **Communication** | Precise description of problems and desired outcomes | Technical jargon or implementation details |

**The technical skill level:**

You need to be someone who:
- Can follow technical instructions without hand-holding
- Understands that software has configuration files, services, and dependencies
- Can recognise when error output indicates a problem (even without understanding the details)
- Is comfortable running commands in a terminal
- Can describe what they observe precisely ("the image appears in the right third of the box" vs "it's broken")

You do **not** need to:
- Know Rust, Python, or any specific language
- Understand the libcosmic framework or iced GUI toolkit
- Know how systemd works internally
- Be able to write or debug code yourself

**Approximate skill level:** A technically-inclined person who has installed Linux, configured some software, and isn't afraid of the command line. Perhaps someone who has done light scripting or web development, or a "power user" who enjoys tinkering. Not a professional developer, but not computer-naive either.

**The minimum viable skill set:**
1. **Technical comfort** — not intimidated by terminals, config files, or error messages
2. **Methodical testing** — systematically verify functionality, observe carefully, report precisely
3. **Domain understanding** — know what the software should do from a user's perspective
4. **Clear communication** — articulate requirements and problems without ambiguity
5. **Patience and persistence** — some problems take multiple iterations to solve

**What would this project take without AI assistance?**

For a solo developer with moderate Rust experience:
- Learning libcosmic/iced framework: **1-2 weeks**
- Core application development: **1-2 weeks**
- Panel applet / system tray implementation: **3-5 days**
- Systemd integration and packaging: **3-5 days**
- Testing and bug fixing: **1 week**
- **Total estimate: 4-6 weeks**

For a developer new to Rust:
- Learning Rust basics: **2-4 weeks**
- Plus all the above: **4-6 weeks**
- **Total estimate: 6-10 weeks**

With AI assistance, the initial scope was completed in **~8 hours of active work**, plus **~4 hours** for Flatpak refactoring—a productivity multiplier of roughly **30-50x** for this type of project.

### Lessons Learned (Retrospective)

After completion, we analysed what worked and what could have been better:

| What Worked | What Didn't |
|-------------|-------------|
| Organic, iterative development | Excessive iteration on "simple" problems (8+ attempts for image centering) |
| Human testing caught every real bug | Platform knowledge gaps (COSMIC specifics learned by trial and error) |
| The "no code" rule forced clear communication | Best practices not applied automatically (had to retrofit later) |
| Dedicated code review phase found 13 issues | All testing was manual; no automation |
| Documentation created alongside development | Scope grew without explicit milestone acknowledgments |

**Key insight for future projects:** Earlier and more frequent code reviews, upfront platform research, and explicit prompts for "what could go wrong?" would have made the process smoother.

*See [docs/RETROSPECTIVE.md](docs/RETROSPECTIVE.md) for the complete analysis.*

### What the Thematic Analysis Revealed

A thematic analysis of our conversation transcripts (also performed by Claude, but only after I asked for it) identified key patterns across 6 development sessions:

| Theme | Finding |
|-------|---------|
| **Human as Quality Gate** | Every significant bug was discovered through my testing, not by Claude |
| **Iterative Debugging** | Problems like image centering took 8+ attempts; autostart took 5 iterations |
| **Platform Knowledge Gaps** | Claude had general knowledge but missed COSMIC-specific details |
| **The Cost of Abstraction** | The "last 20%" (packaging, icons, autostart) consumed disproportionate effort |
| **Organic Scope Evolution** | The project grew from shell script → GUI → tray → systemd through dialogue |
| **External Knowledge Integration** | Solutions sometimes came from other AI tools (e.g., Gemini suggested pixmap approach) |
| **Architecture Emerges from Pain** | The daemon+clients model emerged only after synchronization problems became clear |
| **Sandbox Path Isolation** | Flatpak's sandboxed paths differ from host paths—broke COSMIC integration |
| **Incremental Permission Discovery** | Flatpak permissions discovered through runtime errors, not documentation |
| **Cross-Distribution Testing** | Apps working on Manjaro failed on Pop!_OS due to SDK version differences |

The emerging model of AI-assisted development:

| Role | AI | Human |
|------|:---:|:-----:|
| Write code | ✓ | |
| Fix compilation errors | ✓ | |
| Propose solutions | ✓ | |
| Test in real environment | | ✓ |
| Recognise incorrect behaviour | | ✓ |
| Test across session boundaries | | ✓ |
| **Test across distributions** | | ✓ |
| Make final decisions | | ✓ |
| Know when to stop | | ✓ |

The human becomes an **editor, tester, and director**—roles that require understanding *what* software should do, even without knowing *how* to implement it. Cross-distribution testing is now an essential human responsibility.

*See [docs/THEMATIC-ANALYSIS.md](docs/THEMATIC-ANALYSIS.md) for the complete analysis (26 themes across 6 sessions).*

### Educational Resources

| Resource | Description |
|----------|-------------|
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Technical journey from concept to release |
| [docs/THEMATIC-ANALYSIS.md](docs/THEMATIC-ANALYSIS.md) | 26 themes identified in AI-human collaboration patterns |
| [docs/RETROSPECTIVE.md](docs/RETROSPECTIVE.md) | What worked, what didn't, and lessons for future projects |
| **Conversation Transcripts** | |
| [Part 1: Creation](docs/transcripts/CONVERSATION-PART1-CREATION.md) | Initial development from shell script to GUI application |
| [Part 2: Refinement](docs/transcripts/CONVERSATION-PART2-REFINEMENT.md) | Bug fixes, system tray, systemd integration, packaging |
| [Part 3: Code Review](docs/transcripts/CONVERSATION-PART3-CODE-REVIEW.md) | Edge case analysis and 13 fixes |
| [Part 4: Architecture & Polish](docs/transcripts/CONVERSATION-PART4-ARCHITECTURE.md) | D-Bus daemon refactoring, theme-aware icons, colored indicators |
| [Part 6: Cross-Distribution Flatpak](docs/transcripts/CONVERSATION-PART6-FLATPAK.md) | Flatpak debugging on Pop!_OS after development on Manjaro |
| [Raw transcripts](docs/transcripts/) | JSONL files for programmatic analysis |

---

## Overview

This project includes both a simple shell script for quick use and a native COSMIC panel applet with full settings window.

## Features

### Panel Applet (v0.4.0+)
- **Native COSMIC Integration**: Lives directly in the COSMIC panel as a native applet
- **Quick Popup**: Click the panel icon for instant access to controls
- **Fetch Wallpaper**: Download and apply today's Bing image with one click
- **Timer Toggle**: Enable/disable daily automatic updates from the popup
- **Status Display**: Shows timer state and next scheduled update

### Settings Window
- **Image Preview**: See today's Bing image before applying
- **History Browser**: Browse and re-apply previously downloaded wallpapers
- **Region Selector**: Choose from 21 Bing markets (US, UK, Germany, Japan, etc.)
- **One-click Apply**: Set any image as your desktop wallpaper instantly
- **Auto-Update Timer**: Enable/disable daily updates directly from settings
- **Status Display**: Shows next scheduled update time

### Shell Script
- Lightweight alternative for automation
- Can be run via cron or systemd timer
- No dependencies beyond curl and Python 3

## Screenshots

![Main Window](screenshots/Screenshot_2026-02-06_20-51-29.png)

*Panel applet popup and settings window showing today's Bing image, region selector, and auto-update timer controls.*

![History Browser](screenshots/Screenshot_2026-02-06_20-52-11.png)

*Browse and re-apply previously downloaded wallpapers.*

## Installation

Build and install the Flatpak package from GitHub:

```bash
# Install flatpak-builder if not already installed
sudo apt install flatpak-builder  # Debian/Ubuntu/Pop!_OS
sudo pacman -S flatpak-builder    # Arch/Manjaro

# Install required Flatpak SDK (if not already installed)
flatpak install flathub org.freedesktop.Platform//25.08 org.freedesktop.Sdk//25.08
flatpak install flathub org.freedesktop.Sdk.Extension.rust-stable//25.08

# Clone the repository
git clone https://github.com/reality2-roycdavies/cosmic-bing-wallpaper.git
cd cosmic-bing-wallpaper

# Build and install the Flatpak (first build takes a while)
flatpak-builder --user --install --force-clean build-dir flathub/io.github.reality2_roycdavies.cosmic-bing-wallpaper.yml

# The applet will appear in COSMIC panel's applet list
# Add it via Panel Settings → Applets
```

### Uninstalling

```bash
flatpak uninstall io.github.reality2_roycdavies.cosmic-bing-wallpaper
```

## Automatic Daily Updates

### From the Panel Applet or Settings

1. Click the applet icon in the panel, or open Settings
2. Toggle "Daily Update" to enable automatic updates
3. The wallpaper will automatically update daily at 8:00 AM

The timer runs within the applet process - no systemd services required. The applet starts automatically with the COSMIC panel.

## Configuration

Configuration is stored at `~/.config/cosmic-bing-wallpaper/config.json`:

| Option | Description | Default |
|--------|-------------|---------|
| `wallpaper_dir` | Directory where images are saved | `~/Pictures/BingWallpapers/` |
| `market` | Regional Bing market code (e.g., "en-US") | `en-US` |
| `auto_update` | Whether the daily update timer is enabled | `false` |
| `keep_days` | Days to keep old wallpapers before cleanup (0 = keep forever) | `30` |
| `fetch_on_startup` | Automatically fetch today's image when app starts | `true` |

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
├── flathub/                           # Flatpak manifest for building
├── src/
│   ├── main.rs                        # Entry point (applet/settings/fetch)
│   ├── applet.rs                      # COSMIC panel applet with popup
│   ├── settings.rs                    # Settings window (full UI)
│   ├── bing.rs                        # Bing API client
│   ├── config.rs                      # Configuration & markets
│   ├── service.rs                     # D-Bus service + wallpaper operations
│   ├── timer.rs                       # Internal timer for daily updates
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

## Development

### Technology Stack
- **Rust** - Systems programming language
- **libcosmic** - COSMIC desktop toolkit (based on iced) with applet support
- **tokio** - Async runtime for non-blocking operations
- **reqwest** - HTTP client for API calls
- **serde** - JSON serialization/deserialization
- **zbus** - D-Bus IPC for applet/settings communication

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
        ▲
        │ D-Bus calls
┌───────┴───────────┐
│  Settings Window  │
│  (D-Bus client)   │
└───────────────────┘
```

#### Components

| Component | File | Purpose |
|-----------|------|---------|
| **Applet** | `applet.rs` | Native COSMIC panel applet with popup. Embeds D-Bus service and timer. |
| **Settings** | `settings.rs` | Full settings window with image preview, history browser, region selector. |
| **Service** | `service.rs` | D-Bus service managing wallpaper operations. |
| **Timer** | `timer.rs` | Internal async timer for daily updates (no systemd required). |
| **D-Bus Client** | `dbus_client.rs` | Proxy for settings window to communicate with applet. |

#### Key Design Points

- The applet runs as a native COSMIC panel applet (auto-starts with the panel)
- D-Bus service and timer run in a background thread within the applet process
- The settings window is a separate process launched via `--settings`
- No systemd, autostart, or lockfile management needed - COSMIC panel handles lifecycle

### Technical Documentation

See [DEVELOPMENT.md](DEVELOPMENT.md) for detailed technical learnings including:
- Panel applet implementation (v0.4.0+)
- D-Bus service architecture
- COSMIC desktop internals
- Flatpak compatibility

## License

MIT License - See [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## Acknowledgments

- [System76](https://system76.com) for the COSMIC desktop environment
- [Microsoft Bing](https://bing.com) for the beautiful daily images
- [Anthropic](https://anthropic.com) and Claude for AI-assisted development

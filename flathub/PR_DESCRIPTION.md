# Add io.github.reality2_roycdavies.cosmic-bing-wallpaper

## App Information

**Name:** Bing Wallpaper
**Summary:** Bing Daily Wallpaper for COSMIC Desktop
**License:** MIT
**Homepage:** https://github.com/reality2-roycdavies/cosmic-bing-wallpaper

## Description

A native COSMIC desktop application that fetches Microsoft Bing's beautiful daily wallpaper images and sets them as your desktop background.

**Features:**
- Preview today's Bing image before applying
- Browse and apply previously downloaded wallpapers
- Choose from 20+ regional Bing markets
- System tray integration with quick controls
- Automatic daily updates via internal timer
- Dark/light theme adaptation
- Lightweight and native - built with libcosmic

This application was built specifically for the COSMIC desktop environment using libcosmic and Rust.

## Technical Notes

- **Runtime:** org.freedesktop.Platform 24.08
- **Build:** Rust (cargo) with offline build via cargo-sources.json
- **Desktop:** COSMIC only (will not function on other desktop environments)
- **Note:** Source code is in `cosmic-bing-wallpaper/` subdirectory

### Permissions Justification

| Permission | Reason |
|------------|--------|
| `--share=network` | Fetch wallpaper images from Bing API |
| `--talk-name=org.kde.StatusNotifierWatcher` | System tray via StatusNotifierItem protocol |
| `--filesystem=~/.config/cosmic:rw` | Read theme config, write background config |
| `--filesystem=~/.config/cosmic-bing-wallpaper:create` | Store app configuration and timer state |
| `--filesystem=~/Pictures/BingWallpapers:create` | Store downloaded wallpaper images |
| `--filesystem=~/.config/autostart:create` | Create autostart entry for tray |

## Checklist

- [x] App builds successfully with flatpak-builder
- [x] AppStream metainfo.xml is valid
- [x] Desktop file is valid
- [x] Icon is included (SVG)
- [x] Screenshots are included
- [x] OARS content rating is set
- [x] License is open source (MIT)

## Screenshots

![Main Window](https://raw.githubusercontent.com/reality2-roycdavies/cosmic-bing-wallpaper/main/screenshots/main.png)

![History Browser](https://raw.githubusercontent.com/reality2-roycdavies/cosmic-bing-wallpaper/main/screenshots/history.png)

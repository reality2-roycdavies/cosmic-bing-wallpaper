#!/bin/bash
#
# Install Script for Cosmic Bing Wallpaper
# =========================================
#
# Compiles the Rust application and installs it as a COSMIC desktop app.
#
# USAGE:
#   ./install.sh           Install for current user (~/.local)
#   sudo ./install.sh      Install system-wide (/usr/local)
#
# WHAT IT DOES:
#   1. Builds the release binary with cargo
#   2. Installs the binary to bin directory
#   3. Installs the desktop file (shows in Applications)
#   4. Installs the icon
#   5. Installs AppStream metadata
#
# TO UNINSTALL:
#   ./install.sh --uninstall
#   sudo ./install.sh --uninstall
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_NAME="cosmic-bing-wallpaper"
APP_ID="io.github.reality2_roycdavies.cosmic-bing-wallpaper"

# Determine install prefix based on whether running as root
if [[ $EUID -eq 0 ]]; then
    PREFIX="/usr/local"
    echo "Installing system-wide to $PREFIX..."
else
    PREFIX="$HOME/.local"
    echo "Installing for current user to $PREFIX..."
fi

BIN_DIR="$PREFIX/bin"
SHARE_DIR="$PREFIX/share"
APPLICATIONS_DIR="$SHARE_DIR/applications"
ICONS_DIR="$SHARE_DIR/icons/hicolor/scalable/apps"
ICONS_SYMBOLIC_DIR="$SHARE_DIR/icons/hicolor/symbolic/apps"
METAINFO_DIR="$SHARE_DIR/metainfo"
DATA_DIR="$SHARE_DIR/$APP_NAME"

# Handle uninstall
if [[ "${1:-}" == "--uninstall" ]] || [[ "${1:-}" == "-u" ]]; then
    echo "Uninstalling $APP_NAME..."

    rm -f "$BIN_DIR/$APP_NAME"
    rm -f "$APPLICATIONS_DIR/$APP_ID.desktop"
    rm -f "$ICONS_DIR/$APP_ID.svg"
    rm -f "$ICONS_SYMBOLIC_DIR/$APP_ID-symbolic.svg"
    rm -f "$METAINFO_DIR/$APP_ID.metainfo.xml"
    rm -f "$HOME/.config/autostart/$APP_ID.desktop"
    rm -rf "$DATA_DIR"

    # Update desktop database
    if command -v update-desktop-database &> /dev/null; then
        update-desktop-database "$APPLICATIONS_DIR" 2>/dev/null || true
    fi

    echo "Uninstall complete."
    exit 0
fi

# Build release binary
echo ""
echo "=== Building release binary ==="
cd "$SCRIPT_DIR"
cargo build --release

# Stop any running instances before upgrading
echo ""
echo "=== Stopping running instances ==="
if pgrep -f "$APP_NAME" > /dev/null 2>&1; then
    echo "Stopping running processes..."
    pkill -f "$APP_NAME" 2>/dev/null || true
    sleep 1
fi

# Create directories
echo ""
echo "=== Installing files ==="
mkdir -p "$BIN_DIR"
mkdir -p "$APPLICATIONS_DIR"
mkdir -p "$ICONS_DIR"
mkdir -p "$ICONS_SYMBOLIC_DIR"
mkdir -p "$METAINFO_DIR"
mkdir -p "$DATA_DIR"

# Install binary (remove old first to avoid permission issues)
echo "Installing binary..."
rm -f "$BIN_DIR/$APP_NAME"
cp "$SCRIPT_DIR/target/release/$APP_NAME" "$BIN_DIR/"
chmod +x "$BIN_DIR/$APP_NAME"

# Install desktop file
echo "Installing desktop entry..."
cat > "$APPLICATIONS_DIR/$APP_ID.desktop" << EOF
[Desktop Entry]
Name=Bing Wallpaper
Comment=Bing Daily Wallpaper for COSMIC Desktop
GenericName=Wallpaper Manager
Exec=$BIN_DIR/$APP_NAME
Icon=$APP_NAME
Terminal=false
Type=Application
Categories=Settings;DesktopSettings;
Keywords=wallpaper;bing;background;desktop;cosmic;
StartupNotify=true
EOF

# Install icons
echo "Installing icons..."
cp "$SCRIPT_DIR/resources/$APP_ID.svg" "$ICONS_DIR/"
cp "$SCRIPT_DIR/resources/$APP_ID-symbolic.svg" "$ICONS_SYMBOLIC_DIR/"
cp "$SCRIPT_DIR/resources/$APP_ID-on-symbolic.svg" "$ICONS_SYMBOLIC_DIR/"
cp "$SCRIPT_DIR/resources/$APP_ID-off-symbolic.svg" "$ICONS_SYMBOLIC_DIR/"

# Install autostart entry for tray
AUTOSTART_DIR="$HOME/.config/autostart"
mkdir -p "$AUTOSTART_DIR"
echo "Installing autostart entry..."
cat > "$AUTOSTART_DIR/$APP_ID.desktop" << EOF
[Desktop Entry]
Name=Bing Wallpaper Tray
Comment=Daily Bing wallpaper for COSMIC desktop
Exec=$BIN_DIR/$APP_NAME --tray
Icon=$APP_ID
Terminal=false
Type=Application
Categories=Utility;
X-GNOME-Autostart-enabled=true
NoDisplay=true
EOF

# Install AppStream metadata
echo "Installing metadata..."
cp "$SCRIPT_DIR/resources/$APP_ID.appdata.xml" "$METAINFO_DIR/"

# Install shell script (for timer)
echo "Installing shell script..."
cp "$SCRIPT_DIR/../bing-wallpaper.sh" "$DATA_DIR/"
chmod +x "$DATA_DIR/bing-wallpaper.sh"

# Update desktop database
if command -v update-desktop-database &> /dev/null; then
    echo "Updating desktop database..."
    update-desktop-database "$APPLICATIONS_DIR" 2>/dev/null || true
fi

# Update icon cache
if command -v gtk-update-icon-cache &> /dev/null; then
    echo "Updating icon cache..."
    gtk-update-icon-cache "$SHARE_DIR/icons/hicolor" 2>/dev/null || true
fi

echo ""
echo "=== Installation complete ==="
echo ""
echo "The application is now available in your Applications menu as 'Bing Wallpaper'."
echo "You can also run it from the terminal: $APP_NAME"
echo ""
echo "The tray app will start automatically on next login."
echo "To start it now, run: $APP_NAME --tray"
echo ""
echo "To uninstall, run: $0 --uninstall"

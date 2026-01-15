#!/bin/bash
#
# Install cosmic-bing-wallpaper AppImage
# ======================================
#
# This script installs the AppImage as a proper application:
# 1. Creates ~/Apps directory if it doesn't exist
# 2. Copies the AppImage there
# 3. Extracts the icon
# 4. Creates a .desktop file for app launcher integration
#
# Usage:
#   ./install-appimage.sh [path-to-appimage]
#
# If no path is provided, it looks for the AppImage in the current directory.
#

set -euo pipefail

# Configuration
APP_NAME="cosmic-bing-wallpaper"
APPIMAGE_NAME="cosmic-bing-wallpaper-x86_64.AppImage"
APPS_DIR="$HOME/Apps"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"
SYMBOLIC_ICON_DIR="$HOME/.local/share/icons/hicolor/symbolic/apps"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() {
    echo -e "${GREEN}==>${NC} $1"
}

warn() {
    echo -e "${YELLOW}Warning:${NC} $1"
}

error() {
    echo -e "${RED}Error:${NC} $1"
    exit 1
}

# Find the AppImage
if [ $# -ge 1 ]; then
    APPIMAGE_PATH="$1"
else
    # Look in current directory
    if [ -f "./$APPIMAGE_NAME" ]; then
        APPIMAGE_PATH="./$APPIMAGE_NAME"
    elif [ -f "./cosmic-bing-wallpaper.AppImage" ]; then
        APPIMAGE_PATH="./cosmic-bing-wallpaper.AppImage"
    else
        error "AppImage not found. Please provide the path as an argument:
    $0 /path/to/$APPIMAGE_NAME"
    fi
fi

# Verify the AppImage exists
if [ ! -f "$APPIMAGE_PATH" ]; then
    error "AppImage not found at: $APPIMAGE_PATH"
fi

APPIMAGE_PATH="$(realpath "$APPIMAGE_PATH")"
info "Found AppImage: $APPIMAGE_PATH"

# Create Apps directory
if [ ! -d "$APPS_DIR" ]; then
    info "Creating $APPS_DIR directory..."
    mkdir -p "$APPS_DIR"
fi

# Copy AppImage to Apps directory
DEST_APPIMAGE="$APPS_DIR/$APPIMAGE_NAME"
if [ "$APPIMAGE_PATH" != "$DEST_APPIMAGE" ]; then
    info "Installing AppImage to $APPS_DIR..."
    cp "$APPIMAGE_PATH" "$DEST_APPIMAGE"
    chmod +x "$DEST_APPIMAGE"
else
    info "AppImage already in $APPS_DIR"
    chmod +x "$DEST_APPIMAGE"
fi

# Extract icons from AppImage
info "Extracting icons..."
mkdir -p "$ICON_DIR"
mkdir -p "$SYMBOLIC_ICON_DIR"

# Create temp directory for extraction
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

# Extract the AppImage to get the icons
cd "$TEMP_DIR"
"$DEST_APPIMAGE" --appimage-extract >/dev/null 2>&1 || true

# Find and copy the main icon
if [ -f "squashfs-root/io.github.cosmic-bing-wallpaper.svg" ]; then
    cp "squashfs-root/io.github.cosmic-bing-wallpaper.svg" "$ICON_DIR/"
    info "App icon installed"
elif [ -f "squashfs-root/usr/share/icons/hicolor/scalable/apps/io.github.cosmic-bing-wallpaper.svg" ]; then
    cp "squashfs-root/usr/share/icons/hicolor/scalable/apps/io.github.cosmic-bing-wallpaper.svg" "$ICON_DIR/"
    info "App icon installed"
else
    warn "Could not extract app icon from AppImage"
fi

# Find and copy the symbolic icon (for system tray)
if [ -f "squashfs-root/usr/share/icons/hicolor/symbolic/apps/io.github.cosmic-bing-wallpaper-symbolic.svg" ]; then
    cp "squashfs-root/usr/share/icons/hicolor/symbolic/apps/io.github.cosmic-bing-wallpaper-symbolic.svg" "$SYMBOLIC_ICON_DIR/"
    info "Symbolic icon installed (for system tray)"
else
    warn "Could not extract symbolic icon from AppImage"
fi

cd - >/dev/null

# Create .desktop file
info "Creating desktop entry..."
mkdir -p "$DESKTOP_DIR"

cat > "$DESKTOP_DIR/cosmic-bing-wallpaper.desktop" << EOF
[Desktop Entry]
Name=Bing Wallpaper
GenericName=Wallpaper Manager
Comment=Bing Daily Wallpaper for COSMIC Desktop
Exec=$DEST_APPIMAGE
Icon=io.github.cosmic-bing-wallpaper
Terminal=false
Type=Application
Categories=Settings;DesktopSettings;
Keywords=wallpaper;bing;background;desktop;cosmic;
StartupNotify=true
EOF

# Update desktop database
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi

# Update icon cache
if command -v gtk-update-icon-cache &> /dev/null; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
fi

echo ""
echo -e "${GREEN}=== Installation complete ===${NC}"
echo ""
echo "Bing Wallpaper has been installed and should now appear in your"
echo "application launcher."
echo ""
echo "Installed to: $DEST_APPIMAGE"
echo "Desktop file: $DESKTOP_DIR/cosmic-bing-wallpaper.desktop"
echo ""
echo "To uninstall, run:"
echo "  rm \"$DEST_APPIMAGE\""
echo "  rm \"$DESKTOP_DIR/cosmic-bing-wallpaper.desktop\""
echo "  rm \"$ICON_DIR/io.github.cosmic-bing-wallpaper.svg\""
echo "  rm \"$SYMBOLIC_ICON_DIR/io.github.cosmic-bing-wallpaper-symbolic.svg\""

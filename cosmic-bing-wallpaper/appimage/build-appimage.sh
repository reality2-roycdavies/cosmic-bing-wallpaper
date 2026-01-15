#!/bin/bash
#
# Build AppImage for cosmic-bing-wallpaper
#
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
REPO_DIR="$(dirname "$PROJECT_DIR")"
BUILD_DIR="$SCRIPT_DIR/build"
APPDIR="$BUILD_DIR/cosmic-bing-wallpaper.AppDir"

echo "=== Building cosmic-bing-wallpaper AppImage ==="

# Build release binary
echo "Building release binary..."
cd "$PROJECT_DIR"
cargo build --release

# Create AppDir structure
echo "Creating AppDir..."
rm -rf "$BUILD_DIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/scalable/apps"
mkdir -p "$APPDIR/usr/share/metainfo"
mkdir -p "$APPDIR/usr/share/cosmic-bing-wallpaper"

# Copy binary
cp "$PROJECT_DIR/target/release/cosmic-bing-wallpaper" "$APPDIR/usr/bin/"

# Copy the shell script (for timer functionality)
cp "$REPO_DIR/bing-wallpaper.sh" "$APPDIR/usr/share/cosmic-bing-wallpaper/"
chmod +x "$APPDIR/usr/share/cosmic-bing-wallpaper/bing-wallpaper.sh"

# Copy icon (using full app ID for proper panel icon association)
cp "$PROJECT_DIR/resources/io.github.cosmic-bing-wallpaper.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/"

# Copy AppStream metadata
cp "$PROJECT_DIR/resources/io.github.cosmic-bing-wallpaper.appdata.xml" "$APPDIR/usr/share/metainfo/"

# Update and copy desktop file
cat > "$APPDIR/io.github.cosmic-bing-wallpaper.desktop" << 'EOF'
[Desktop Entry]
Name=Bing Wallpaper
Comment=Bing Daily Wallpaper for COSMIC
Exec=cosmic-bing-wallpaper
Icon=io.github.cosmic-bing-wallpaper
Terminal=false
Type=Application
Categories=Settings;DesktopSettings;
Keywords=wallpaper;bing;background;desktop;
EOF
cp "$APPDIR/io.github.cosmic-bing-wallpaper.desktop" "$APPDIR/usr/share/applications/"

# Create AppRun script
cat > "$APPDIR/AppRun" << 'EOF'
#!/bin/bash
SELF=$(readlink -f "$0")
HERE=${SELF%/*}
export PATH="${HERE}/usr/bin:${PATH}"
export LD_LIBRARY_PATH="${HERE}/usr/lib:${LD_LIBRARY_PATH}"
export BING_WALLPAPER_SCRIPT="${HERE}/usr/share/cosmic-bing-wallpaper/bing-wallpaper.sh"
exec "${HERE}/usr/bin/cosmic-bing-wallpaper" "$@"
EOF
chmod +x "$APPDIR/AppRun"

# Create symlinks required by AppImage
cd "$APPDIR"
ln -sf io.github.cosmic-bing-wallpaper.desktop cosmic-bing-wallpaper.desktop
ln -sf usr/share/icons/hicolor/scalable/apps/io.github.cosmic-bing-wallpaper.svg io.github.cosmic-bing-wallpaper.svg
ln -sf io.github.cosmic-bing-wallpaper.svg .DirIcon

# Download appimagetool if not present
APPIMAGETOOL="$SCRIPT_DIR/appimagetool-x86_64.AppImage"
if [ ! -f "$APPIMAGETOOL" ]; then
    echo "Downloading appimagetool..."
    wget -q --show-progress -O "$APPIMAGETOOL" \
        "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$APPIMAGETOOL"
fi

# Build AppImage
echo "Building AppImage..."
cd "$BUILD_DIR"
ARCH=x86_64 "$APPIMAGETOOL" "$APPDIR" "cosmic-bing-wallpaper-x86_64.AppImage"

echo ""
echo "=== AppImage built successfully ==="
echo "Output: $BUILD_DIR/cosmic-bing-wallpaper-x86_64.AppImage"
echo ""
echo "To run: ./cosmic-bing-wallpaper-x86_64.AppImage"

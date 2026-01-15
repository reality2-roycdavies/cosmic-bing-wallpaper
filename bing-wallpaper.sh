#!/bin/bash
#
# Bing Daily Wallpaper for COSMIC Desktop
# ========================================
#
# Downloads Microsoft Bing's daily image and sets it as the desktop
# background for the COSMIC desktop environment (Pop!_OS / System76).
#
# USAGE:
#   ./bing-wallpaper.sh
#
# DEPENDENCIES:
#   - curl      (for HTTP requests)
#   - python3   (for JSON parsing - more portable than jq)
#   - cosmic-bg (COSMIC desktop background service)
#
# CONFIGURATION:
#   Edit the variables below to customize behavior:
#   - WALLPAPER_DIR: Where to save downloaded images
#   - BING_API: Bing market (change mkt=en-US to your region)
#
# HOW IT WORKS:
#   1. Fetches daily image metadata from Bing's HPImageArchive API
#   2. Downloads the image to ~/Pictures/BingWallpapers/
#   3. Updates COSMIC background config (RON format)
#   4. Restarts cosmic-bg to apply the new wallpaper
#
# SCHEDULING:
#   Can be run via systemd timer or cron for daily updates.
#   The GUI app (cosmic-bing-wallpaper) can install the timer for you.
#
# Created with Claude (Anthropic's AI assistant) using Claude Code.
#

# Exit on error, undefined variables, and pipe failures
set -euo pipefail

# =============================================================================
# CONFIGURATION
# =============================================================================

# Directory to store downloaded wallpaper images
WALLPAPER_DIR="${HOME}/Pictures/BingWallpapers"

# Path to COSMIC desktop background configuration file (RON format)
COSMIC_BG_CONFIG="${HOME}/.config/cosmic/com.system76.CosmicBackground/v1/all"

# Bing market code - determines which regional image to fetch
# Supported markets: en-US, en-GB, en-AU, en-CA, en-IN, en-NZ,
#                    de-DE, fr-FR, es-ES, it-IT, nl-NL, pl-PL,
#                    ja-JP, zh-CN, ko-KR, pt-BR, ru-RU,
#                    da-DK, fi-FI, nb-NO, sv-SE
MARKET="en-US"

# Bing API endpoint (constructed from market)
BING_API="https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1&mkt=${MARKET}"

# =============================================================================
# MAIN SCRIPT
# =============================================================================

# Create wallpaper directory if it doesn't exist
mkdir -p "$WALLPAPER_DIR"

echo "Fetching Bing daily image info..."

# Fetch JSON response from Bing's image archive API
BING_JSON=$(curl -s "$BING_API")

# Validate API response
if [[ -z "$BING_JSON" ]]; then
    echo "Error: Failed to fetch Bing API response"
    exit 1
fi

# -----------------------------------------------------------------------------
# Parse JSON response using Python
# We use Python instead of jq because:
# - jq is not installed by default on many systems
# - Python 3 is almost always available on modern Linux
# The NBSP trick preserves spaces in the copyright string during read parsing
# -----------------------------------------------------------------------------
read -r IMAGE_PATH COPYRIGHT <<< $(python3 -c "
import json
import sys
data = json.loads('''$BING_JSON''')
img = data.get('images', [{}])[0]
url = img.get('url', '')
copyright = img.get('copyright', '')
print(url, copyright.replace(' ', '\u00A0'))  # Use NBSP to preserve spaces
" 2>/dev/null || echo "")

# Validate that we got an image URL
if [[ -z "$IMAGE_PATH" ]]; then
    echo "Error: Failed to parse image URL from Bing response"
    exit 1
fi

# Build full URL (API returns partial path like /th?id=OHR...)
FULL_URL="https://www.bing.com${IMAGE_PATH}"

# -----------------------------------------------------------------------------
# Download the image
# Filename format: bing-{market}-YYYY-MM-DD.jpg
# Matches the Rust app's naming convention for consistency
# -----------------------------------------------------------------------------
DATE_TODAY=$(date +%Y-%m-%d)
IMAGE_NAME="bing-${MARKET}-${DATE_TODAY}.jpg"
IMAGE_FILE="${WALLPAPER_DIR}/${IMAGE_NAME}"

# Skip download if we already have today's image (idempotent operation)
if [[ -f "$IMAGE_FILE" ]]; then
    echo "Today's image already downloaded: $IMAGE_FILE"
else
    echo "Downloading: $FULL_URL"
    # -s: silent mode, -L: follow redirects
    curl -s -L -o "$IMAGE_FILE" "$FULL_URL"

    # Verify download succeeded
    if [[ ! -f "$IMAGE_FILE" ]]; then
        echo "Error: Failed to download image"
        exit 1
    fi

    echo "Saved to: $IMAGE_FILE"
fi

# =============================================================================
# Update COSMIC Desktop Configuration
# =============================================================================
# COSMIC stores its configuration in RON (Rusty Object Notation) format.
# The background config specifies which image to display and how to scale it.
# =============================================================================

echo "Updating COSMIC background configuration..."

# Ensure the config directory exists
mkdir -p "$(dirname "$COSMIC_BG_CONFIG")"

# Write the new config in RON format
# Key settings:
#   - output: "all"         Apply to all monitors
#   - source: Path(...)     Absolute path to image file
#   - scaling_mode: Zoom    Fill screen, cropping if needed (no black bars)
#   - filter_method: Lanczos High-quality image scaling algorithm
cat > "$COSMIC_BG_CONFIG" << EOF
(
    output: "all",
    source: Path("${IMAGE_FILE}"),
    filter_by_theme: false,
    rotation_frequency: 300,
    filter_method: Lanczos,
    scaling_mode: Zoom,
    sampling_method: Alphanumeric,
)
EOF

echo "Configuration updated."

# =============================================================================
# Restart cosmic-bg to Apply Changes
# =============================================================================
# cosmic-bg doesn't watch for config changes, so we need to restart it.
# The process is typically managed by the COSMIC compositor and will
# auto-restart, but we ensure it's running manually just in case.
# =============================================================================

echo "Applying new wallpaper..."

# Kill the existing cosmic-bg process (if running)
if pkill -x cosmic-bg 2>/dev/null; then
    sleep 0.5  # Wait for process to fully terminate
fi

# Start cosmic-bg if it's not running
# nohup + disown ensures it survives if this script is run from a terminal
if ! pgrep -x cosmic-bg > /dev/null; then
    nohup cosmic-bg > /dev/null 2>&1 &
    disown
fi

# =============================================================================
# Done - Print Summary
# =============================================================================

echo "Done! Bing wallpaper set: $IMAGE_NAME"

# Display image copyright/attribution info
if [[ -n "$COPYRIGHT" ]]; then
    # Convert NBSP back to regular spaces for readable output
    echo "Image: ${COPYRIGHT//$'\u00A0'/ }"
fi

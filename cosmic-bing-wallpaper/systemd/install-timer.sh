#!/bin/bash
#
# Install systemd user timer for Bing wallpaper auto-updates
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SYSTEMD_USER_DIR="${HOME}/.config/systemd/user"

echo "Installing Bing Wallpaper systemd timer..."

# Create systemd user directory
mkdir -p "$SYSTEMD_USER_DIR"

# Copy service and timer files
cp "$SCRIPT_DIR/cosmic-bing-wallpaper.service" "$SYSTEMD_USER_DIR/"
cp "$SCRIPT_DIR/cosmic-bing-wallpaper.timer" "$SYSTEMD_USER_DIR/"

# Reload systemd user daemon
systemctl --user daemon-reload

# Enable and start the timer
systemctl --user enable cosmic-bing-wallpaper.timer
systemctl --user start cosmic-bing-wallpaper.timer

echo "Timer installed and started!"
echo ""
echo "Status:"
systemctl --user status cosmic-bing-wallpaper.timer --no-pager || true
echo ""
echo "Next trigger:"
systemctl --user list-timers cosmic-bing-wallpaper.timer --no-pager || true
echo ""
echo "To disable: systemctl --user disable --now cosmic-bing-wallpaper.timer"

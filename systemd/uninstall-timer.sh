#!/bin/bash
#
# Uninstall systemd user timer for Bing wallpaper
#

set -e

SYSTEMD_USER_DIR="${HOME}/.config/systemd/user"

echo "Uninstalling Bing Wallpaper systemd timer..."

# Stop and disable the timer
systemctl --user disable --now cosmic-bing-wallpaper.timer 2>/dev/null || true

# Remove the files
rm -f "$SYSTEMD_USER_DIR/cosmic-bing-wallpaper.service"
rm -f "$SYSTEMD_USER_DIR/cosmic-bing-wallpaper.timer"

# Reload systemd
systemctl --user daemon-reload

echo "Timer uninstalled!"

#!/bin/sh
# Remove the old "rdesk" install (what RustTouch was called before).
# Your settings are kept: RustTouch copies ~/.config/rdesk to
# ~/.config/rusttouch the first time it starts. Delete ~/.config/rdesk
# yourself whenever you like.
set -e
echo "Removing old rdesk files (asks for sudo)..."
sudo rm -f \
  /usr/local/bin/rdesk \
  /usr/local/bin/rdesk-settings \
  /usr/local/bin/rdesk-session \
  /usr/local/bin/rdesk-shell \
  /usr/share/wayland-sessions/rdesk.desktop \
  /usr/local/share/applications/rdesk-settings.desktop \
  /etc/pam.d/rdesk \
  /etc/systemd/logind.conf.d/rdesk.conf \
  /etc/xdg/xdg-desktop-portal/rdesk-portals.conf \
  /etc/udev/rules.d/70-rdesk-buttons.rules
sudo rm -rf /usr/local/share/rdesk
sudo udevadm control --reload-rules 2>/dev/null || true
# runtime leftovers (previews, logs) from a running/old session
rm -rf "${XDG_RUNTIME_DIR:-/tmp}/rdesk"
echo "Old rdesk removed."

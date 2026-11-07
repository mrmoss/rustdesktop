#!/bin/sh
# Remove RustTouch from the system. Keeps your settings in ~/.config/rusttouch
# and your login items in ~/.config/autostart (GNOME uses those too).
# Leaves apt packages and a source-built gtk4-layer-shell alone.
set -e
echo "Removing RustTouch (asks for sudo)..."
sudo rm -f \
  /usr/local/bin/rusttouch \
  /usr/local/bin/rusttouch-settings \
  /usr/local/bin/rusttouch-session \
  /usr/local/bin/rusttouch-shell \
  /usr/share/wayland-sessions/rusttouch.desktop \
  /usr/local/share/applications/rusttouch-settings.desktop \
  /usr/local/share/applications/dev.rusttouch.Settings.desktop \
  /usr/local/share/icons/hicolor/scalable/apps/dev.rusttouch.Settings.svg \
  /etc/pam.d/rusttouch \
  /etc/systemd/logind.conf.d/rusttouch.conf \
  /etc/xdg/xdg-desktop-portal/rusttouch-portals.conf \
  /usr/local/lib/systemd/user/rusttouch-session.target \
  /etc/udev/rules.d/70-rusttouch-buttons.rules
sudo rm -rf /usr/local/share/rusttouch /usr/local/lib/rusttouch
sudo udevadm control --reload-rules 2>/dev/null || true
echo "RustTouch removed. Settings are still in ~/.config/rusttouch (delete it to start fresh)."

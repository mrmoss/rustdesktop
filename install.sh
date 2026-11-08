#!/bin/sh
# Build and install rusttouch system-wide, adding a "rusttouch (touch)" login session.
# In a downloaded release (.tar.gz) the programs are already built: it just
# installs them (no Rust needed).
set -e
cd "$(dirname "$0")"
if [ -x bin/rusttouch ] && [ -x bin/rusttouch-settings ]; then
  BIN=bin
  # the release brings its own copy of gtk4-layer-shell (it isn't packaged on 24.04)
  if ls lib/libgtk4-layer-shell.so* >/dev/null 2>&1; then
    sudo install -d /usr/local/lib/rusttouch
    sudo cp -P lib/libgtk4-layer-shell.so* /usr/local/lib/rusttouch/
  fi
else
  . ./ensure-layer-shell.sh
  cargo build --release
  BIN=target/release
fi
# replace an old "rdesk" install (RustTouch's old name)
if [ -e /usr/local/bin/rdesk ] || [ -e /usr/share/wayland-sessions/rdesk.desktop ]; then
  ./uninstall-rdesk.sh
fi
sudo install -Dm755 "$BIN/rusttouch"                 /usr/local/bin/rusttouch
sudo install -Dm755 "$BIN/rusttouch-settings"        /usr/local/bin/rusttouch-settings
sudo install -Dm755 session/rusttouch-session         /usr/local/bin/rusttouch-session
sudo install -Dm755 session/rusttouch-shell           /usr/local/bin/rusttouch-shell
sudo install -Dm644 session/sway/config         /usr/local/share/rusttouch/sway/config
sudo install -Dm644 session/phoc.ini              /usr/local/share/rusttouch/phoc.ini
sudo install -Dm644 session/labwc/rc.xml          /usr/local/share/rusttouch/labwc/rc.xml
sudo install -Dm755 session/labwc/autostart       /usr/local/share/rusttouch/labwc/autostart
sudo install -Dm644 session/rusttouch.desktop         /usr/share/wayland-sessions/rusttouch.desktop
sudo install -Dm644 session/dev.rusttouch.Settings.desktop /usr/local/share/applications/dev.rusttouch.Settings.desktop
sudo install -Dm644 session/icons/dev.rusttouch.Settings.svg /usr/local/share/icons/hicolor/scalable/apps/dev.rusttouch.Settings.svg
sudo rm -f /usr/local/share/applications/rusttouch-settings.desktop
sudo gtk-update-icon-cache -f -t /usr/local/share/icons/hicolor 2>/dev/null || true
sudo install -Dm644 session/pam.d/rusttouch           /etc/pam.d/rusttouch
sudo install -Dm644 session/logind-rusttouch.conf     /etc/systemd/logind.conf.d/rusttouch.conf
sudo install -Dm644 session/rusttouch-session.target /usr/local/lib/systemd/user/rusttouch-session.target
systemctl --user daemon-reload 2>/dev/null || true
sudo install -Dm644 session/rusttouch-portals.conf    /etc/xdg/xdg-desktop-portal/rusttouch-portals.conf
sudo install -Dm644 session/70-rusttouch-buttons.rules /etc/udev/rules.d/70-rusttouch-buttons.rules
sudo udevadm control --reload-rules 2>/dev/null || true
sudo udevadm trigger --subsystem-match=input --action=change 2>/dev/null || true
if ! command -v sway >/dev/null && ! command -v phoc >/dev/null && ! command -v labwc >/dev/null; then
  echo "WARNING: no compositor installed - sudo apt install sway"
fi
command -v sway >/dev/null || echo "Note: split screen needs sway (sudo apt install sway)."
echo "Installed. Reboot once (power button + button permissions), then pick 'RustTouch' at login."

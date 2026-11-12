#!/bin/sh
# Build and install RustTV system-wide, adding a "RustTV" login session.
# RustTV is separate from RustTouch: installing one doesn't change the other.
set -e
cd "$(dirname "$0")"
if [ -x bin/rusttv ]; then
  BIN=bin
  # the release brings its own copy of gtk4-layer-shell (it isn't packaged on 24.04)
  if ls lib/libgtk4-layer-shell.so* >/dev/null 2>&1; then
    sudo install -d /usr/local/lib/rusttouch
    sudo cp -P lib/libgtk4-layer-shell.so* /usr/local/lib/rusttouch/
  fi
else
  # With gtk4-layer-shell (RustTouch needs it too) the volume pop-up can
  # show over apps; without it RustTV still works (volume shown on its own
  # screen only).
  if [ -f ./ensure-layer-shell.sh ] && (. ./ensure-layer-shell.sh); then
    . ./ensure-layer-shell.sh
    cargo build --release --bin rusttv
  else
    echo "(building RustTV without gtk4-layer-shell: no volume pop-up over apps)"
    cargo build --release --no-default-features --bin rusttv
  fi
  BIN=target/release
fi
sudo install -Dm755 "$BIN/rusttv"                       /usr/local/bin/rusttv
sudo install -Dm755 session/rusttv/rusttv-session       /usr/local/bin/rusttv-session
sudo install -Dm644 session/rusttv/sway/config          /usr/local/share/rusttv/sway/config
sudo install -Dm644 session/rusttv/rusttv.desktop       /usr/share/wayland-sessions/rusttv.desktop
# logging in by itself (Settings > Start-up), for your own account only
sudo install -Dm755 session/rusttv/rusttv-autologin     /usr/local/libexec/rusttv-autologin
sudo install -Dm644 session/rusttv/dev.rusttv.autologin.policy /usr/share/polkit-1/actions/dev.rusttv.autologin.policy
# updates (Settings > Updates): apt, snaps and Flatpaks
sudo install -Dm755 session/rusttv/rusttv-update        /usr/local/libexec/rusttv-update
sudo install -Dm644 session/rusttv/dev.rusttv.update.policy /usr/share/polkit-1/actions/dev.rusttv.update.policy
missing=""
for p in sway chromium wpctl pkexec swayidle nmcli bluetoothctl wl-mirror; do
  command -v "$p" >/dev/null || missing="$missing $p"
done
if [ -n "$missing" ]; then
  echo "Note: not installed:$missing"
  echo "      sudo apt install sway wireplumber pkexec swayidle chromium network-manager bluez wl-mirror"
fi
echo "Installed. Pick 'RustTV' on the login screen (or turn on Settings > Log in by itself in RustTV)."

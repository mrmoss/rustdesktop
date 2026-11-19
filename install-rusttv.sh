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
for p in sway chromium wpctl pkexec swayidle nmcli bluetoothctl wl-mirror uxplay; do
  command -v "$p" >/dev/null || missing="$missing $p"
done
if [ -n "$missing" ]; then
  echo "Note: not installed:$missing"
  echo "      sudo apt install sway wireplumber pkexec swayidle chromium network-manager bluez wl-mirror bubblewrap uxplay gstreamer1.0-plugins-bad gstreamer1.0-plugins-good gstreamer1.0-libav gstreamer1.0-gl gstreamer1.0-x"
fi
# AirPlay's sandbox (bubblewrap): on Ubuntu, AppArmor can stop programs making
# the private namespaces a sandbox needs ("setting up uid map: Permission
# denied"). A small AppArmor profile lets bubblewrap make them - asked first:
# it lets any program use bubblewrap for that (as Flatpak needs too).
if command -v bwrap >/dev/null && ! bwrap --ro-bind / / --dev /dev --proc /proc true 2>/dev/null \
   && [ "$(sysctl -n kernel.apparmor_restrict_unprivileged_userns 2>/dev/null)" = 1 ]; then
  echo
  echo "AirPlay's sandbox (bubblewrap) is blocked by Ubuntu's AppArmor here."
  echo "An AppArmor profile can let bubblewrap make its sandboxes (any program could"
  echo "then use bubblewrap for that, as with Flatpak)."
  a=n
  if [ -t 0 ]; then
    printf "Add it? [y/N] "
    read -r a || a=n
  fi
  case "$a" in
    y|Y|yes|YES)
      sudo tee /etc/apparmor.d/rusttv-bwrap >/dev/null <<'PROFILE'
# Added by RustTV's install-rusttv.sh (uninstall-rusttv.sh removes it): lets
# bubblewrap make the private namespaces its sandboxes need (AirPlay's).
abi <abi/4.0>,
include <tunables/global>
profile rusttv-bwrap /usr/bin/bwrap flags=(unconfined) {
  userns,
  include if exists <local/rusttv-bwrap>
}
PROFILE
      if sudo apparmor_parser -r /etc/apparmor.d/rusttv-bwrap \
         && bwrap --ro-bind / / --dev /dev --proc /proc true 2>/dev/null; then
        echo "bubblewrap works now. In RustTV: Settings > AirPlay > Restart AirPlay."
      else
        echo "That didn't help - AirPlay's sandbox still can't start (see docs/RUSTTV.md)."
      fi
      ;;
    *)
      echo "Not added. To add it later: run this script again (see docs/RUSTTV.md, AirPlay)."
      ;;
  esac
fi
echo "Installed. Pick 'RustTV' on the login screen (or turn on Settings > Log in by itself in RustTV)."

#!/bin/sh
# Remove RustTV from the system (RustTouch is left alone). Keeps your
# settings in ~/.config/rusttv and the launchers' browser profiles.
# If RustTV logs in by itself, that's turned off first.
set -e
if [ -x /usr/local/libexec/rusttv-autologin ] && command -v pkexec >/dev/null; then
  pkexec /usr/local/libexec/rusttv-autologin disable 2>/dev/null || true
fi
echo "Removing RustTV (asks for sudo)..."
sudo rm -f \
  /usr/local/bin/rusttv \
  /usr/local/bin/rusttv-session \
  /usr/share/wayland-sessions/rusttv.desktop \
  /usr/local/libexec/rusttv-autologin \
  /usr/share/polkit-1/actions/dev.rusttv.autologin.policy \
  /usr/local/libexec/rusttv-update \
  /usr/share/polkit-1/actions/dev.rusttv.update.policy
sudo rm -rf /usr/local/share/rusttv
echo "RustTV removed. Settings are still in ~/.config/rusttv (delete it to start fresh)."

#!/bin/sh
# Build + runtime dependencies for Ubuntu 24.04 (noble) and 26.04 (resolute).
# Every optional package is installed one at a time, so one missing
# package can't block the rest.
set -e
cd "$(dirname "$0")"

. /etc/os-release
echo "== rusttouch deps for $PRETTY_NAME"

# Most of this lives in "universe".
if ! grep -rqsE '^[^#]*universe|Components:.*universe' /etc/apt/sources.list /etc/apt/sources.list.d/; then
  sudo add-apt-repository -y universe
fi
sudo apt update

# A downloaded release (.tar.gz) is already built: only what it needs to run.
PREBUILT=""
[ -x bin/rusttouch ] && PREBUILT=1

if [ -n "$PREBUILT" ]; then
  sudo apt install -y libgtk-4-1
else
  # Required for any build (including run-windowed.sh)
  sudo apt install -y build-essential pkg-config libgtk-4-dev curl

  # Rust: Ubuntu's cargo is fine on 26.04; on 24.04 it's old, so prefer rustup.
  if ! command -v cargo >/dev/null 2>&1; then
    echo "== installing Rust via rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    echo "   (open a new shell or: . \$HOME/.cargo/env)"
  fi
fi

# Optional runtime helpers (shell works without them; features just hide)
for p in sway wl-mirror phoc wvkbd squeekboard swayidle grim wtype wlr-randr iio-sensor-proxy \
         xdg-desktop-portal-gtk xdg-desktop-portal-wlr \
         network-manager bluez wireplumber pulseaudio-utils sound-theme-freedesktop foot labwc xdotool; do
  sudo apt install -y "$p" >/dev/null 2>&1 && echo "  ok      $p" || echo "  MISSING $p (optional)"
done

if [ -n "$PREBUILT" ]; then
  echo
  echo "Done. Now: ./install.sh"
  exit 0
fi

# gtk4-layer-shell (needed for run-nested.sh / install.sh, not run-windowed.sh)
. ./ensure-layer-shell.sh

echo
echo "Done. Try: ./run-windowed.sh   (then ./run-nested.sh)"

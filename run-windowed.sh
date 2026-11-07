#!/bin/sh
# Quickest look: no compositor needed, runs as a phone-sized window
# on GNOME / X11 / anything. Gestures work with the mouse too.
# Built without layer-shell, so libgtk4-layer-shell-dev isn't required.
set -e
cd "$(dirname "$0")"
RUSTTOUCH_WINDOWED=1 cargo run --no-default-features

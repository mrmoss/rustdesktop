#!/bin/sh
# Dev loop: run the compositor as a window inside your current desktop with
# the freshly built rusttouch as its shell. Close the window to stop.
# Uses phoc if installed (RUSTTOUCH_COMPOSITOR=labwc to override).
set -e
cd "$(dirname "$0")"
. ./ensure-layer-shell.sh
cargo build
BIN="$PWD/target/debug/rusttouch"
COMP="${RUSTTOUCH_COMPOSITOR:-}"
[ -z "$COMP" ] && { command -v phoc >/dev/null && COMP=phoc || COMP=labwc; }
# mouse in a nested window: keep the cursor visible
export RUSTTOUCH_SHOW_CURSOR=1
# don't start your login apps a second time inside the test window
export RUSTTOUCH_NO_AUTOSTART=1
case "$COMP" in
phoc)
  PHOC_DEBUG=auto-maximize exec phoc -X -C "$PWD/session/phoc.ini" -E "$BIN" ;;
labwc)
  command -v labwc >/dev/null || { echo "install phoc or labwc"; exit 1; }
  D=$(mktemp -d); cp session/labwc/rc.xml "$D/"; echo "\"$BIN\" &" > "$D/autostart"
  exec labwc -C "$D" ;;
esac

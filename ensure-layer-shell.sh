#!/bin/sh
# Make sure the gtk4-layer-shell C library is installed (needed by the
# default `layer` feature). Sourced by install.sh and run-nested.sh.
#   26.04+: apt install libgtk4-layer-shell-dev
#   24.04 : not packaged -> build from source into /usr/local

# Include /usr/local in case a source build put the .pc file there.
export PKG_CONFIG_PATH="/usr/local/lib/x86_64-linux-gnu/pkgconfig:/usr/local/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

if pkg-config --exists gtk4-layer-shell-0; then
  :
else
  echo "== gtk4-layer-shell not found, installing it"
  if apt-cache show libgtk4-layer-shell-dev >/dev/null 2>&1; then
    sudo apt install -y libgtk4-layer-shell-dev
  else
    echo "   (not packaged for this Ubuntu release - building from source)"
    "$(dirname "$0")/build-layer-shell.sh"
  fi
  if ! pkg-config --exists gtk4-layer-shell-0; then
    echo "ERROR: gtk4-layer-shell still not visible to pkg-config." >&2
    echo "       Run ./build-layer-shell.sh by itself and check its output." >&2
    exit 1
  fi
fi
echo "== using gtk4-layer-shell $(pkg-config --modversion gtk4-layer-shell-0)"

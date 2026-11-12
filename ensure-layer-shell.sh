#!/bin/sh
# Make sure the gtk4-layer-shell C library is installed (needed by the
# default `layer` feature). Sourced by install-rusttouch.sh,
# install-rusttv.sh, deps.sh and the justfile.
#   26.04+: apt install libgtk4-layer-shell-dev
#   24.04 : not packaged -> build from source into /usr/local
# (1.1 or newer is needed: the lock screen uses its session lock)

# Include /usr/local in case a source build put the .pc file there.
# (x86_64-linux-gnu on Intel/AMD, aarch64-linux-gnu on ARM)
MULTIARCH=$(dpkg-architecture -qDEB_HOST_MULTIARCH 2>/dev/null || gcc -dumpmachine 2>/dev/null || echo x86_64-linux-gnu)
export PKG_CONFIG_PATH="/usr/local/lib/$MULTIARCH/pkgconfig:/usr/local/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

# 1.1 or newer: the lock screen uses its session lock (gtk4-session-lock)
NEED=1.1
if pkg-config --atleast-version=$NEED gtk4-layer-shell-0; then
  :
else
  if pkg-config --exists gtk4-layer-shell-0; then
    echo "== gtk4-layer-shell $(pkg-config --modversion gtk4-layer-shell-0) is too old (need $NEED+ for the lock screen)"
  else
    echo "== gtk4-layer-shell not found, installing it"
  fi
  # the packaged one, if this Ubuntu has a new enough one; else build it
  if apt-cache show libgtk4-layer-shell-dev >/dev/null 2>&1; then
    sudo apt install -y libgtk4-layer-shell-dev
  fi
  if ! pkg-config --atleast-version=$NEED gtk4-layer-shell-0; then
    echo "   (no new enough package for this Ubuntu release - building from source)"
    "$(dirname "$0")/build-layer-shell.sh"
  fi
  if ! pkg-config --atleast-version=$NEED gtk4-layer-shell-0; then
    echo "ERROR: gtk4-layer-shell $NEED+ still not visible to pkg-config." >&2
    echo "       Run ./build-layer-shell.sh by itself and check its output." >&2
    exit 1
  fi
fi
echo "== using gtk4-layer-shell $(pkg-config --modversion gtk4-layer-shell-0)"

#!/bin/sh
# Build gtk4-layer-shell from source into /usr/local.
# Only needed on Ubuntu 24.04, where it isn't packaged. On 26.04 just use
#   sudo apt install libgtk4-layer-shell-dev
# Undo with: sudo ninja -C <builddir> uninstall  (or delete the files listed by it)
set -e
# 1.1+ has the session lock the lock screen uses. The commit is pinned too:
# a tag can be moved, a commit hash can't. (To build another version, set
# both GTK4_LAYER_SHELL_VERSION and GTK4_LAYER_SHELL_COMMIT.)
VERSION="${GTK4_LAYER_SHELL_VERSION:-v1.2.0}"
COMMIT="${GTK4_LAYER_SHELL_COMMIT:-948622589fbfc8cb6bf1f436ba06592d5802e255}"

sudo apt install -y git meson ninja-build pkg-config libwayland-dev wayland-protocols libgtk-4-dev

# A fresh private folder (only you can open it, mode 0700): a fixed name in
# /tmp could be made first by another user, who could then swap the files
# that `sudo ninja install` runs as root.
SRC=$(mktemp -d "${TMPDIR:-/tmp}/gtk4-layer-shell.XXXXXXXX")
trap 'rm -rf "$SRC"' EXIT INT TERM
git clone --depth 1 --branch "$VERSION" https://github.com/wmww/gtk4-layer-shell.git "$SRC/src"
cd "$SRC/src"
got=$(git rev-parse HEAD)
if [ "$got" != "$COMMIT" ]; then
  echo "gtk4-layer-shell $VERSION is commit $got, expected $COMMIT - not building it" >&2
  exit 1
fi

meson setup build --prefix=/usr/local --buildtype=release \
  -Dexamples=false -Ddocs=false -Dtests=false \
  -Dintrospection=false -Dvapi=false
ninja -C build
sudo ninja -C build install
# Make sure the dynamic linker knows /usr/local/lib (Ubuntu already does, but be safe)
MULTIARCH=$(dpkg-architecture -qDEB_HOST_MULTIARCH 2>/dev/null || gcc -dumpmachine 2>/dev/null || echo x86_64-linux-gnu)
echo "/usr/local/lib/$MULTIARCH" | sudo tee /etc/ld.so.conf.d/rusttouch-local.conf >/dev/null
sudo ldconfig
export PKG_CONFIG_PATH="/usr/local/lib/$MULTIARCH/pkgconfig:/usr/local/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

# pkg-config on Ubuntu already searches /usr/local/lib/<multiarch>/pkgconfig
if pkg-config --exists gtk4-layer-shell-0; then
  echo "gtk4-layer-shell $(pkg-config --modversion gtk4-layer-shell-0) installed to /usr/local"
else
  echo "Installed, but pkg-config can't see it. Try:"
  echo "  export PKG_CONFIG_PATH=/usr/local/lib/$MULTIARCH/pkgconfig:/usr/local/lib/pkgconfig"
  exit 1
fi

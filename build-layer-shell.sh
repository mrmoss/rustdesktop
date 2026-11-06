#!/bin/sh
# Build gtk4-layer-shell from source into /usr/local.
# Only needed on Ubuntu 24.04, where it isn't packaged. On 26.04 just use
#   sudo apt install libgtk4-layer-shell-dev
# Undo with: sudo ninja -C <builddir> uninstall  (or delete the files listed by it)
set -e
VERSION="${GTK4_LAYER_SHELL_VERSION:-v1.0.4}"

sudo apt install -y git meson ninja-build pkg-config libwayland-dev wayland-protocols libgtk-4-dev

SRC="${TMPDIR:-/tmp}/gtk4-layer-shell"
rm -rf "$SRC"
git clone --depth 1 --branch "$VERSION" https://github.com/wmww/gtk4-layer-shell.git "$SRC"
cd "$SRC"

meson setup build --prefix=/usr/local --buildtype=release \
  -Dexamples=false -Ddocs=false -Dtests=false \
  -Dintrospection=false -Dvapi=false
ninja -C build
sudo ninja -C build install
# Make sure the dynamic linker knows /usr/local/lib (Ubuntu already does, but be safe)
echo /usr/local/lib/x86_64-linux-gnu | sudo tee /etc/ld.so.conf.d/rusttouch-local.conf >/dev/null
sudo ldconfig
export PKG_CONFIG_PATH="/usr/local/lib/x86_64-linux-gnu/pkgconfig:/usr/local/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

# pkg-config on Ubuntu already searches /usr/local/lib/x86_64-linux-gnu/pkgconfig
if pkg-config --exists gtk4-layer-shell-0; then
  echo "gtk4-layer-shell $(pkg-config --modversion gtk4-layer-shell-0) installed to /usr/local"
else
  echo "Installed, but pkg-config can't see it. Try:"
  echo "  export PKG_CONFIG_PATH=/usr/local/lib/x86_64-linux-gnu/pkgconfig:/usr/local/lib/pkgconfig"
  exit 1
fi

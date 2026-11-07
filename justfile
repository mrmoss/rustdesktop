# Build helper for RustTouch. Run `just` to list the recipes.
# Get just:  sudo apt install just  |  cargo install just
#
# Plain scripts do the same jobs if you don't have just: ./deps.sh, ./install.sh,
# ./run-windowed.sh, ./run-nested.sh, ./uninstall.sh.

version := `grep -m1 '^version' Cargo.toml | cut -d'"' -f2`
cpu     := arch()
name    := "rusttouch-" + version + "-linux-" + cpu
# Release builds (package, deb) go in their own folder, built so they find
# the copy of gtk4-layer-shell they bring along (Ubuntu 24.04 has none).
dist_bin := "target/dist/release"
rpath    := "/usr/lib/rusttouch:/usr/local/lib/rusttouch"
# where the project lives (the GitHub workflows set it to this repository)
homepage := env_var_or_default("HOMEPAGE", "https://github.com/mrmoss/rusttouch")

# List the recipes
default:
    @just --list

# Install what building and running RustTouch needs (asks for your password)
deps:
    ./deps.sh

# Build RustTouch and its settings app
build:
    #!/bin/sh
    set -eu
    . ./ensure-layer-shell.sh
    cargo build --release

# Build the phone-sized window version only (no gtk4-layer-shell needed)
build-windowed:
    cargo build --release --no-default-features

# Try it in a phone-sized window on your normal desktop
try:
    ./run-windowed.sh

# Try the real thing in a window: a compositor inside your desktop, RustTouch as its shell
nested:
    ./run-nested.sh

# Run the tests
test:
    #!/bin/sh
    set -eu
    . ./ensure-layer-shell.sh
    cargo test --release

# Build and install system-wide, with a "RustTouch" entry on the login screen
install:
    ./install.sh

# Remove what `just install` installed
uninstall:
    ./uninstall.sh

# (used by package and deb) release build that finds its own gtk4-layer-shell
_dist-build:
    #!/bin/sh
    set -eu
    . ./ensure-layer-shell.sh
    RUSTFLAGS="-C link-arg=-Wl,-rpath,{{rpath}}" cargo build --release --target-dir target/dist

# (used by package and deb) copy the gtk4-layer-shell library the programs use into $1
_layer-shell-lib dest:
    #!/bin/sh
    set -eu
    . ./ensure-layer-shell.sh
    so=$(readelf -d "{{dist_bin}}/rusttouch" | sed -n 's/.*Shared library: \[\(libgtk4-layer-shell[^]]*\)\].*/\1/p')
    [ -n "$so" ] || { echo "rusttouch doesn't use gtk4-layer-shell?"; exit 1; }
    libdir=$(pkg-config --variable=libdir gtk4-layer-shell-0)
    mkdir -p "{{dest}}"
    cp -L "$libdir/$so" "{{dest}}/$so"
    echo "bundled $so"

# Make dist/rusttouch-<version>-linux-<cpu>.tar.gz: the programs, ready to install with ./install.sh
package: _dist-build
    #!/bin/sh
    set -eu
    d="dist/{{name}}"
    rm -rf "$d"
    mkdir -p "$d/bin" "$d/docs"
    cp "{{dist_bin}}/rusttouch" "{{dist_bin}}/rusttouch-settings" "$d/bin/"
    just _layer-shell-lib "$d/lib"
    cp -r session "$d/"
    cp install.sh uninstall.sh uninstall-rdesk.sh deps.sh ensure-layer-shell.sh build-layer-shell.sh "$d/"
    cp README.md CHANGELOG.md LICENSE "$d/"
    cp docs/DETAILS.md docs/rusttouch-icon.svg "$d/docs/"
    cp -r docs/screenshots "$d/docs/"
    tar -C dist -czf "dist/{{name}}.tar.gz" "{{name}}"
    rm -rf "$d"
    echo "made dist/{{name}}.tar.gz"

# Make dist/rusttouch_<version>_<amd64|arm64>.deb (Ubuntu 24.04 / 26.04). Needs dpkg-dev.
deb: _dist-build
    #!/bin/sh
    set -eu
    command -v dpkg-deb >/dev/null || { echo "dpkg-deb is missing (sudo apt install dpkg-dev)"; exit 1; }
    arch=$(dpkg --print-architecture)
    pkg="dist/deb-root"
    out="dist/rusttouch_{{version}}_${arch}.deb"
    rm -rf "$pkg"
    mkdir -p "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/share/rusttouch" "$pkg/usr/share/doc/rusttouch"
    # the programs, and the private copy of gtk4-layer-shell they find by themselves
    install -m 755 "{{dist_bin}}/rusttouch" "{{dist_bin}}/rusttouch-settings" "$pkg/usr/bin/"
    just _layer-shell-lib "$pkg/usr/lib/rusttouch"
    # The session files say /usr/local (where ./install.sh puts things); the package uses /usr.
    put() { # put <mode> <source> <destination>
      mkdir -p "$(dirname "$pkg/$3")"
      sed 's#/usr/local/#/usr/#g' "$2" > "$pkg/$3"
      chmod "$1" "$pkg/$3"
    }
    put 755 session/rusttouch-session        usr/bin/rusttouch-session
    put 755 session/rusttouch-shell          usr/bin/rusttouch-shell
    put 644 session/sway/config              usr/share/rusttouch/sway/config
    put 644 session/phoc.ini                 usr/share/rusttouch/phoc.ini
    put 644 session/labwc/rc.xml             usr/share/rusttouch/labwc/rc.xml
    put 755 session/labwc/autostart          usr/share/rusttouch/labwc/autostart
    put 644 session/rusttouch.desktop        usr/share/wayland-sessions/rusttouch.desktop
    put 644 session/dev.rusttouch.Settings.desktop usr/share/applications/dev.rusttouch.Settings.desktop
    put 644 session/icons/dev.rusttouch.Settings.svg usr/share/icons/hicolor/scalable/apps/dev.rusttouch.Settings.svg
    put 644 session/rusttouch-session.target usr/lib/systemd/user/rusttouch-session.target
    put 644 session/70-rusttouch-buttons.rules usr/lib/udev/rules.d/70-rusttouch-buttons.rules
    put 644 session/pam.d/rusttouch          etc/pam.d/rusttouch
    put 644 session/logind-rusttouch.conf    etc/systemd/logind.conf.d/rusttouch.conf
    put 644 session/rusttouch-portals.conf   etc/xdg/xdg-desktop-portal/rusttouch-portals.conf
    install -m 644 README.md CHANGELOG.md docs/DETAILS.md "$pkg/usr/share/doc/rusttouch/"
    install -m 644 LICENSE "$pkg/usr/share/doc/rusttouch/copyright"
    # files under /etc are settings: an upgrade keeps your changes to them
    (cd "$pkg" && find etc -type f | sed 's#^#/#') > "$pkg/DEBIAN/conffiles"
    cat > "$pkg/DEBIAN/postinst" <<'POSTINST'
    #!/bin/sh
    set -e
    if [ "$1" = configure ]; then
      udevadm control --reload-rules 2>/dev/null || true
      udevadm trigger --subsystem-match=input --action=change 2>/dev/null || true
      gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
      echo "RustTouch installed. Restart once, then pick RustTouch on the login screen."
    fi
    POSTINST
    cat > "$pkg/DEBIAN/postrm" <<'POSTRM'
    #!/bin/sh
    set -e
    udevadm control --reload-rules 2>/dev/null || true
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
    POSTRM
    chmod 755 "$pkg/DEBIAN/postinst" "$pkg/DEBIAN/postrm"
    # The libraries the programs need: let dpkg work them out (it needs a stub source package to
    # run). If that is not possible, fall back to the ones RustTouch is known to need.
    deps="libc6, libgtk-4-1 (>= 4.10), libwayland-client0"
    stub=$(mktemp -d)
    mkdir "$stub/debian"
    printf 'Source: rusttouch\n\nPackage: rusttouch\nArchitecture: any\n' > "$stub/debian/control"
    here=$PWD
    if found=$(cd "$stub" && dpkg-shlibdeps -O --ignore-missing-info -l"$here/$pkg/usr/lib/rusttouch" \
                 -e"$here/$pkg/usr/bin/rusttouch" -e"$here/$pkg/usr/bin/rusttouch-settings" \
                 "$here/$pkg"/usr/lib/rusttouch/libgtk4-layer-shell.so* 2>/dev/null \
                 | sed -n 's/^shlibs:Depends=//p') && [ -n "$found" ]; then
      deps="$found"
    fi
    rm -rf "$stub"
    size=$(du -sk "$pkg" | cut -f1)
    cat > "$pkg/DEBIAN/control" <<CONTROL
    Package: rusttouch
    Version: {{version}}
    Section: x11
    Priority: optional
    Architecture: $arch
    Maintainer: mrmoss <mrmoss@users.noreply.github.com>
    Installed-Size: $size
    Depends: $deps, sway | phoc | labwc
    Recommends: sway, grim, swayidle, wlr-randr, wl-mirror, iio-sensor-proxy, network-manager, bluez, wireplumber, pulseaudio-utils, sound-theme-freedesktop, wtype, xdg-desktop-portal-gtk, xdg-desktop-portal-wlr
    Suggests: foot, xdotool
    Homepage: {{homepage}}
    Description: Simple, Android-style touch desktop for Linux tablets
     RustTouch is a small touch desktop: swipe up for your apps, swipe down for
     Wi-Fi, Bluetooth and the clock, the back, home and recent apps buttons at
     the bottom, its own on-screen keyboard, and apps side by side (on sway).
     .
     After installing, restart and pick "RustTouch" on the login screen.
    CONTROL
    dpkg-deb --root-owner-group --build "$pkg" "$out" >/dev/null
    rm -rf "$pkg"
    echo "made $out"

# Format the code
fmt:
    cargo fmt

# Remove build output and dist/
clean:
    cargo clean
    rm -rf dist

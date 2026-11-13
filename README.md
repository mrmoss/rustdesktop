<p align="center"><img src="docs/rusttouch-icon.svg" width="128" alt="RustDesktop logo"></p>

# !!!VIBE CODED CLANKER CODE!!!

# RustDesktop

**Two small, simple Linux desktops in Rust and GTK 4, on the sway window manager.** Each one is
its own choice on the login screen; installing one doesn't change the other, or your usual desktop.

| | For | What it is | Guide |
|---|---|---|---|
| **RustTouch** | tablets, touch-screen laptops | An Android-style touch desktop: swipe up for your apps, swipe down for Wi-Fi, Bluetooth and the clock, ◀ ● ■ at the bottom, its own on-screen keyboard, apps side by side. | [docs/RUSTTOUCH.md](docs/RUSTTOUCH.md) |
| **RustTV** | a TV box, a mini PC on the TV | A home screen made for a remote control: launchers for Plex, Jellyfin, Netflix, YouTube, Hulu... opened full screen, your apps, Wi-Fi, Bluetooth, sound and screens set up with the remote, AirPlay screen mirroring from an iPhone, iPad or Mac, an on-screen keyboard for sign-ins, media buttons. | [docs/RUSTTV.md](docs/RUSTTV.md) |

> **Please read this first.** This project was written almost entirely by an AI assistant and has
> had very little human review or real-world testing. It is a hobby project. Keep your usual
> desktop installed as well, and log out to pick it again whenever you like.

| RustTouch | RustTV |
|---|---|
| ![RustTouch home screen](docs/screenshots/rusttouch/home.png) | ![RustTV home screen](docs/screenshots/rusttv/home.png) |

More pictures in each guide: [RustTouch](docs/RUSTTOUCH.md#pictures) · [RustTV](docs/RUSTTV.md#pictures).

## Install

**Where it runs:** Ubuntu 26.04 and 24.04, on 64-bit Intel/AMD (`x86_64`) and ARM (`aarch64`).
Other Linux with GTK 4.10 or newer and sway may work, but isn't tested.

**The easy way:** download `rustdesktop_<version>_amd64.deb` (or `arm64` for ARM) from the
[Releases page](../../releases). It has both desktops:

```sh
sha256sum --check --ignore-missing SHA256SUMS   # with SHA256SUMS from the same release: must say "OK"
sudo apt install ./rustdesktop_0.9.16_amd64.deb
```

It replaces the older `rusttouch` package if you have it, and installs sway and the helpers both
desktops use. Remove it with `sudo apt remove rustdesktop` (your settings stay). The same page
has `rustdesktop-<version>-linux-<cpu>.tar.gz`, already built, for installing without the
package manager (see [installing RustTouch](docs/RUSTTOUCH.md#install)).

**From source**, open a terminal in this folder:

```sh
./deps.sh               # what building and running both need (asks for your password)
./install-rusttouch.sh  # RustTouch: builds it and adds "RustTouch" to the login screen
./install-rusttv.sh     # RustTV: builds it and adds "RustTV" to the login screen
```

Then pick one on the login screen (click your name, then the gear or session button near the
password box). `./uninstall-rusttouch.sh` and `./uninstall-rusttv.sh` remove them again; your
settings stay in `~/.config/rusttouch` and `~/.config/rusttv`.

## What's in this folder

| Path | What it is |
|---|---|
| `src/main.rs`, `src/*.rs` | RustTouch (the `rusttouch` program) |
| `src/bin/rusttouch-settings.rs` | RustTouch's settings app |
| `src/bin/rusttv/` | RustTV (the `rusttv` program); it shares RustTouch's Wi-Fi, Bluetooth and sound code |
| `session/` | RustTouch's login session: session scripts, sway config, PAM, udev and logind files |
| `session/rusttv/` | RustTV's login session, sway config, and its autologin and update helpers |
| `deps.sh` | Installs what building and running both desktops needs (Ubuntu) |
| `install-rusttouch.sh`, `uninstall-rusttouch.sh` | Install / remove RustTouch |
| `install-rusttv.sh`, `uninstall-rusttv.sh` | Install / remove RustTV |
| `ensure-layer-shell.sh`, `build-layer-shell.sh` | Make sure gtk4-layer-shell is there (it isn't packaged on Ubuntu 24.04, so it's built from source); used by the scripts above and the release builds |
| `justfile` | Short names for the common jobs (below) |
| `docs/` | The guides, and the pictures in `docs/screenshots/` |

The Cargo package, the `.deb` and the downloads are called `rustdesktop`; the programs keep
their own names (`rusttouch`, `rusttouch-settings`, `rusttv`).

## Building and releasing

A [`justfile`](justfile) wraps the common jobs, so you don't have to remember the steps (get
`just` with `sudo apt install just` or `cargo install just`). Run `just` to see them all. The
plain scripts do the same jobs if you'd rather not install it.

| Command | Script | What it does |
|---|---|---|
| `just deps` | `./deps.sh` | Install what building and running both desktops needs. |
| `just build` | | Build RustTouch, its settings app and RustTV in release mode. |
| `just test` | | Run the tests. |
| `just install` | `./install-rusttouch.sh` | Build and install RustTouch into `/usr/local`, with a *RustTouch* entry on the login screen. |
| `just uninstall` | `./uninstall-rusttouch.sh` | Remove RustTouch again (your settings stay). |
| `just install-tv` | `./install-rusttv.sh` | Build and install RustTV, with a *RustTV* entry on the login screen. |
| `just uninstall-tv` | `./uninstall-rusttv.sh` | Remove RustTV again (your settings stay). |
| `just package` | | Make `dist/rustdesktop-<version>-linux-<cpu>.tar.gz` (both desktops, built), ready to install with `./install-rusttouch.sh` / `./install-rusttv.sh`. |
| `just deb` | | Make `dist/rustdesktop_<version>_<amd64\|arm64>.deb` (both desktops). |
| `just clean` | | Remove build output and `dist/`. |

**Releases are made by GitHub Actions.** Pushing a tag builds the `.deb` and `.tar.gz` (both desktops) for
Intel/AMD and ARM on Ubuntu 24.04 (they work on 26.04 too) and puts them in a **draft** release:

```sh
# 1. bump `version` in Cargo.toml (and CHANGELOG.md) and commit it
git tag v0.9.16
git push --tags
# 2. wait for the "release" workflow, look over the draft under Releases, then publish it
```

The tag must match the version in `Cargo.toml` or the build stops. The downloads come with a
`SHA256SUMS` file and a build attestation (a statement signed by GitHub that the files were
built by this workflow from this repository; check one with
`gh attestation verify FILE --repo OWNER/REPO`). A separate workflow (`build`) builds and tests
every push on Ubuntu 24.04 (Intel/AMD and ARM) and 26.04, and checks that the `.deb` installs.
Another (`audit`) checks the Rust libraries for known security problems every week. For public
repositories, GitHub Actions is free.

## More

- [docs/RUSTTOUCH.md](docs/RUSTTOUCH.md): RustTouch, how to install and use it, its settings, security and troubleshooting.
- [docs/DETAILS.md](docs/DETAILS.md): RustTouch's every feature and config key, and how it works inside.
- [docs/RUSTTV.md](docs/RUSTTV.md): RustTV, the remote, the home screen, launchers and settings.
- [CHANGELOG.md](CHANGELOG.md): what changed in each version.

## License

RustDesktop (RustTouch and RustTV) is free software, released under the
[GNU General Public License, version 2](LICENSE) (`GPL-2.0-only`). You may use, study, change
and share it under those terms; there is no warranty.

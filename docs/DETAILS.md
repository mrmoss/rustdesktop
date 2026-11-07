# RustTouch - technical details

The full reference: every feature, config key and how it works inside. New here? Start with the [README](../README.md).

A deliberately small, Android-style touch shell for Linux, in Rust + GTK4.
Think "Phosh, but a weekend project you can read in one sitting".


## What it does

| Thing | How |
|---|---|
| **App drawer** | Swipe up on home or the nav bar; it tracks your finger and settles on release. Drag down from the top of the list to close. |
| **Quick settings** | Pull down from the top bar / home. Wi-Fi, Bluetooth, Rotate, Settings; ⌨ button shows the keyboard; brightness + volume; Lock / Sleep / Restart / Power off / Log out. **Long-press Wi-Fi or Bluetooth** for a list: tap to connect (password prompt for new networks; new Bluetooth devices are paired), tap again to disconnect, × to forget, ✎ to give a Bluetooth device a nickname. |
| **Home screen** | An Android-style icon grid (no clock, no dock). Long-press an icon and drag: onto a free spot to move it, onto another icon to swap, onto *Remove* at the top to take it off. Long-press an app in the drawer: the drawer fades and you drop it into a spot. Portrait and landscape each keep their own arrangement, so rotating never shuffles icons. |
| **◀ Back** | Closes rusttouch panels → hides the keyboard → otherwise sends **Alt+Left** to the app. Long-press = **Escape**. Per-app overrides in `back.conf`. |
| **● Home** | Fades the home screen in over your apps. |
| **■ Recents** | Cards with screenshots. On sway each card is a window group (apps sharing the screen); ✕ closes one window, flick up closes the group, hold-and-drag a window to rearrange. Elsewhere one card per app. |
| **Lock screen** | Clock → swipe up → password with a built-in full keyboard. PAM-checked. |
| **Keyboard button** | ⌨ at the bottom right of the button bar opens the on-screen keyboard by hand; ⌨⌄ closes it. Settings → *Keyboard*: height (portrait/landscape), key label size, key shape, colors (match the theme). |
| **On-screen keyboard** | Pops up when you tap a text field in any app (wvkbd driven by rusttouch via input-method-v2; squeekboard optional). Put it away with the ⌨⌄ button above it, Back (shows ⌄ while the keyboard is up), or by touching the home screen/top bar. It also goes away when the app un-focuses the field (browsers do when you tap the page; many GTK apps keep the field focused). |
| **Hardware buttons** | Power: press / hold / double-press actions. Volume & brightness: **hold to keep changing**, with a popup. |
| **Idle** | Screen off after N, lock on screen off, sleep after N (swayidle). |
| **Settings app** | `rusttouch-settings` (Settings tile, or in the app drawer): theme, accent, background (presets or your own picture), power & screen, power button, keyboard, Wi-Fi/Bluetooth. |
| **Sound** | › next to the volume slider: pick speakers/headphones and mic; switch a Bluetooth headset between music quality and mic (call) mode. Connecting a headset from rusttouch makes it the default output (and mic, when it has one active). The Bluetooth tile shows what you're connected to. |
| **Tray** | Apps with tray icons (StatusNotifierItem, e.g. `rustsync-gui --tray`) show in the top bar. Tap = open, long-press / right-click = the app's menu. |
| **Clock & bars** | Settings → *Clock & bars*: 12/24-hour time, what the top bar clock shows, date style (default *Thursday November 5, 2026*), the heights of the top bar and the ◀ ● ■ bar, and **auto-hide** for each bar (tap/swipe the screen edge to bring it back). |
| **Startup apps** | Settings → *Startup apps*: RustTouch starts *everything GNOME starts* (`~/.config/autostart`), or *only the ones you pick* - tick a subset for RustTouch without changing GNOME. Add installed apps or any command (e.g. `rustsync start`). |
| **Multiple monitors** | Extra monitors mirror the main screen (wl-mirror). Settings → *Displays*: which screen is main, resolution, scale, rotation. |
| **One config file** | `~/.config/rusttouch/rusttouch.conf` - everything above, commented. Edit it by hand or in the app; rusttouch applies changes live. Wi-Fi/Bluetooth/auto-rotate are remembered across logins. |

### The config file

```ini
theme = dark                 # dark | light | amoled | nord | dracula | gruvbox | solarized-dark | solarized-light | ocean | sepia | contrast
accent = blue                # blue | green | purple | orange | pink | red
background = aurora          # 17 presets (aurora, midnight, ember, glacier, neon, ...) or /path/to/picture.jpg
background_fit = fill        # fill | fit | stretch | center | tile | zoom150 | zoom200
lock_background = same       # same | a preset | /path/to/picture.jpg
lock_message = If found, call 555-0123
wifi = on
bluetooth = on
auto_rotate = on
keyboard_auto = on
keyboard_program = auto      # auto | wvkbd | squeekboard (next login)
screen_off_after = 120       # seconds, 0 = never
lock_on_screen_off = on
sleep_after = 600            # seconds, 0 = never
power_button = sleep         # sleep | lock | screen_off | menu | poweroff | nothing
power_button_hold = menu
power_button_double = nothing
show_cursor = auto          # auto (while a mouse is used) | on | off
compositor = auto           # auto | sway | phoc | labwc (next login)
time_24h = off
bar_clock = time             # time | day_time | date_time
date_format = full           # full | long | short | numeric | iso
top_bar_size = 32            # px
nav_bar_size = 56            # px
home_apps = firefox_firefox.desktop@0:0,org.gnome.Nautilus.desktop@2:0   # id@column:row
home_apps_landscape = ...    # same, for landscape
```

### Hardware buttons

rusttouch reads button devices (power button, tablet volume buttons, brightness
keys) directly, which is what makes long/double press and hold-to-repeat
possible. `install.sh` adds a udev rule giving the logged-in user read access
to devices that have such keys **but are not keyboards** - rusttouch never reads
your keyboard. Reboot once after installing. While rusttouch runs it tells logind
to leave the power button alone; if rusttouch isn't running, the button sleeps.
Volume/brightness keys on a laptop's main keyboard (Fn keys) are not handled.

## Quick start (Ubuntu 24.04 or 26.04, x86_64)

```sh
./deps.sh            # apt packages; enables "universe"; builds gtk4-layer-shell on 24.04
./run-windowed.sh    # 1) phone-sized window inside your normal desktop - instant look
./run-nested.sh      # 2) real thing: phoc running as a window, rusttouch as its shell
./install.sh         # 3) adds "RustTouch" to the login screen (removes an old rdesk install)
./uninstall.sh       # removes RustTouch again (keeps your settings)
```

Option 1 works anywhere and doesn't need gtk4-layer-shell, but it's only
a preview: no window list, rotation, keyboard or lock-on-suspend.

## Split screen (sway)

Side-by-side / top-and-bottom apps need **sway** (`sudo apt install sway`):
phoc and labwc don't let a shell arrange windows. With `compositor = auto`
RustTouch logs in to sway when it's installed (Settings → Windows to change).

- Every app opens full screen on its own, like a tablet (or next to what's
  showing: Settings → Windows → New apps).
- Recents (■) shows **window groups**: apps sharing the screen are one card,
  drawn in their real arrangement.
  - Hold a window and drag it onto an edge of another window (same group or
    another) to put it on that side, or onto empty space for a group of its own.
  - ✕ on a window closes just that window; flick a card up closes the group.
- Drag the black line between apps to resize; tap it to arrange on the
  screen itself (Flip / Full / Close).

Your own sway lines go in `~/.config/rusttouch/sway.conf`.

## More than one monitor

Extra monitors mirror the main screen (needs `wl-mirror`). That's all - for
separate screens and placement, use GNOME.

## Mouse

The pull-down and the app drawer also open with a mouse: drag them like a
finger, click the top bar, or use the scroll wheel (scroll down on the top bar
or home screen = pull down, scroll up on home or the button bar = app drawer).
The pointer shows while a mouse is used and hides on touch (`show_cursor`).

## Why phoc

The session now runs on **phoc** (Phosh's compositor) instead of labwc,
because phoc is built for exactly this: it rotates touch input with the
screen, supports the input-method protocol that makes squeekboard pop up
on text fields, maximizes every window, and is a big part of why Phosh
feels snappy. labwc still works as a fallback (`RUSTTOUCH_COMPOSITOR=labwc`
in `~/.config/rusttouch/env`); there the keyboard is a manual toggle and touch
is mapped to the built-in panel via `rc.xml`.

What makes rusttouch itself feel quick:
- sheets move by a GPU transform (no relayout) and follow your finger 1:1,
  then finish with a short ease-out (110-280ms depending on distance)
- the app grid is built once and refreshed only when apps are installed/removed
- all system calls (nmcli, wpctl, grim, ...) run off the UI thread
- button press feedback has no fade

## How it's built

On a wlroots compositor rusttouch creates `wlr-layer-shell` surfaces:

- **top bar / nav bar** - Top layer, reserve space so apps sit between them
- **home** - Top layer over the app area, shown/hidden (phosh does the same)
- **drawer / shade / recents** - Overlay layer, mapped while visible
- **lock** - Overlay layer covering everything, exclusive keyboard

```
src/
  main.rs       app setup + CSS loading
  shell.rs      shared state: sheets, home, lock, window list, events from threads
  sheet.rs      sliding panel that can be dragged (drawer, shade, recents)
  slide.rs      tiny custom widget: draws its child shifted by an offset
  gestures.rs   finger-tracking drag with fling detection
  anim.rs       frame-clock ease-out animation helper
  toplevels.rs  window list via wlr-foreign-toplevel (own Wayland connection)
  sensors.rs    orientation (monitor-sensor) + suspend (logind) watchers
  lock.rs       lock screen + built-in keyboard
  pam.rs        ~100 lines of PAM FFI
  bars.rs home.rs drawer.rs quick.rs recents.rs   the UI pieces
  apps.rs       .desktop discovery + launching
  sys.rs        ALL other system interaction (shells out to CLI tools)
  style.css     the whole look
```

## System integration

| Feature | Tool |
|---|---|
| Wi-Fi / airplane | `nmcli` |
| Bluetooth | `bluetoothctl` |
| Battery | `/sys/class/power_supply` |
| Brightness | `/sys/class/backlight` + logind `SetBrightness` (no root / video group needed) |
| Volume | `wpctl` (PipeWire) |
| Back key | `wtype` (X11: `xdotool`) |
| Window list, switch, close | wlr-foreign-toplevel-management (built in) |
| Recents screenshots | `grim` |
| Rotation | `monitor-sensor` (iio-sensor-proxy) + `wlr-randr` |
| On-screen keyboard | `wvkbd-mobintl` shown/hidden by rusttouch from input-method-v2, or `squeekboard` |
| Screen off / sleep timers | `swayidle` + wlr-output-power-management |
| Buttons | `/dev/input` button devices (udev rule) |
| Settings | `~/.config/rusttouch/rusttouch.conf` (watched with GIO) |
| Lock before suspend | `gdbus monitor` + `systemd-inhibit` delay lock |
| Password | PAM service `rusttouch` (`/etc/pam.d/rusttouch` → common-auth) |

## Customizing

- `~/.config/rusttouch/rusttouch.conf` - the settings above
- `~/.config/rusttouch/favorites` - one desktop id per line (e.g. `firefox_firefox.desktop`)
- `~/.config/rusttouch/back.conf` - what Back sends, per app id:
  ```
  * = alt+Left
  org.gnome.TextEditor = Escape
  org.gnome.Ptyxis = none
  ```
- `~/.config/rusttouch/style.css` - loaded on top of the built-in theme
- `~/.config/rusttouch/env` - sourced by the session, e.g. `RUSTTOUCH_COMPOSITOR=labwc`,
  `RUSTTOUCH_ROTATE_INVERT=1` (if rotation goes the wrong way), `RUSTTOUCH_SHOW_CURSOR=1`

Logs: `$XDG_RUNTIME_DIR/rusttouch/rusttouch.log` (i.e. `/run/user/$(id -u)/rusttouch/rusttouch.log`).
At startup it says which keyboard it uses and which button devices it watches.

### Slow login / slow first app launch

GTK apps ask the desktop *portal* for settings at startup. If nothing answers
for the session, each one waits out a 25s D-Bus timeout. `rusttouch-shell` now
exports the session environment to D-Bus/systemd, and `install.sh` installs
`/etc/xdg/xdg-desktop-portal/rusttouch-portals.conf` so the GTK portal answers
(`xdg-desktop-portal-gtk`, installed by `deps.sh`). Snap apps (Firefox) are
still slower on their very first launch after boot - that's snap.

## 24.04 vs 26.04

| | 24.04 (noble) | 26.04 (resolute) |
|---|---|---|
| gtk4-layer-shell | not packaged → `build-layer-shell.sh` (automatic) | `libgtk4-layer-shell-dev` |
| phoc / squeekboard | in universe | in universe |
| Rust | use rustup | apt cargo is fine |

`libgtk-layer-shell-dev` (no **4**) is the GTK3 library - not what rusttouch needs.

## Known rough edges / next steps

1. **Lock screen security:** it's a layer surface, so if rusttouch crashes the
   screen is unlocked. The proper fix is `ext-session-lock-v1`
   (gtk4-layer-shell ≥ 1.1 has it; needs newer phoc/labwc too).
2. **Back = Alt+Left** is a heuristic. Apps without "go back" ignore it;
   use `back.conf` for those.
3. **Recents screenshots** are taken when you press Home/Recents while the
   app is in front. Apps you never left show their icon until then.
4. **App icons in recents** rely on app_id matching the .desktop name
   (wrong for some snaps / Electron apps).
5. Notifications (`org.freedesktop.Notifications`), idle screen-off/lock,
   and a swipe-from-the-very-bottom gesture over fullscreen apps.

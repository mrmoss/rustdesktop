# RustTouch

A simple, Android-style touch desktop for Linux tablets and touch-screen
laptops. Swipe up for your apps, swipe down for Wi-Fi, Bluetooth and the
clock, and use the familiar ◀ ● ■ buttons at the bottom. Put two or more apps
side by side when you need to.

Written in Rust with GTK 4. Runs on Ubuntu 24.04 and 26.04 (x86_64).

> The pictures below are illustrations of the layout, not captures from a real
> device. To replace them with real screenshots, see
> [Taking screenshots](#taking-screenshots).

| Home screen | Pull-down menu |
|---|---|
| ![Home screen](docs/screenshots/home.png) | ![Pull-down menu](docs/screenshots/quick.png) |
| **App drawer** | **Recent apps and window groups** |
| ![App drawer](docs/screenshots/drawer.png) | ![Recent apps](docs/screenshots/recents.png) |
| **Split screen** | **Lock screen** |
| ![Split screen](docs/screenshots/split.png) | ![Lock screen](docs/screenshots/lock.png) |

## Install

Open a terminal in this folder and run:

```sh
./deps.sh      # installs what RustTouch needs (asks for your password)
./install.sh   # builds RustTouch and adds it to the login screen
```

Then restart, and on the login screen pick **RustTouch** (click your name,
then the gear or session button near the password box).

## How to use it

| To do this | Do this |
|---|---|
| Open an app | Swipe up from the bottom, or tap an icon on the home screen |
| Search for an app | Swipe up, then type in the search box |
| Wi-Fi, Bluetooth, brightness, volume | Swipe down from the top |
| Connect to a Wi-Fi network or Bluetooth device | Swipe down, then press and hold the Wi-Fi or Bluetooth button |
| Go back | ◀ |
| Go to the home screen | ● |
| See your open apps | ■ |
| Jump back to the app you used before | ■ twice, or swipe right along the bottom bar |
| Close an app | ■, then flick its card up |
| Show the keyboard | It opens when you tap a text box, or tap ⌨ in the bottom right |
| Add an app to the home screen | Swipe up, press and hold the app, drag it onto the home screen |
| Move or remove a home screen icon | Press and hold it, then drag it (onto **Remove** at the top to take it off) |
| Lock, sleep, restart, power off | Swipe down; the buttons are at the bottom |
| Change settings | Swipe down and tap **Settings** |

A mouse works too: click the top bar or scroll on the home screen to open the
menus.

Swipe sideways along the bottom bar to slide to the next or previous app or
window group.

## Apps side by side

1. Open two or more apps.
2. Tap ■ to see your open apps.
3. Press and hold one app's card and drag it onto the edge of another
   app's card. A highlight shows where it will go. Let go.
4. The apps now share the screen. Drag the small handle on the line between
   them to change their sizes.

Apps that share the screen are shown as one card, a "window group".

- **✕** on a window in a card closes just that window.
- **Flick a card up** to close every window in that group.
- **Drag a window onto the outer edge of a card** to put it beside the whole
  group.
- **Drag a window onto an empty area** to give it a screen of its own again.

Side by side needs the **sway** window manager, which `./deps.sh` installs.
RustTouch picks it automatically. Settings → **Windows** lets you change how
new apps open and how splitting works.

## Settings

Swipe down and tap **Settings**. The search box at the top finds a setting
by name. You can change:

- **Appearance**: theme, accent color, home and lock screen backgrounds, a
  message on the lock screen
- **Clock & bars**: 12 or 24 hour time, date style, bar sizes, auto-hide
- **Displays**: resolution, rotation, which screen is the main one (other
  screens show a copy of it)
- **Startup apps**: which programs start when you log in
- **Windows**: side by side behaviour
- **Power & screen**: when the screen turns off, what the power button does
- **Keyboard**: size, style, whether it opens by itself, how fast the text
  cursor blinks
- **Network**: Wi-Fi and Bluetooth

Everything is also saved in one text file, `~/.config/rusttouch/rusttouch.conf`,
if you prefer to edit it by hand.

## Something not working?

- **The log** is at `/run/user/$(id -u)/rusttouch/rusttouch.log`. Most
  problems say why there.
- **Back to your normal desktop**: log out (swipe down → **Log out**) and pick
  your usual session on the login screen.
- **No previews in ■**: install `grim` (`sudo apt install grim`). The card
  says what went wrong if a screenshot fails.
- **No side by side**: Settings → **Windows** tells you whether sway is
  running.

## Taking screenshots

Inside RustTouch, open a terminal and run:

```sh
grim ~/Pictures/rusttouch.png
```

To refresh the pictures in this README, save them over the files in
`docs/screenshots/` with the same names.

## Uninstall

```sh
./uninstall.sh
```

## More

[docs/DETAILS.md](docs/DETAILS.md) explains every feature, setting and how
RustTouch works inside.

## License

RustTouch is free software under the GNU General Public License, version 2.
See [LICENSE](LICENSE).

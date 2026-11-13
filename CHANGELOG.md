# Changelog

The version is in `Cargo.toml` and shows in Settings → About.

## 0.9.16

RustTV:

- Alt+F4 (and Super+Q) could close RustTV itself: they were sway's plain
  `kill`. They now close the app on screen through RustTV, never RustTV.
  And if RustTV ever stops, the session starts it again. (RustTouch's
  shell is layer-shell surfaces, which `kill` can't close - unchanged.)
- **Settings > Power button**: what the remote's power button does -
  *Suspend* (to start with), *Show the power menu*, *Shut down* or
  *Nothing*. RustTV keeps the button from the system (logind, which shut
  down) while it runs (`systemd-inhibit --what=handle-power-key`).
- Launcher icons from your own servers: a server on your own network (a
  private or Tailscale address, a .local name...) is trusted with a
  home-made certificate. When its page gives no icon, a service RustTV
  knows (Plex, Jellyfin, Emby, Frigate, Home Assistant, Immich, Nextcloud,
  Audiobookshelf, Navidrome) gets its icon from the project's official
  site. Icons for your own servers are looked for again after 10 minutes,
  not a day.

## 0.9.15

- RustTV: Settings > AirPlay > **Restart AirPlay** - starts UxPlay again,
  fresh, from the remote (for a device that connects but whose screen
  doesn't show; what restarting RustTV did).

## 0.9.14

RustTV, AirPlay - instead of 0.9.13's approach (tested: a UxPlay that has
had a device sometimes gives the next one no picture; a freshly started
one always does - which is why restarting RustTV fixed it):

- UxPlay starts again, fresh, whenever mirroring ends, in every mode - now
  in about a second (it was given 3 to stop; that was for the register it
  never wrote).
- UxPlay shows the video in a plain X11 video window, decoded by the
  processor (`-avdec -vs xvimagesink`): what came up most reliably when
  tried by hand. Needs `gstreamer1.0-x` (`./deps.sh` and the `.deb` bring
  it).
- Still there: if a device streams but no window comes, UxPlay starts
  again after 6 seconds.

## 0.9.13

RustTV, AirPlay - the phone's screen sometimes didn't show:

- Stopping mirroring and connecting again straight away got no picture:
  RustTV started UxPlay again whenever mirroring ended, which takes a few
  seconds. Now it only does with *Every time* (which needs a new identity
  for the next device); with *First time only* and *Never*, UxPlay stays
  up, ready.
- UxPlay decodes the video with the processor (`-avdec`): with the
  graphics card's decoder the window sometimes never came. (Tested: with
  it, the window comes every time.)

## 0.9.12

RustTV, AirPlay:

- With *First time only*, a device that had typed the code was asked for
  one again and again (with no code on the TV): RustTV still told UxPlay to
  check returning devices against its register (`-reg`), which UxPlay never
  writes - so with it empty, every returning device was turned away. RustTV
  no longer asks for it: a device that paired is let back in. (RustTV keeps
  its own list of them, for Settings > AirPlay > Devices.)

## 0.9.11

RustTV, AirPlay - the phone's screen still sometimes didn't show:

- The AirPlay window is known by its app id too (`GStreamer`, as UxPlay's
  log showed), not only by its title.
- 6 seconds after a device starts streaming, RustTV checks its window is
  on screen. If not, it shows any window that may be UxPlay's; with none at
  all, it starts UxPlay again (the device drops, and connecting again starts
  clean - no more "connected" with nothing on the TV). Either way the log
  lists the windows open then.

## 0.9.10

RustTV, AirPlay:

- The phone's screen sometimes didn't show on the TV (the phone said it
  was connected; reconnecting a few times helped): UxPlay's X11 window gets
  its name - how RustTV knows it - a moment after it opens. Seen before
  that, it was taken for an app and moved off the home screen without
  showing. RustTV now puts the AirPlay window on screen as soon as it's
  known, whenever that is, and doesn't count it as an app.
- Unpair all (and unpairing one) shows the list without them straight
  away, and the device count on the AirPlay page follows.

## 0.9.9

RustTV, AirPlay (from UxPlay's log of a real pairing):

- The code shows exactly when a device asks for it (UxPlay's "CLIENT MUST
  NOW ENTER PIN" line, with the code it prints), and goes away once the
  device has typed it and is connecting. Before, the steps after typing it
  (`pair-setup-pin`) showed it again.
- The Devices list works: UxPlay doesn't keep its register, so RustTV keeps
  its own (`~/.config/rusttv/airplay.devices`) - each device that connects
  with First time only, with its name and model. Unpairing one gives the
  TV a new identity (a device keeps its pairing itself, so there's no
  sending just one away): it types a code again, and the others do once.
- Docs: AirPlay names clashing on the network (two TVs with one name).

## 0.9.8

RustTV, AirPlay:

- *First time only* didn't remember devices (the Devices list stayed
  empty): RustTV stopped UxPlay outright when starting it again (after
  each mirroring, or a settings change), before it could save the devices
  that typed the code. Now it's asked to stop (and only forced after 3
  seconds).
- The Devices list reads UxPlay's own register too
  (`~/.uxplay.register`), in case it keeps the devices there.
- RustTV's log has the exact command UxPlay is started with.

## 0.9.7

RustTV, AirPlay:

- The black screen: UxPlay's video window can be an X11 one (through
  XWayland), which RustTV didn't know was AirPlay's - so it wasn't put on
  its own screen, didn't show as AirPlay in Open now, and stayed after the
  device went. It's now also known by its title (the TV's AirPlay name).
  And whenever mirroring ends, UxPlay starts again, fresh (what restarting
  RustTV did).
- Unpairing really unpairs: devices know the TV by its AirPlay device ID
  (UxPlay used the network card's), not just its key. The TV now has its
  own (`~/.config/rusttv/airplay.id`): **Unpair all** makes a new one, and
  **Every time** uses a new one for each mirroring.
- Settings > AirPlay > **Devices**: the devices that typed the code, with
  a Search row; OK on one unpairs it, plus Unpair all.
- Each pairing gets its own code (a new one each time UxPlay starts).
- Removed the Video setting (0.9.6): the black screen wasn't the video.

Docs: how to copy RustTV's settings to another box.

## 0.9.6

RustTV, AirPlay:

- Mirroring could show only a black screen (the graphics card's video
  decoder - NVIDIA's above all - with a GL window under sway). New setting
  Settings > AirPlay > **Video**: *Safe* (to start with) decodes with the
  processor into a plain Wayland window (`uxplay -vs waylandsink -avdec`);
  *Automatic* is UxPlay's own choice.
- RustTV's log says when the code shows and why it's put away, and when
  the AirPlay window opens (`grep -E "uxplay|airplay"
  /run/user/$(id -u)/rusttv/sway.log`).

## 0.9.5

RustTV, AirPlay:

- The code showed for only a second: it was put away when the device's
  connection closed, but pairing itself closes the first connection (and
  connects again once the code is typed). Now it stays until Back (or any
  button), until mirroring starts, or for a minute. A button already on
  its way as it shows doesn't close it either.

## 0.9.4

RustTV, AirPlay:

- **Forget devices** and **Ask for a code: Every time** didn't make devices
  type the code again: a device that paired once remembers the TV by its
  AirPlay key, and the TV kept the same key. Now Forget devices throws the
  key away too (the TV is new to every device), and Every time uses a new
  key for each device - when mirroring ends, UxPlay starts again with a
  fresh one.

## 0.9.3

RustTV, AirPlay:

- The code on the TV can be put away: Back (or any button, or a click)
  closes it. While it shows it has the keys - Home and the volume still
  work. It also goes by itself when the device cancels or disconnects
  (and, as before, when mirroring starts, or after a minute).

## 0.9.2

RustTV:

- **Search** when adding a launcher or an app (and in any long list): a
  Search row at the top opens the on-screen keyboard with what matches
  beside it, narrowing with every letter. Right goes from the keys to the
  results, Done to the first one, OK picks it; a real keyboard types too.
- AirPlay: the code didn't show on the TV when a device asked for it -
  UxPlay's messages waited in a buffer (it wasn't writing to a terminal).
  RustTV now has it write them line by line (`stdbuf`), so the code pops up
  as soon as it's asked for. Settings > AirPlay shows the code for new
  devices too.
- Pictures of the search and of Add a launcher.

## 0.9.1

RustTV, AirPlay:

- UxPlay needs GStreamer's plugins (`gstreamer1.0-plugins-bad`, `-good`,
  `-libav`, `-gl`), which Ubuntu's uxplay doesn't bring along: without them
  it stops straight away and the TV never shows up. `./deps.sh` and the
  `.deb` install them now, and Settings > AirPlay says when they're missing,
  with the command to install them.
- Settings > AirPlay said "Ready" while UxPlay kept stopping: it says
  "Starting..." until UxPlay has kept going for a few seconds.

## 0.9.0

RustTV:

- **AirPlay**: an iPhone, iPad or Mac shows its screen on the TV (Screen
  Mirroring), or sends it a video or music. RustTV runs UxPlay
  (`sudo apt install uxplay`) in the background and starts it again if it
  stops. The mirrored screen fills the TV, shows in "Open now" as AirPlay
  (Close stops it), and the TV goes back home when the device stops.
  A new device types a 4-digit code that the TV shows big, over whatever is
  on screen - the first time only (to start with), every time, or never.
  Settings > AirPlay: on/off, the TV's name, when to ask for the code, and
  forget devices.
- `./deps.sh` and the `.deb` bring UxPlay along (and avahi-daemon, which it
  needs to show up on the network).

Docs:

- No more text drawings: RustTouch's guide shows its pictures instead.
- Pictures of RustTV's AirPlay settings and code.

## 0.8.6

- Pictures of RustTV (`docs/screenshots/rusttv/`): the home screen,
  Settings, Wi-Fi, Sound and the keyboard for apps. They're mock-ups drawn
  from RustTV's own style sheet and layout; `docs/RUSTTV.md` says how to
  take real ones on the TV.
- The README shows one picture of each desktop; each guide has the rest
  (`docs/RUSTTOUCH.md` now shows its Settings picture too).
- The `.tar.gz` brings RustTV's pictures along with its guide.

## 0.8.5

- The `.deb` is now **`rustdesktop`** (`rustdesktop_<version>_<arch>.deb`)
  and has both desktops: RustTouch and RustTV (RustTV's session, its sway
  config, and its autologin and update helpers in `/usr/libexec` with their
  polkit rules). It replaces the old `rusttouch` package: `apt install`
  swaps it over, keeping your settings.
- The download is `rustdesktop-<version>-linux-<cpu>.tar.gz`, with RustTV
  built too, and `install-rusttv.sh` / `uninstall-rusttv.sh` in it.
- The Cargo package is `rustdesktop`; the programs keep their names
  (`rusttouch`, `rusttouch-settings`, `rusttv`).
- RustTV finds its helpers in `/usr/local/libexec` (installed from source)
  or `/usr/libexec` (the `.deb`).
- Uninstalling one desktop leaves the bundled gtk4-layer-shell copy in place
  while the other still uses it.
- The build workflow checks that `rusttv` from the `.deb` finds its
  gtk4-layer-shell too.

## 0.8.4

The project is now **RustDesktop** (the repository: RustTouch and RustTV).
The programs, the `.deb` and the Cargo package keep their names.

- `install.sh` is now `install-rusttouch.sh`, and `uninstall.sh` is
  `uninstall-rusttouch.sh` (next to `install-rusttv.sh` and
  `uninstall-rusttv.sh`). `just install-tv` / `just uninstall-tv` for RustTV.
- Removed `uninstall-rdesk.sh` (and the install script no longer looks for
  the old rdesk install), `run-windowed.sh` and `run-nested.sh` (and
  `just try` / `just nested`). `just build-windowed` still makes the
  windowed preview build. `ensure-layer-shell.sh` and `build-layer-shell.sh`
  stay: the install scripts, `deps.sh` and the release builds use them.
- `./deps.sh` installs RustTV's helpers too (Chromium, pkexec, openssl).
- Docs: a new README for the whole project; RustTouch's guide moved to
  `docs/RUSTTOUCH.md`, its pictures to `docs/screenshots/rusttouch/`;
  `docs/RUSTTV.md` brought up to date (install, media buttons, pause when
  going home, all the `--do` jobs).
- The build workflow also checks RustTV's session scripts.

## 0.8.3

RustTV:

- Going back home pauses what was playing in the app (Settings > Pause
  when going home to turn it off).
- More jobs for remote buttons: play / pause, back and forward 10 seconds,
  previous, next, stop - for whatever is playing (through MPRIS, which
  Firefox, Chromium and media players offer).
- Each launcher can have its own remote buttons (Edit > Remote buttons),
  working while it's on screen: the media jobs, sending the app a key
  (Space, the arrows, Enter, Esc, F, M, K, J, L, Page Up/Down - the site's
  own shortcuts), or Home, close, keyboard, volume. Elsewhere those buttons
  keep their usual job.

## 0.8.2

RustTV:

- An on-screen keyboard for apps: it comes up at the bottom when a text
  box on a web page (or in an app) gets the focus. The remote's arrows move
  around its keys, OK types one, Back puts it away; a box above the keys
  shows what's typed so far. Typing on a real keyboard puts it away and the
  keys go straight to the page. Settings > Keyboard in apps turns it off;
  a remote button can be given "Show the keyboard" (Settings > Remote
  buttons) to bring it back in the same text box.
  (RustTV is the input method; Chromium launchers are started with its
  Wayland text-input switched on. Over apps it needs gtk4-layer-shell, like
  the volume pop-up.)

## 0.8.1

RustTV:

- Sound: the volume is a row with − and + (5% a press, like the remote)
  and a bar; it follows the remote's volume buttons while it's open, and
  Mute too.
- The on-screen keyboard's Shift: once for the next letter only, twice in
  a row for caps lock (⇪), once more to turn it off.

## 0.8.0

RustTV:

- Settings > Connections:
  - Wi-Fi: turn it on or off, scan, connect (the password typed with the
    remote, shown as dots - the eye key shows it), disconnect, forget.
  - Bluetooth: turn it on or off, search for devices (8 seconds), connect
    (pairs first), disconnect, rename, forget. Keyboards, mice and game
    pads aren't trusted to reconnect by themselves, like in RustTouch.
  - Sound: volume, mute, the output and input to use, and a Bluetooth
    headset's microphone on or off.
- Settings > Screen > Screens: with more than one screen, which one RustTV
  is on (automatic: the TV over a laptop's own screen) - the others show a
  copy of it (Fit / Fill / Sharp); each screen's resolution.
- (Wi-Fi, Bluetooth and sound work the same way as RustTouch's - much of
  it is RustTouch's own code.)

## 0.7.19

RustTV:

- Battery (when there is one): its level at the top of the home screen
  (⚡ when charging, red when low), and a warning over everything when it
  runs low - again near empty. Settings > Battery: Battery icon on/off,
  Low battery warning at 5-30% or off.
- Settings > Look: Accent color (the focus outline and highlights - white,
  blue, teal, green, yellow, orange, red, pink, purple, slate) and
  Background (dark, black, accent glow, or a picture from your Pictures
  folder or the system's wallpapers, dimmed so the cards stand out).

## 0.7.18

RustTV:

- Moving along a row keeps room for the focused card's white outline: going
  all the way right and back left no longer cuts off its left edge.

## 0.7.17

RustTV:

- Server addresses start with https:// (an address typed without one gets
  https:// too); type http:// for a server without https.
- New ready-made launcher: Home Assistant (your own - it asks for the
  address), with Home Assistant's own icon.

## 0.7.16

RustTV:

- Launcher option "Trust certificate": for a server at home with a
  home-made certificate (Frigate, a camera, a router on an IP address),
  Chromium trusts the certificate that server has, so the "Your connection
  is not private" page stops coming back each time it starts. Only that
  certificate is trusted - if the server's certificate changes, the warning
  is back (turn it off and on again to take the new one). Firefox and
  Waterfox remember "Accept the risk" by themselves.

## 0.7.15

RustTV:

- Frigate's card gets Frigate's own icon, from the Frigate project itself
  (your server's page may be behind its sign-in or a home-made
  certificate). A Frigate launcher you added already picks it up.
- A dark logo on a see-through background (like Frigate's black bird) is
  put on white, so it doesn't vanish on the card's dark colour.

## 0.7.14

RustTV:

- Simpler key hints at the bottom of the screens, keys in bold:
  "OK Open · Hold OK Options · Home This screen" on the home screen,
  "OK Select · Back Back" on the others, "OK Type · Back Cancel" on the
  keyboard.

## 0.7.13

RustTV:

- Open now finds the app a window belongs to more often - by the snap or
  Flatpak it runs in, and by looser name matches (snaps name their
  desktop files "<snap>_<app>") - so the Snap Store and the like show their
  icon. Opening an app that's open already switches to it in the same way.
- New ready-made launcher: Frigate (your own camera server - it asks for
  the address).

## 0.7.12

RustTV:

- A tighter home screen: smaller gaps between the rows and around the
  clock, slightly smaller cards - Open now, Watch and Apps all fit on a
  1080p screen without scrolling.
- Apps in Open now show their icon and name, like in Apps.

## 0.7.11

RustTV:

- Card option "Open for sign-in": opens a launcher as a plain web page (no
  TV headers, no spatial selecting - they get in the way of some sign-in
  pages, Hulu's for one) in its usual browser and profile, so the sign-in
  is kept. Opening it normally afterwards offers to reload it.
- Turning on Spatial selecting for a launcher in Firefox or Waterfox moves
  it to Chromium for good: turning it off again no longer switches back to
  the other browser (with its own, separate sign-in).
- Reloading or restarting a browser waits until the old one has really
  quit - starting the new one sooner could lose what it was still saving
  (sign-ins).

## 0.7.10

RustTV:

- Moving up and down the home screen shows the whole row, its title too:
  scrolling only as far as the card left "Open now" tucked under the
  clock, with the tail of its "p" showing (the "white dot").

## 0.7.9

RustTV:

- Picking an open launcher in Open now also asks to reload it when it was
  edited since it opened (Reload / Open as is).
- Shorter questions and answers everywhere: "Clear Hulu's history?" -
  Clear / Cancel, "Remove Plex?" - Remove / Cancel, "Turn on autologin?",
  card options Switch / Close / Remove, remote buttons Add button / Clear
  buttons. The action comes first, Cancel has the focus.

## 0.7.8

RustTV:

- Simpler settings: Max open apps (a number; 1 to start with - opening
  one more closes the oldest), Separate history (on to start with),
  Default browser, Scale, Autologin; in a launcher: Address, Color, Icon,
  Reload icon, Send TV headers, Spatial selecting (Chrome-based browsers
  only), Clear history.
- Power: Suspend.
- Auto suspend after 15 minutes to 4 hours with nothing going on (video
  playing keeps it awake; needs swayidle).
- Updates: checks and installs system (apt), snap and Flatpak updates,
  through a small helper (installed by install-rusttv.sh).

## 0.7.7

RustTV:

- Messages ("Opening Hulu...") and the volume pop-up show at the top of
  the screen - subtitles are at the bottom.
- Only the card with the focus has the white outline: the mouse pointer
  (even hidden) lit up a second one when the cards under it moved, e.g.
  after closing an app from Open now. The focus also comes back to a card
  when the one it was on goes away.
- The Open now row is always there, with "Nothing open right now" when
  nothing is.
- No stray white dot over the rows (the scrolling areas' edge marks and
  outline are gone).

## 0.7.6

RustTV:

- A volume pop-up shows how loud it is (or "Muted") for a moment when you
  turn the volume up or down or mute it - over every app. Turning it up or
  down while muted unmutes, like a TV. (Over apps it needs gtk4-layer-shell,
  which install-rusttv.sh now builds RustTV with when it's there - RustTouch
  needs it too; without it the volume shows on RustTV's own screen.)

## 0.7.5

RustTV:

- Opening a launcher you've edited since it opened asks: reload it with
  the changes, or just show it as it is.
- New launchers start out in Chromium (or Chrome), saying they're a TV,
  with the arrows jumping between buttons - except YouTube, whose TV page
  is made for the arrows already. ("Browser for new launchers" now starts
  at Chromium too; launchers you have keep their settings.)
- In Chromium, saying it's a TV is a switch it's started with, so such
  launchers keep your Chromium sign-ins; when Chromium is already open
  with other switches, it's started afresh for the launcher.

## 0.7.4

RustTV:

- Launcher option "Arrow keys jump between buttons": opens it in Chromium
  (or Chrome) with spatial navigation, so the remote's arrows move between
  a desktop site's buttons and links (Hulu, Netflix...).
- Cards showing a service's logo sit on a dark shade of its colour instead
  of the full, bright colour.
- Settings > About RustTV opens a page: version, system, browsers found,
  and where the settings and log are.

## 0.7.3

- RustTouch and RustTV start on NVIDIA's own (proprietary) driver: sway
  refuses it unless started with --unsupported-gpu, so the session ended
  straight away at login. Both sessions now pass that when the NVIDIA
  driver is loaded (and turn off the hardware cursor, which can be
  invisible on it).

## 0.7.2

RustTV:

- Launchers open with your browser's own profile - your history and
  sign-ins - instead of an empty one each (Settings > Launchers' sign-ins
  and history to go back to separate ones). A launcher that says it's a TV
  keeps a profile of its own.
- One app at a time closes the other apps once the new one's window shows
  (no black screen while it starts, and a shared browser isn't caught
  quitting).

## 0.7.1

RustTV (RustTouch is unchanged):

- Browsers installed as a Flatpak (Waterfox's is) or a snap are found, as
  well as browsers unpacked in /opt or your home folder.
- Launcher cards show the service's own icon, found on its page (its big
  touch icon or web app icons), on its colour - or its colour and name
  when there's no good one. Edit: turn it off, or look for it again.
- "Say it's a TV" (per launcher): the browser tells sites it's a TV, so
  YouTube shows its TV layout, made for a remote. The ready-made YouTube
  launcher uses it.
- One app at a time: opening something closes what was open (Settings to
  turn it off).
- Settings > Remote buttons: give any button on your remote a job - OK,
  Back, options, the arrows (in RustTV), Home, close the app, volume (in
  every app).

## 0.7.0

- New: **RustTV**, a separate home screen for a TV box, made for a remote
  control (see docs/RUSTTV.md). Its own program (`rusttv`), login session,
  settings and sway config - RustTouch is unchanged.
  - rows of big cards: what's open now, launchers ("Watch") and apps;
    arrows, OK, Back, Home, and hold OK / Menu for a card's options
  - launchers: web services (Plex, Jellyfin, Emby, Netflix, Hulu, HBO Max,
    Disney+, Prime Video, YouTube and more, or any address) full screen in
    Firefox, Waterfox, Chromium or Chrome kiosk mode, each with its own
    browser profile (own sign-ins), protected video turned on
  - settings with the remote: launchers, apps, browser, screen size, log in
    by itself (GDM, LightDM, SDDM), restart / turn off / log out
  - an on-screen keyboard for the remote
  - `./install-rusttv.sh` / `./uninstall-rusttv.sh`

## 0.6.30

- Keyboard, touch vs mouse:
  - a tap on the touchpad (tap-to-click) counts as a mouse click - it
    used to go unnoticed, so the on-screen keyboard came up or stayed up;
  - clicking the ⌨ button with the mouse opens the keyboard (the click no
    longer counts as "using the mouse" and puts it straight away again),
    and the text box you were in stays selected;
  - a newly opened app's text box follows what you used last as soon as
    its window shows (it used to wait up to 10 seconds, so a tap on a text
    box in that time could be ignored).
- Alt+F4 closes the window (sway).
- Home screen: the spot that lights up (and the pressed icon) is always
  about an icon's size; at 100% on a big screen it filled a much larger
  area. The grid is also made again after the display scale changes.
- Displays: the choices for other screens are now named Fit, Fill and
  Sharp ("Picture position on other screens").

## 0.6.29

- On-screen keyboard follows what you use, everywhere: a mouse click or
  typing on a real keyboard puts it away, and text boxes then don't bring
  it up by themselves (an app opening in one, switching windows); touching
  anything - or a text box, even one that's already active - brings it
  back. Real-keyboard typing is noticed from the text box's text changing
  (RustTouch never reads your keys). The Super key and Escape count as the
  real keyboard, so Super twice no longer pops the keyboard up.
- Lock screen: other screens show a copy of it instead of staying black.
- Displays: how other screens show the copy - Fit (whole picture, black
  bars), Fill (edge to edge, edges cut off) or Exact (sharp, pixel for
  pixel).
- Home screen: the grid's spots stay about an icon's size at every display
  scale (at 100% there were only 8 huge columns), and the pressed look of
  an icon is now the same size as the spot that lights up when you drag one.
- Recents: the cards have the shape of the space apps get (screen minus the
  bars), with no dark strip above the windows; this mattered most at bigger
  scales.
- Mouse pointer: a resize arrow over the grab areas between windows, and a
  hand over things you can click (also in Settings).

## 0.6.28

- Lock screen: the battery level and whether it's charging, under the date.
- Keyboard: an app opened with the mouse that starts in a text box doesn't
  bring up the on-screen keyboard (tap the box to get it). Same for
  switching back to an app with the mouse from recents.
- The Super key again leaves recents (or the window arranger) instead of
  jumping to another app.
- Recents with the mouse: drag a card up to close it, like a finger; drag
  it sideways or down (or press and hold first) to move the window into a
  group.
- Pull-down: the mouse's back button leaves the Wi-Fi, Bluetooth, Sound or
  tray app page; forward goes back into it.
- Rotate: turning it on no longer makes an external monitor go black for a
  second. On sway the screen is turned through sway itself, and only when
  it really needs turning (wlr-randr re-sent every monitor's settings).

## 0.6.27

- Escape closes the app drawer, the pull-down, recents and the window
  arranger (on sway, Escape is only taken while one of them shows).
- The Windows/Super key on its own opens recents (Super + another key
  still does that key's job).
- Settings: Escape and the mouse's back button go back (closing an open
  choice list first); the mouse's forward button goes forward again.
- Mouse: dragging a window in the window arranger, or an icon or card,
  works right away with the mouse - no press-and-hold first, just like
  touch. Recents no longer drag-scrolls with the mouse (it got in the way).
- Keyboard: clicking a text box with the mouse hides the on-screen keyboard
  even if a tap had brought it up; tapping a text box after clicking it
  brings the keyboard back sooner (half a second after a click).

## 0.6.26

- Mouse: the pointer stays on the main screen. Other screens only show a
  copy, but sway counted them as more room, so the pointer wandered onto
  them ("wrapping" left/right and up/down).
- Mouse: drag lists to scroll them like a finger does - the app drawer,
  recents, the pull-down's lists (GTK only drag-scrolls with touch).
  Pressing and holding first still moves icons and cards.
- Settings > Mouse & touchpad (new): natural scrolling (on by default), the
  mouse pointer (moved here from Displays), and "Mouse click hides the
  keyboard".
- On-screen keyboard: clicking with a mouse anywhere puts it away, and a
  text box clicked with the mouse doesn't bring it up (on by default;
  Settings > Keyboard or Mouse & touchpad). Clicking the keyboard itself
  doesn't count. RustTouch only looks at mouse button presses for this
  (install.sh gives it access to mice and touchpads, like the buttons).

## 0.6.25

- Security follow-up: PAM's answer callback checks everything before
  allocating its replies (it leaked them when called without data); the
  comment on when_locked() says it waits as long as the compositor takes;
  README says plainly that closing the lid only locks where the session
  lock works (on other window managers the computer sleeps and wakes
  unlocked).

- Window groups, for real this time: dropping a window on a group's edge
  line puts it on that side of *all* the group's windows, taking that half
  of the screen, with the group exactly as it was on the other half.
  Checked against sway's own code why it went wrong before: "move to
  workspace" puts a window next to whatever was last focused there - often
  inside a stack - so the new window kept landing in the wrong place. Now
  the group is gathered into one container and set aside, the new window
  is put on the empty group workspace, and the group is moved back next to
  it (moving onto a single window is the one placement sway does
  predictably); for right / bottom the two then swap.
  Dropping onto a half of one window still puts it into that half of that
  window.

## 0.6.24

- Keyboard (wide landscape layout): the → key is as wide as the other
  arrow keys. It was half a key wider, which also made its row half a key
  longer than the others.

## 0.6.23

- App icons that aren't in the icon theme in use are looked up in the other
  installed icon themes (e.g. "Additional Drivers", whose icon is only in
  Ubuntu's Yaru theme, showed no icon); if none has it, a generic app icon.

## 0.6.22

Third security review.

- Sleep: only the compositor's "locked" event lets the system sleep -
  there's no longer a 3-second timeout that went ahead without it (the log
  just says it's still waiting). If locking fails, what was waiting for it
  (letting the system sleep, the Sleep button) doesn't happen.
- PAM: only the first hidden prompt gets the password. A second hidden
  prompt (a one-time code, another factor) makes the check fail instead of
  being handed the password.
- sway: answers bigger than 16 MB are refused before any memory is set
  aside for them.
- Wi-Fi: joining a network with a password keeps the network's saved
  profile until the new one has connected, and only then replaces it - a
  failed try no longer loses the saved password.
- Pull-down: Lock / Sleep / Restart... buttons get 4px more room at the
  bottom.

## 0.6.21

- Window groups: dropping a window on the left (or top) edge line of a
  group now puts it on the left (top) half, with the group as it was on the
  other half - e.g. two windows stacked on top of each other stay stacked
  on the right. It used to land on the wrong side: the window was put in
  place with `swap`, which didn't work with a whole group. Now the group is
  moved past the window instead. Same fix for dropping onto the left / top
  edge of a single window. Dropping on the edge of a row of windows now
  also gives the new window half the screen (it used to be squeezed in
  between, or into a stack at the end of the row).
- Settings: the close button shows its ✕ again (the desktop's own close
  button drew only an empty circle).

## 0.6.20

- Pull-down: after tapping Wi-Fi or Bluetooth, holding it now always opens
  its list. The hold is timed by RustTouch itself from the touch events
  (0.5s without moving) instead of GTK's long-press gesture, which after a
  tap on the same button could stop firing. Letting go on the tile opens
  the list (and doesn't also switch Wi-Fi/Bluetooth); sliding off and
  letting go does nothing; moving before 0.5s is a swipe.
- Pull-down: Lock / Sleep / Restart... icon and label centered in the
  button (0.6.19 moved them down too far).
- Wi-Fi password box: the show-password eye sits further in from the round
  end, with more room around it.

## 0.6.19

- Pull-down: holding Wi-Fi or Bluetooth always works now, without closing
  and reopening the pull-down - every new touch on a tile first drops
  whatever was left of an earlier hold.
- Wi-Fi / Bluetooth lists: an on/off switch in the header, so you can turn
  them on (or off) right there.
- Wi-Fi / Bluetooth lists: only the network or device you're connected to
  is highlighted (filled), and the one you're connecting to (outlined,
  "Connecting…"). Touching a row to scroll the list no longer highlights it.
- Pull-down: the icons and labels on Lock / Sleep / Restart / Power off /
  Log out sit about 10px lower.

## 0.6.18

- Pull-down: fixed holding Wi-Fi or Bluetooth stopping working (until you
  logged in again). When Wi-Fi was switched off, a tile could briefly stop
  taking touches while a finger was on it, and GTK never let go of that
  touch. The tiles now always take touches (if Wi-Fi/Bluetooth is
  unavailable, tapping just does nothing), and any hold still going on when
  the pull-down closes is dropped.
- Settings > Network: Wi-Fi > Networks and Bluetooth > Devices open the
  pull-down's own lists - the same ones holding the tiles opens - so
  networks and devices are handled in one place. "Advanced network
  settings" (nm-connection-editor) is still there for VPNs and the like.
  `rusttouch --do wifi-list` / `bluetooth-list` open them too.

## 0.6.17

- Wi-Fi: picking a network that needs a password now opens its own card,
  outlined in the accent color: "Join “<network>”", "Type the Wi-Fi
  password", the password box, Cancel and Connect. The list of networks
  below dims while it's open, so it's clear you've moved on from choosing a
  network to joining it.

## 0.6.16

- Battery: any power supply that isn't a battery and says it's online now
  counts as "on power" (not only ones calling themselves Mains/USB), and a
  charger is never skipped for its scope (only pen/mouse batteries are).
  Battery changes are written to the log ("battery 87%, on power").
- Home screen: the grid is redrawn for the real screen size as soon as it's
  known (and when screens change). It was laid out at startup before the
  screen size was known, so the first app dragged onto it suddenly changed
  the grid.

## 0.6.15

- Fixed the build: saved Wi-Fi settings are read with VariantDict
  (glib's Variant has no lookup_value).
- On a window manager without the secure session lock (not sway), a notice
  shows for 10 seconds after logging in: "This screen can't be locked
  securely here (the window manager has no session lock - use sway)". Once
  per login, not again after a crash restart.

## 0.6.14

Follow-up to the second security review.

- No secure lock (a window manager without the session lock): "Lock",
  "Sleep", the screen-off timer and the power button's screen-off no longer
  turn the screen off or put the device to sleep after locking failed - the
  screen stays on with the warning, for 6 seconds. (Before, the warning
  flashed and the screen went dark, unlocked.) The warning now says "needs
  a window manager with a session lock" (labwc and newer phoc have one too).
- Button devices: back to access for the person at the device only
  (`uaccess`). 0.6.13's read-only-for-everyone rule let other accounts, SSH
  sessions and services read them.
- Bluetooth: Low Energy keyboards (HID over GATT, 0x1812) count as input
  devices too, so they aren't trusted automatically.
- Wrong-password waits: never more than 10 minutes even if the clock went
  backwards, not skipped if it jumps forward, and still counted when
  ~/.local/state can't be written (kept in memory too).
- Tray: an app's own icon folder is only used for apps running on the
  host, not in a Flatpak or Snap sandbox (checked from the app's process).

## 0.6.13

Second security review.

- **build-layer-shell.sh** (Ubuntu 24.04 source builds): builds in a fresh
  private folder (`mktemp -d`, removed afterwards) instead of a fixed name in
  /tmp that another user could make first and use to get their files run by
  `sudo ninja install`; and checks the source is the pinned commit of
  v1.2.0, not just the tag.
- **Tray**: an app could crash RustTouch with a huge icon (32768x32768
  overflowed the size check). Icon sizes are now worked out without
  overflow, capped at 1024x1024 and the data must be exactly the right size;
  the icon and menu data are type-checked (menus at most 8 deep and 500
  entries). File paths given as icon names are ignored, and an app's own
  icon folder is used for that app only (it used to be added to the icon
  theme of the whole shell).
- **Lock screen fails closed**: without the session-lock protocol RustTouch
  no longer shows a cover a crash would remove - it says "Can't lock here"
  instead (`insecure_lock = on` in rusttouch.conf brings the cover back). If
  the compositor ends the lock by itself, RustTouch locks again.
- **Slower guessing**: after 5 wrong passwords in a row, a 30s wait,
  doubling with each further wrong one (up to 10 minutes), with a countdown.
  Kept in ~/.local/state/rusttouch/lock-tries, so restarts don't reset it.
- **Crash restarts never give up** while the session runs (waiting 1s, 2s,
  4s... up to 15s between tries), so a locked screen always gets its unlock
  screen back.
- **Wi-Fi**: saved networks are found by the network's own name and only
  among Wi-Fi profiles, and connected, disconnected and forgotten through
  NetworkManager's D-Bus API by that profile - `nmcli connection
  up/down/delete id <name>` could hit a VPN or wired profile with the same
  name as a network.
- **Nothing of the shell opens over the lock screen**: the menus (also from
  holding the power button) check for the lock first.
- **Sleep waits for the lock**: the system only goes to sleep once the
  compositor confirms the lock screen is up (3s at most), instead of after a
  fixed 0.4s.
- Lock screen: the account comes from the system's user list (getpwuid),
  not $USER; no "show password" eye; going back to the clock clears what was
  typed.
- Screen-on requests (org.freedesktop.ScreenSaver): at most 8 per app and
  64 in all, and only the app that asked can take one back.
- Bluetooth: keyboards, mice and game pads (and anything unknown) aren't
  trusted automatically after pairing.
- udev rule: the button devices are read-only now (it used to allow writing,
  which could fake button presses).
- README: check SHA256SUMS (and optionally the attestation) before
  installing a download; a longer Security section.
- CI: `cargo deny check advisories bans sources` next to cargo audit
  (deny.toml).

## 0.6.12

- Recents: no more 1px line between the button bar and the shaded recents
  area. Recents is now placed like the app drawer (between the bars by
  margins) and reaches 2px under the button bar, which is drawn on top.

## 0.6.11

- Low battery warning: the test switch ("warn at every 1% down") is gone;
  it warns once, at the level set in Settings > Power.
- Settings: all accent swatches are exactly the same round shape and size.
  The chosen one is marked with a ring drawn around it (outside the circle)
  and a ✓, instead of a border that changed its shape.

## 0.6.10

- Low battery warning: a popup ("Battery low · 14%") and a sound when the
  battery gets down to the level in Settings > Power (5-30%, default 15%),
  once, while not charging. Settings > Power > Low battery: the level,
  "Sound and popup" / "Popup only" / "Off", and "Hear it now".
  **Test switch, on for now:** "Test: warn at every 1% down" warns at every
  percent the battery drops, to try the sound - turn it off (or remove it)
  for normal use. The sound is the sound theme's battery-low (Yaru on
  Ubuntu), played with canberra-gtk-play, paplay or pw-play.
- Settings > Displays: no more "Main screen" choice (other screens always
  show a copy of the built-in one). One Scale for everything; each screen
  has its resolution and refresh rate and its rotation, and other screens
  an on/off switch. An old main-screen choice is dropped.
- Home screen: the remove strip opens only right at the top edge (56px,
  removes at 30px), so moving icons around the top row doesn't pull it out.
- Settings: the custom accent swatch is the same shape as the presets.

## 0.6.9

- Settings: the drop-down lists are gone. Tapping a choice now opens the
  whole list over the window, Android-style: big rows, a round mark on the
  chosen one, the list starts scrolled to it, scroll with a finger, tap one
  to pick it, tap outside or Cancel to close. Long lists (themes, startup
  apps) can be scrolled and picked from by touch now.
- Settings: the chosen accent color is clear - it gets a ✓ and a ring
  around it (the ring used to be drawn in the accent color itself, so it
  vanished on the chosen swatch). Picking a preset after a custom color
  takes the mark off the custom one.
- Settings: the close button is the desktop's own round one, like other
  apps' title bars (only close). Without an icon theme, a small round ✕.
- Home screen: the remove area is now a thin strip hanging from the top
  edge while you drag an icon. It opens up like a drawer as your finger
  gets near the top, and the icon is only removed when let go right at the
  edge ("Let go to remove"), so the top row is easy to drop onto. The icon
  fades over it.
- Top bar: the battery is checked every 3 seconds on its own, so plugging
  in, unplugging and the percentage show right away (it waited for the full
  status check, which could stop updating after the screen had been off).

### Security (fixes for the security review)

- Lock screen: now a real session lock (ext-session-lock-v1, through
  gtk4-session-lock) wherever the window manager offers one, so a crash or
  kill no longer unlocks the screen - the window manager keeps it covered.
  The session starts RustTouch again if it crashes, and a restarted
  RustTouch locks again straight away (a "locked" marker in the private
  runtime folder; a new login clears it). Gives up after 5 crashes in a
  minute; stops when you log out. Without the protocol it falls back to the
  old plain cover and the log says WARNING.
- Wi-Fi passwords: sent to NetworkManager over its D-Bus API (new
  `src/nm.rs`), never on a command line where `/proc/<pid>/cmdline` shows
  them. A network saved earlier under the same name is replaced; a failed
  join removes the new one again.
- PAM: only a hidden password prompt gets the password; a visible prompt
  (user name, one-time code) fails the check instead. Allocation failures
  are handled, and copies of the password are wiped after use.
- Runtime folder: never a shared folder. It's `$XDG_RUNTIME_DIR/rusttouch`
  when that's a private folder of yours, otherwise `~/.cache/rusttouch-run`
  (it used to fall back to /tmp). Made with mode 0700 and checked to be a
  real folder (not a link) owned by you; the control pipe is made with mode
  0600 and checked the same way. The session scripts use the same folder.
- README: a Security section.
- Builds on gtk4 0.11 (the 0.6.8 upgrade) - gtk4-session-lock 0.4.

## 0.6.8

- Builds again with gtk4 0.11 / gtk4-layer-shell 0.8. `DesktopAppInfo` moved
  out of `gio` into the new `gio-unix` crate (added as a dependency), and the
  Slide widget now lists the interfaces every GTK widget has
  (`Accessible`, `Buildable`, `ConstraintTarget`), which gtk4 0.11 requires.
- Quieted the `signal_subscribe` deprecation warnings in the tray and the
  screen-saver inhibitor. Its replacement drops the subscription when its
  handle is dropped, and these need to last the whole session.

## 0.6.7

- Wi-Fi/Bluetooth tiles: no more flash of the accent color in the corners
  while holding them. Each tile now sits in a round frame that clips
  everything drawn on it (the theme's square pressed look included), the
  theme's animations are off for the tiles, and the hold glow is drawn inside
  the tile.
- Downloads: `just deb` makes `rusttouch_<version>_<amd64|arm64>.deb` and
  `just package` makes `rusttouch-<version>-linux-<cpu>.tar.gz` (already
  built: unpack, `./deps.sh`, `./install.sh`). Both bring their own copy of
  gtk4-layer-shell, which Ubuntu 24.04 doesn't package. Needs `just`; the
  old scripts still work as before.
- GitHub Actions: `build` builds and tests every push on Ubuntu 24.04
  (Intel/AMD and ARM) and 26.04 and checks the .deb installs; `release`
  makes a draft release with the .deb and .tar.gz for both CPUs when you
  push a tag like v0.6.7; `audit` checks the Rust libraries weekly.
- The layer-shell scripts work on ARM too (they assumed Intel/AMD paths).
- README: rewritten in the same style as rustsync, with a logo, the six
  words to know, a picture of what starts at login, install from the .deb,
  .tar.gz or source, building and releasing, and a troubleshooting table.

## 0.6.6

- Volume/brightness popup: slide (or tap) along it with your finger to set
  the level while it shows. It stays up while your finger is on it.
- Wi-Fi/Bluetooth hold: the list only opens if you let go on the tile.
  Sliding off takes the glow away, and letting go there does nothing.
- Tiles stay round while pressed (the theme's square pressed look flashed
  when holding them).
- Recents previews: the keyboard is out of the picture again - after the app
  starts growing, it gets 200ms to redraw before the picture (450ms at most).
- Black screen at login (it came back when a portal from the login screen
  was left half started): the session now starts the desktop portal itself
  after handing over the screen details, which clears any request left
  hanging, and resets portals that failed earlier. RustTouch also tells GTK
  harder not to use portals itself.
- Fewer "GtkGizmo (slider)" warnings in the log.

## 0.6.5

- Split screen: the white/grey handle is gone. Drag the black line itself.
- Full screen: the bars and menus now go away again after the auto-hide
  time. The app counts as "in front" by being the one you used last, not by
  having focus (the menus take the focus while open, which made RustTouch
  think the full-screen app had gone and back, over and over). Surfaces are
  taken off the screen while they move between layers instead of being moved
  while showing - a likely cause of the crash in the last log.
- Recents previews are faster: instead of a fixed half-second wait, the
  picture is taken as soon as sway reports the app has filled the keyboard's
  space (usually 50-150ms).
- Coming back from recents to an app whose text box still has the cursor
  brings the keyboard back.
- Pull-down: holding Wi-Fi or Bluetooth now says "Edit settings".
- Logs: sway's own messages go to `sway.log` next to `rusttouch.log`, and the
  previous session's logs are kept as `.old` (so a crash can be looked at
  after logging back in).
- Fixed the real source of the "GtkGizmo (slider)" warnings (the hidden
  scroll bars of the recents pictures).
- README: removed the screenshot notes; added what every package deps.sh
  installs is for.

## 0.6.4

- Full-screen apps: the bars now act like auto-hide instead of vanishing for
  good - they go away when the app goes full screen, and a tap or swipe on
  the top or bottom edge brings them back over the app for a few seconds
  (the menus open over the full-screen app too).
- Recents previews: the keyboard is put away and the app gets a moment to
  grow back before its picture is taken, so the picture shows the whole app
  instead of an empty strip where the keyboard was. Background preview
  refreshes are skipped while you're typing.
- Slow login / slow apps: the session now tells systemd the graphical
  session is running (rusttouch-session.target), which the desktop portals
  wait for - Firefox was stalling on "Settings portal not found". The log
  also says how long the portal took to answer (and why, if it didn't), and
  how long each startup step of the shell took.
- Quick settings sliders: fixed the stream of "GtkGizmo (slider) reported
  min width -2" warnings in the log.
- Removed a harmless "sway command failed" line at startup.

## 0.6.3

- Title bars: reverted to how they were before 0.6.0 (no extra bars, no
  GTK_CSD; X11 apps keep the small ✕ in their top right corner; the
  "Title bars" setting is gone).
- Split screen: while you drag, only the black gap moves - the handle hides
  and comes back on the gap when you let go, so the two never come apart.
- Recents: rotating the screen while it's open redraws the cards in the new
  screen shape.

## 0.6.2

- Title bars: GTK3 apps like Terminator now draw GTK's own title bar with ✕
  (GTK_CSD=1 for the session and every app RustTouch starts), the same as
  Firefox and GNOME apps - no RustTouch bar on them anymore.
- X11 apps' title bars look like GTK's: same colors, height, bold centered
  title, a small round ✕.

## 0.6.1

- Split screen: while resizing, the handle stays exactly on the black gap
  between the windows (it's placed where sway really puts the edge after
  each step), instead of following the finger ahead of it.
- Title bars: Terminator gets one by default; listed apps match in any case
  and both as Wayland apps and X11 apps.
- The log now has timestamps for login steps, when rusttouch is ready, and
  how long each app took to show its window (to find slow starts).
- Warnings fixed.

## 0.6.0

- Keyboard: only the RustTouch keyboard now (wvkbd / squeekboard aren't
  offered - they could end up behind the menus). Settings > Keyboard:
  the "Keyboard" choice is gone; new "Layout (landscape)" (wide: every key
  on one page, or compact: like portrait); heights and key shape defaults
  match it (300 / 260 px, square); the preview shows its layout.
- ⌨ shows on the home screen again after a menu was opened and closed.
- Split screen: the handle moves with your finger while resizing (it stayed
  behind), and windows resize more often while you drag.
- Title bars: X11 apps get a title bar (themed, title centered) with ✕ on its
  right. Apps that draw no title bar themselves can be listed in Settings >
  Windows > Title bars to get the same.

## 0.5.1

- Keyboard: a key no longer keeps typing after you lift your finger (the
  release got lost; keys are now plain key tiles that own the touch, and a
  repeat also stops if the key goes away).
- Keyboard: always sits above the button bar (◀ ● ■ at the very bottom),
  and the bar reserves room for both, so apps move up. With the bar hidden
  it sits at the bottom edge.
- App drawer keeps its full size; the keyboard goes over its bottom part.

## 0.5.0

- New: the RustTouch keyboard - one on-screen keyboard for everything (apps,
  terminals, Firefox, the app drawer search, the pull-down, settings); the
  lock screen keeps its own.
  - Always on top (apps and menus included), and apps move up out of its way.
  - wvkbd's layout in portrait (full + symbols/🙂 pages), a full laptop-style
    layout in landscape, an emoji page.
  - Types real key presses, so it works everywhere (terminals and X11 apps
    too); Ctrl / Alt / Super for shortcuts; hold a key to repeat.
  - Colors, height, rounding and font from Settings > Keyboard.
  - It's the default ("Automatic"); wvkbd and squeekboard can still be picked.
- Split screen: the handles between windows disappear right away when you go
  home (they lingered a moment).

## 0.4.4

- Recents: the tall black area under each card is gone (a screenshot asked
  for its full pixel size; now each asks for exactly its spot on the card).
- App drawer keyboard now copies wvkbd's own keyboard: the same keys and
  layout (Esc, Ctr, arrow keys, number row, ⌨→ to the symbols/🙂 layer,
  Enter), square keys, same colors and height.

## 0.4.3

- App drawer search: back to the 0.3 keyboard that worked - RustTouch's own
  keyboard floating over the bottom of the drawer, styled from Settings >
  Keyboard (on sway the keyboard program is drawn underneath the drawer).
- Recents: dropping beside a whole group shows the line along that edge of
  that one group again (inside its card).
- Recents: cards have the shape of the whole screen, windows placed where
  they are on it.

## 0.4.2

- Recents: dragging windows into groups works again - the row no longer
  glides to center a card while you're dragging (it moved the cards out
  from under your finger).
- Auto-hide: after closing a menu the bars stay for the set time.
- Settings: drop-down choices really centered; the search box brings up the
  keyboard when tapped (and still doesn't grab the cursor when the app opens).

## 0.4.1

- App drawer search brings up the keyboard again: while a menu is open, a
  text field quietly losing focus (which happened when the drawer made room
  for the keyboard) no longer takes the keyboard away.
- ⌨ on the button bar is hidden while the app drawer, the pull-down or
  recents shows, as well as while a keyboard is up.
- Auto-hide bars: default wait is 3 seconds.
- Recents, Android-style: the app you just left is on the right, centered;
  older ones to its left; scrolling settles on a card.
- Recents: dropping beside a whole group shows the band on that group again
  (as before 0.3.0).
- Settings: drop-down buttons show their choice centered; the custom accent
  swatch is round like the others.

## 0.4.0

- The on-screen keyboard you chose is used everywhere again, the app drawer
  search included (only the lock screen has its own). An open menu shortens
  to end where the keyboard starts, so the keyboard is never hidden.
- Auto-hide: choose how long the bars stay (3 / 6 / 10 / 20 s; default 6,
  was 3). Touching a bar starts the wait over.
- Auto-hide: swipe sideways along the bottom edge to switch window groups,
  like on the button bar.
- Custom accent color: the "+" swatch opens a color picker.
- Smoother group swipe and recents: screenshots are GPU textures now (the
  GPU scales them), prepared in the background; the swipe asks sway once
  instead of twice.
- Settings: pages switch with a quick fade instead of a slide.

## 0.3.1

- Build fix: screenshots are read on the worker thread as plain pixel data.

## 0.3.0

- ⌨ on the button bar is hidden whenever a keyboard shows - including the app
  drawer's own keyboard.
- Auto-hide: one setting for both bars (turn it on again once - the two old
  settings are gone). While the bars show they make room, so apps shrink to
  fit instead of being covered, and with a menu open they stay, so the
  drawer's keyboard sits above the button bar.
- Recents: dropping a window beside a whole group now shows a line along
  that edge of the group (all four sides).
- Faster on older CPUs: screenshots are stored uncompressed (nothing to pack
  or unpack), read in the background right after they're taken, and every
  size is kept, so recents and the group swipe open without decoding.
  Lighter shadows.

## 0.2.0

- App drawer search: its own keyboard (same look as the on-screen keyboard)
  floats over the bottom of the drawer, so it can't end up hidden.
- Volume / brightness popup: no more square edges around it.
- Brightness slider follows how bright the screen looks: the bottom end is
  much dimmer.
- Auto-hide bars go away reliably once an app is in front.
- Full-screen apps (videos, games) hide both bars.
- The screen stays on while media plays (browsers, video and music players)
  or an app asks it to (org.freedesktop.ScreenSaver, systemd idle inhibitors).
- Settings: taller drop-down lists, easier to hit with a finger.
- Battery: RustTouch now sleeps until there's something to do (no more
  checking 33 times a second), the clock updates once a minute, status
  every 15 s (not while the screen is off), previews mostly only when you
  leave an app.
- Hold Wi-Fi / Bluetooth: an outline grows in, then "Let go for the list".

## 0.1.0

- First versions: home screen, app drawer, pull-down, recents with window
  groups and split screen (sway), lock screen, settings app.

# Changelog

The version is in `Cargo.toml` and shows in Settings → About.

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

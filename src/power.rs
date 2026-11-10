//! Idle timers (screen off / sleep) via swayidle, and hardware buttons
//! (power, volume, brightness) read straight from their input devices, so
//! we can tell short/long/double power presses apart and repeat volume
//! while a button is held.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;

use crate::shell::Event;
use crate::sys;

/// swayidle runs our "commands" (`echo blank` ...) and their output comes
/// back to us through its stdout. Returns the process so it can be
/// restarted when the timeouts change.
pub fn start_idle(tx: Sender<Event>, screen_off: u32, sleep: u32) -> Option<Child> {
    if screen_off == 0 && sleep == 0 {
        return None;
    }
    if !sys::which("swayidle") {
        eprintln!(
            "rusttouch: swayidle not installed - no screen-off/sleep timers (apt install swayidle)"
        );
        return None;
    }
    let mut args: Vec<String> = Vec::new();
    if screen_off > 0 {
        args.extend([
            "timeout".into(),
            screen_off.to_string(),
            "echo blank".into(),
        ]);
        args.extend(["resume".into(), "echo unblank".into()]);
    }
    if sleep > 0 {
        args.extend(["timeout".into(), sleep.to_string(), "echo sleep".into()]);
    }
    let mut child = Command::new("swayidle")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let out = child.stdout.take()?;
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = tx.send(Event::Idle(line.trim().to_string()));
            crate::shell::wake();
        }
    });
    Some(child)
}

pub fn stop(mut child: Child) {
    let _ = child.kill();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

const EV_KEY: u16 = 1;
pub const KEY_A: u16 = 30;
pub const KEY_MUTE: u16 = 113;
pub const KEY_VOLUMEDOWN: u16 = 114;
pub const KEY_VOLUMEUP: u16 = 115;
pub const KEY_POWER: u16 = 116;
pub const KEY_BRIGHTNESSDOWN: u16 = 224;
pub const KEY_BRIGHTNESSUP: u16 = 225;
const WANTED: [u16; 6] = [
    KEY_MUTE,
    KEY_VOLUMEDOWN,
    KEY_VOLUMEUP,
    KEY_POWER,
    KEY_BRIGHTNESSDOWN,
    KEY_BRIGHTNESSUP,
];

/// Parse /sys/class/input/eventN/device/capabilities/key (hex words,
/// most significant first, one bit per key code).
fn has_key(bitmap: &str, code: u16) -> bool {
    let words: Vec<u64> = bitmap
        .split_whitespace()
        .rev()
        .map(|w| u64::from_str_radix(w, 16).unwrap_or(0))
        .collect();
    let bits = usize::BITS as usize; // kernel uses unsigned long words
    let (w, b) = (usize::from(code) / bits, usize::from(code) % bits);
    words.get(w).map(|x| x >> b & 1 == 1).unwrap_or(false)
}

/// Hardware buttons: power, volume, mute, brightness. Watches every input
/// device that has one of those keys but is NOT a full keyboard (we never
/// read keyboards). Needs the udev rule from install.sh, which gives the
/// logged-in user read access to exactly those button devices.
/// Returns (devices watched, any of them has a power key).
pub fn start_buttons(tx: Sender<Event>) -> (usize, bool) {
    let mut n = 0;
    let mut power = false;
    let Ok(dir) = fs::read_dir("/sys/class/input") else {
        return (0, false);
    };
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with("event") {
            continue;
        }
        let caps = fs::read_to_string(e.path().join("device/capabilities/key")).unwrap_or_default();
        if has_key(&caps, KEY_A) || !WANTED.iter().any(|k| has_key(&caps, *k)) {
            continue;
        }
        let label = fs::read_to_string(e.path().join("device/name")).unwrap_or_default();
        let Ok(mut f) = File::open(format!("/dev/input/{name}")) else {
            eprintln!(
                "rusttouch: can't read /dev/input/{name} ({}) - run install.sh and reboot",
                label.trim()
            );
            continue;
        };
        eprintln!(
            "rusttouch: watching buttons on /dev/input/{name} ({})",
            label.trim()
        );
        n += 1;
        power |= has_key(&caps, KEY_POWER);
        let tx = tx.clone();
        std::thread::spawn(move || {
            // struct input_event on 64-bit: timeval (16) + type (2) + code (2) + value (4)
            let mut buf = [0u8; 24];
            while f.read_exact(&mut buf).is_ok() {
                let ty = u16::from_ne_bytes([buf[16], buf[17]]);
                let code = u16::from_ne_bytes([buf[18], buf[19]]);
                let value = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);
                // value: 1 = press, 0 = release, 2 = kernel autorepeat (ignored;
                // we do our own repeat so holding works the same everywhere)
                if ty == EV_KEY && value != 2 && WANTED.contains(&code) {
                    let _ = tx.send(Event::Key(code, value == 1));
                    crate::shell::wake();
                }
            }
        });
    }
    (n, power)
}

const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
/// a finger on a touchpad (a quick touch is a tap = a click)
const BTN_TOUCH: u16 = 0x14a;

/// Is input device eventN (with this "13:68" number) a mouse or touchpad -
/// not a touch screen or pen? (from udev's own notes on it)
fn is_mouse(dev: &str) -> bool {
    let Ok(props) = fs::read_to_string(format!("/run/udev/data/c{dev}")) else {
        return false;
    };
    let has = |k: &str| props.lines().any(|l| l == format!("E:{k}=1"));
    (has("ID_INPUT_MOUSE") || has("ID_INPUT_TOUCHPAD"))
        && !has("ID_INPUT_TOUCHSCREEN")
        && !has("ID_INPUT_TABLET")
}

/// Watch mice and touchpads for button presses (so a mouse click can put the
/// on-screen keyboard away). Mice plugged in later are found too (looked for
/// every few seconds). Only button presses are passed on, nothing else.
pub fn start_mice(tx: Sender<Event>) {
    std::thread::spawn(move || {
        let watching = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::<
            String,
        >::new()));
        loop {
            if let Ok(dir) = fs::read_dir("/sys/class/input") {
                for e in dir.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if !name.starts_with("event") {
                        continue;
                    }
                    if watching.lock().map(|w| w.contains(&name)).unwrap_or(true) {
                        continue;
                    }
                    let dev = fs::read_to_string(e.path().join("dev")).unwrap_or_default();
                    if !is_mouse(dev.trim()) {
                        continue;
                    }
                    let Ok(mut f) = File::open(format!("/dev/input/{name}")) else {
                        continue; // (no permission: install.sh adds it)
                    };
                    let label =
                        fs::read_to_string(e.path().join("device/name")).unwrap_or_default();
                    eprintln!(
                        "rusttouch: watching mouse clicks on /dev/input/{name} ({})",
                        label.trim()
                    );
                    if let Ok(mut w) = watching.lock() {
                        w.insert(name.clone());
                    }
                    let (tx, watching) = (tx.clone(), watching.clone());
                    std::thread::spawn(move || {
                        let mut buf = [0u8; 24];
                        let mut touched: Option<std::time::Instant> = None;
                        while f.read_exact(&mut buf).is_ok() {
                            let ty = u16::from_ne_bytes([buf[16], buf[17]]);
                            let code = u16::from_ne_bytes([buf[18], buf[19]]);
                            let value = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);
                            if ty != EV_KEY {
                                continue;
                            }
                            let click = match code {
                                BTN_LEFT | BTN_RIGHT | BTN_MIDDLE => value == 1,
                                // tap-to-click: the touchpad sends no button,
                                // just a finger down and up again quickly
                                BTN_TOUCH if value == 1 => {
                                    touched = Some(std::time::Instant::now());
                                    false
                                }
                                BTN_TOUCH if value == 0 => touched
                                    .take()
                                    .is_some_and(|t| t.elapsed().as_millis() < 250),
                                _ => false,
                            };
                            if click {
                                let _ = tx.send(Event::MouseClick);
                                crate::shell::wake();
                            }
                        }
                        // unplugged
                        if let Ok(mut w) = watching.lock() {
                            w.remove(&name);
                        }
                    });
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
    });
}

/// While we handle the button, tell logind not to (released if rusttouch exits,
/// so logind's own setting - sleep, from install.sh - takes over again).
pub fn inhibit_power_key() -> Option<Child> {
    Command::new("systemd-inhibit")
        .args([
            "--what=handle-power-key",
            "--mode=block",
            "--who=rusttouch",
            "--why=rusttouch handles the power button",
            "cat",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

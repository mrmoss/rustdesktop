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

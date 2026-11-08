//! Background watchers that shell out to standard tools and report back
//! over a channel: screen orientation (iio-sensor-proxy) and suspend (logind).

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;

use crate::shell::Event;
use crate::sys;

/// Run `cmd` line-buffered and feed each stdout line to `f` on a thread.
fn spawn_lines(cmd: &[&str], f: impl FnMut(&str) + Send + 'static) -> bool {
    let child = Command::new("stdbuf")
        .arg("-oL")
        .args(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return false;
    };
    let Some(out) = child.stdout.take() else {
        return false;
    };
    std::thread::spawn(move || {
        let mut f = f;
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            f(&line);
        }
        let _ = child.wait();
    });
    true
}

/// Accelerometer orientation: "normal", "left-up", "right-up", "bottom-up".
/// Needs iio-sensor-proxy (installed by default on Ubuntu) and its
/// `monitor-sensor` tool.
pub fn start_orientation(tx: Sender<Event>) {
    if !sys::which("monitor-sensor") {
        eprintln!(
            "rusttouch: monitor-sensor not found - no auto-rotate (apt install iio-sensor-proxy)"
        );
        return;
    }
    spawn_lines(&["monitor-sensor"], move |line| {
        if let Some(o) = parse_orientation(line) {
            let _ = tx.send(Event::Orientation(o));
            crate::shell::wake();
        }
    });
}

fn parse_orientation(line: &str) -> Option<String> {
    // "=== Has accelerometer (orientation: normal, tilt: vertical)"
    // "    Accelerometer orientation changed: left-up"
    let idx = line.find("orientation")?;
    let rest = &line[idx..];
    let colon = rest.find(':')?;
    let val: String = rest[colon + 1..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    match val.as_str() {
        "normal" | "bottom-up" | "left-up" | "right-up" => Some(val),
        _ => None,
    }
}

/// logind PrepareForSleep(true/false) => lock before suspend.
pub fn start_sleep_monitor(tx: Sender<Event>) {
    let ok = spawn_lines(
        &[
            "gdbus",
            "monitor",
            "--system",
            "--dest",
            "org.freedesktop.login1",
            "--object-path",
            "/org/freedesktop/login1",
        ],
        move |line| {
            if line.contains("PrepareForSleep") {
                let _ = tx.send(Event::Sleep(line.contains("true")));
                crate::shell::wake();
            }
        },
    );
    if !ok {
        eprintln!("rusttouch: could not watch logind (gdbus missing?) - no lock on suspend");
    }
}

/// Ask logind to wait for us before sleeping, so the lock screen is up
/// before the machine suspends. Released by closing the child's stdin.
pub fn inhibit_sleep() -> Option<Child> {
    Command::new("systemd-inhibit")
        .args([
            "--what=sleep",
            "--mode=delay",
            "--who=rusttouch",
            "--why=Show the lock screen before sleeping",
            "cat",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

pub fn release(mut child: Child) {
    drop(child.stdin.take()); // cat sees EOF -> exits -> lock released
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

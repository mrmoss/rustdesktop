//! Suspending by itself after a while with nothing going on (Settings >
//! Auto suspend). swayidle does the waiting; a video playing in a browser
//! (or any app that asks the screen to stay on) keeps it from counting.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

thread_local! {
    static CHILD: std::cell::RefCell<Option<Child>> = const { std::cell::RefCell::new(None) };
}

/// (the swayidle started last time, in case RustTV was restarted)
fn pid_file() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::home().join(".cache"))
        .join("rusttv-idle.pid")
}

fn stop() {
    CHILD.with(|c| {
        if let Some(mut ch) = c.borrow_mut().take() {
            let _ = ch.kill();
            let _ = ch.wait();
        }
    });
    // one left from an earlier RustTV
    if let Ok(pid) = std::fs::read_to_string(pid_file()) {
        let pid = pid.trim();
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if comm.trim() == "swayidle" {
            let _ = Command::new("kill").arg(pid).status();
        }
    }
    let _ = std::fs::remove_file(pid_file());
}

/// Suspend after `minutes` with nothing going on (0: never). False when it
/// can't (swayidle isn't installed).
pub fn apply(minutes: u32) -> bool {
    stop();
    if minutes == 0 {
        return true;
    }
    if crate::launch::find("swayidle").is_none() {
        eprintln!("rusttv: auto suspend needs swayidle (sudo apt install swayidle)");
        return false;
    }
    let secs = (minutes as u64) * 60;
    let child = Command::new("swayidle")
        .args(["-w", "timeout", &secs.to_string(), "systemctl suspend"])
        .stdin(Stdio::null())
        .spawn();
    match child {
        Ok(c) => {
            let _ = std::fs::write(pid_file(), c.id().to_string());
            CHILD.with(|x| *x.borrow_mut() = Some(c));
            true
        }
        Err(e) => {
            eprintln!("rusttv: couldn't start swayidle: {e}");
            false
        }
    }
}

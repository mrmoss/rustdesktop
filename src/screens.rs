//! Extra monitors simply mirror the main screen (wl-mirror). For a desktop
//! with separate screens, use GNOME.

use std::cell::RefCell;
use std::collections::HashMap;
use std::process::Child;

use gtk::prelude::*;

use crate::shell::{self, Shell};
use crate::sys;

thread_local! {
    static MIRRORS: RefCell<HashMap<String, Child>> = RefCell::new(HashMap::new());
}

/// app id of wl-mirror - never treated as an app window
pub const MIRROR_APP: &str = "at.yrlf.wl_mirror";

pub fn all_outputs() -> Vec<String> {
    shell::monitors()
        .iter()
        .filter_map(|m| m.connector().map(|c| c.to_string()))
        .collect()
}

/// Monitors other than the main one.
pub fn other_outputs() -> Vec<String> {
    let main = shell::shell_output_name();
    all_outputs()
        .into_iter()
        .filter(|o| Some(o) != main.as_ref())
        .collect()
}

/// Start/stop mirrors so every extra monitor shows the main screen. Call after
/// monitors change or the main screen moves.
pub fn refresh(shell: &Shell) {
    if !shell.layered {
        return;
    }
    let others = other_outputs();
    let main = shell::shell_output_name();
    MIRRORS.with(|m| {
        m.borrow_mut().retain(|name, child| {
            let keep = others.contains(name) && matches!(child.try_wait(), Ok(None));
            if !keep {
                let _ = child.kill();
                let _ = child.wait();
            }
            keep
        });
    });
    let Some(main) = main else { return };
    for o in &others {
        if MIRRORS.with(|m| m.borrow().contains_key(o)) {
            continue;
        }
        if !sys::which("wl-mirror") {
            eprintln!("rusttouch: mirroring needs wl-mirror (sudo apt install wl-mirror)");
            return;
        }
        eprintln!("rusttouch: mirroring {main} on {o}");
        if let Some(c) = sys::spawn(
            "wl-mirror",
            &["--fullscreen-output", o.as_str(), main.as_str()],
        ) {
            MIRRORS.with(|m| m.borrow_mut().insert(o.clone(), c));
        }
    }
}

/// Stop mirroring (shell exit).
pub fn stop() {
    MIRRORS.with(|m| {
        for (_, mut c) in m.borrow_mut().drain() {
            let _ = c.kill();
        }
    });
}

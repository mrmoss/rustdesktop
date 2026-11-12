//! Your remote's buttons. Remotes send ordinary keys; which ones differs
//! from remote to remote, so any button can be given a job here (Settings >
//! Remote buttons). The usual keys always work too (arrows, Enter, Esc...).
//!
//! Jobs inside RustTV (OK, Back, Menu, the arrows) are handled by RustTV.
//! Jobs that work everywhere (Home, close the app, volume) become sway key
//! bindings: set straight away, and kept in ~/.config/rusttv/remote-keys.conf
//! for the next login (rusttv-session includes it).

use crate::config::{Config, RemoteKey};
use crate::sway;
use gtk::gdk;
use gtk::glib::translate::IntoGlib;
use std::path::PathBuf;

/// (job, what it's called, works in every app - not just in RustTV)
pub const ACTIONS: &[(&str, &str, bool)] = &[
    ("ok", "OK (open)", false),
    ("back", "Back", false),
    ("menu", "Options (like holding OK)", false),
    ("up", "Up", false),
    ("down", "Down", false),
    ("left", "Left", false),
    ("right", "Right", false),
    ("home", "Home (back to RustTV from any app)", true),
    ("close", "Close the app on screen", true),
    ("keyboard", "Show the keyboard (in a text box)", true),
    ("volume-up", "Volume up", true),
    ("volume-down", "Volume down", true),
    ("mute", "Mute", true),
    ("play-pause", "Play / pause", true),
    ("seek-back", "Back 10 seconds", true),
    ("seek-forward", "Forward 10 seconds", true),
    ("previous", "Previous", true),
    ("next", "Next", true),
    ("stop", "Stop", true),
];

/// Jobs a button can have in one app (Edit > Remote buttons, on a
/// launcher): the media ones, sending the app a key (the app's own
/// shortcut), and the everywhere ones.
pub const APP_ACTIONS: &[(&str, &str)] = &[
    ("play-pause", "Play / pause"),
    ("seek-back", "Back 10 seconds"),
    ("seek-forward", "Forward 10 seconds"),
    ("previous", "Previous"),
    ("next", "Next"),
    ("stop", "Stop"),
    ("key:space", "Send Space"),
    ("key:left", "Send Left arrow"),
    ("key:right", "Send Right arrow"),
    ("key:up", "Send Up arrow"),
    ("key:down", "Send Down arrow"),
    ("key:enter", "Send Enter"),
    ("key:esc", "Send Esc"),
    ("key:f", "Send F (full screen)"),
    ("key:m", "Send M (mute)"),
    ("key:k", "Send K (pause)"),
    ("key:j", "Send J (back)"),
    ("key:l", "Send L (forward)"),
    ("key:pageup", "Send Page Up"),
    ("key:pagedown", "Send Page Down"),
    ("home", "Home"),
    ("close", "Close the app"),
    ("keyboard", "Show the keyboard"),
    ("volume-up", "Volume up"),
    ("volume-down", "Volume down"),
    ("mute", "Mute"),
];

/// "key:space" -> the key's evdev code (what's sent to the app).
pub fn send_code(action: &str) -> Option<u32> {
    Some(match action.strip_prefix("key:")? {
        "space" => 57,
        "left" => 105,
        "right" => 106,
        "up" => 103,
        "down" => 108,
        "enter" => 28,
        "esc" => 1,
        "f" => 33,
        "m" => 50,
        "k" => 37,
        "j" => 36,
        "l" => 38,
        "pageup" => 104,
        "pagedown" => 109,
        _ => return None,
    })
}

pub fn app_label(action: &str) -> &'static str {
    APP_ACTIONS
        .iter()
        .find(|a| a.0 == action)
        .map(|a| a.1)
        .unwrap_or("")
}

/// What the built-in keys are, for the settings page.
pub fn built_in(action: &str) -> &'static str {
    match action {
        "ok" => "Enter, Select",
        "back" => "Esc, Backspace, Back",
        "menu" => "Menu",
        "up" | "down" | "left" | "right" => "the arrow keys",
        "home" => "Home, the Windows key",
        "close" => "Alt+F4",
        "keyboard" => "(shows by itself in a text box)",
        "volume-up" | "volume-down" | "mute" => "the volume keys",
        "play-pause" | "previous" | "next" | "stop" => "the media keys (to the app on screen)",
        _ => "",
    }
}

pub fn label(action: &str) -> &'static str {
    ACTIONS
        .iter()
        .find(|a| a.0 == action)
        .map(|a| a.1)
        .unwrap_or("")
}

fn everywhere(action: &str) -> bool {
    ACTIONS.iter().any(|a| a.0 == action && a.2)
}

/// A key's X name, as sway knows it ("XF86HomePage", "Return", "a").
/// (GTK leaves the "XF86" off the media keys' names.)
pub fn key_name(key: gdk::Key) -> Option<String> {
    let name = key.name()?.to_string();
    let v = key.into_glib();
    let name = if (0x1008_FF00..=0x1008_FFFF).contains(&v) && !name.starts_with("XF86") {
        format!("XF86{name}")
    } else {
        name
    };
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
        .then_some(name)
}

/// The job your remote's button `key` has in RustTV (not the built-in keys).
pub fn action_for(cfg: &Config, key: gdk::Key) -> Option<String> {
    let v = key.into_glib();
    cfg.keys
        .iter()
        .find(|k| k.keyval == v)
        .map(|k| k.action.clone())
}

fn file() -> PathBuf {
    crate::config::dir().join("remote-keys.conf")
}

fn exe() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .filter(|p| !p.contains(['\'', '"', ' ', ';']))
        .unwrap_or_else(|| "/usr/local/bin/rusttv".into())
}

fn binding(k: &RemoteKey) -> String {
    format!(
        "bindsym --no-warn {} exec {} --do {}",
        k.name,
        exe(),
        k.action
    )
}

/// Make sway's key bindings match the settings (`old`: what they were).
pub fn apply(old: &[RemoteKey], new: &[RemoteKey]) {
    let mut text = String::from(
        "# Written by RustTV (Settings > Remote buttons) - changes here are replaced.\n",
    );
    for k in new.iter().filter(|k| everywhere(&k.action)) {
        text.push_str(&binding(k));
        text.push('\n');
    }
    let _ = std::fs::create_dir_all(crate::config::dir());
    let _ = std::fs::write(file(), text);
    if !sway::available() {
        return;
    }
    for k in old.iter().filter(|k| everywhere(&k.action)) {
        if !new.contains(k) {
            sway::run(&format!("unbindsym {}", k.name));
        }
    }
    for k in new.iter().filter(|k| everywhere(&k.action)) {
        sway::run(&binding(k));
    }
}

/// While a button is being learned, sway lets every key through to RustTV
/// (a sway mode with no key bindings: "rusttv-keys", in its config).
pub fn learning(on: bool) {
    if sway::available() {
        sway::run(if on {
            "mode rusttv-keys"
        } else {
            "mode default"
        });
    }
}

thread_local! {
    /// the app buttons bound now (for the app on screen)
    static APP_BOUND: std::cell::RefCell<Vec<RemoteKey>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The app on screen changed: its own buttons are bound while it shows
/// (`--do app-key NAME`), and the previous app's are given back (to their
/// everywhere job, if they have one, or to the apps).
pub fn app_on_screen(keys: &[RemoteKey], global: &[RemoteKey]) {
    if !sway::available() {
        return;
    }
    let old = APP_BOUND.with(|b| b.borrow().clone());
    if old == keys {
        return;
    }
    for k in &old {
        if keys.iter().any(|n| n.name == k.name) {
            continue;
        }
        match global
            .iter()
            .find(|g| g.name == k.name && everywhere(&g.action))
        {
            Some(g) => {
                sway::run(&binding(g));
            }
            None => {
                sway::run(&format!("unbindsym {}", k.name));
            }
        }
    }
    for k in keys {
        sway::run(&format!(
            "bindsym --no-warn {} exec {} --do app-key {}",
            k.name,
            exe(),
            k.name
        ));
    }
    APP_BOUND.with(|b| *b.borrow_mut() = keys.to_vec());
}

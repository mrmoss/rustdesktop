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
    ("volume-up", "Volume up", true),
    ("volume-down", "Volume down", true),
    ("mute", "Mute", true),
];

/// What the built-in keys are, for the settings page.
pub fn built_in(action: &str) -> &'static str {
    match action {
        "ok" => "Enter, Select",
        "back" => "Esc, Backspace, Back",
        "menu" => "Menu",
        "up" | "down" | "left" | "right" => "the arrow keys",
        "home" => "Home, the Windows key",
        "close" => "Alt+F4",
        "volume-up" | "volume-down" | "mute" => "the volume keys",
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

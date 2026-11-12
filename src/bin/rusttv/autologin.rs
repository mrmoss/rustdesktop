//! Logging in by itself, straight into RustTV (no password at start-up -
//! like a TV). The login screen's own settings say who logs in by itself;
//! changing them needs the system's permission, which the small helper
//! /usr/local/libexec/rusttv-autologin has (/usr/libexec from the .deb) (install-rusttv.sh puts it there,
//! with a polkit rule that lets the person at the TV turn it on or off for
//! *their own* account only).

use std::process::Command;

/// Where the helper is: /usr/local (install-rusttv.sh) or /usr (the .deb).
pub fn helper_path() -> &'static str {
    const LOCAL: &str = "/usr/local/libexec/rusttv-autologin";
    if std::path::Path::new(LOCAL).exists() {
        LOCAL
    } else {
        "/usr/libexec/rusttv-autologin"
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// you log in by yourself
    On,
    Off,
    /// someone else does
    OtherUser(String),
    /// no login screen RustTV knows how to set up (GDM, LightDM, SDDM)
    Unsupported,
}

pub fn user() -> String {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .or_else(|| {
            Command::new("id")
                .arg("-un")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .unwrap_or_default()
}

/// key = value in [section] of an ini-style file
fn ini(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    let mut found = None;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            inside = l.eq_ignore_ascii_case(&format!("[{section}]"));
            continue;
        }
        if !inside || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            if k.trim().eq_ignore_ascii_case(key) {
                found = Some(v.trim().to_string());
            }
        }
    }
    found
}

fn read(p: &str) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

pub fn state() -> State {
    let me = user();
    let who = |u: Option<String>| match u {
        Some(u) if u == me => State::On,
        Some(u) if !u.is_empty() => State::OtherUser(u),
        _ => State::Off,
    };
    // GDM (Ubuntu)
    for p in ["/etc/gdm3/custom.conf", "/etc/gdm/custom.conf"] {
        if let Some(t) = read(p) {
            let on = ini(&t, "daemon", "AutomaticLoginEnable")
                .is_some_and(|v| v.eq_ignore_ascii_case("true") || v == "1");
            return if on {
                who(ini(&t, "daemon", "AutomaticLogin"))
            } else {
                State::Off
            };
        }
    }
    // LightDM
    if std::path::Path::new("/etc/lightdm").is_dir() {
        let t = read("/etc/lightdm/lightdm.conf.d/50-rusttv-autologin.conf").unwrap_or_default();
        return who(ini(&t, "Seat:*", "autologin-user"));
    }
    // SDDM
    if std::path::Path::new("/etc/sddm.conf.d").is_dir()
        || std::path::Path::new("/etc/sddm.conf").exists()
    {
        let t = read("/etc/sddm.conf.d/50-rusttv-autologin.conf").unwrap_or_default();
        return who(ini(&t, "Autologin", "User"));
    }
    State::Unsupported
}

/// Turn it on or off (asks the helper; blocking - run it off the main
/// thread). Err: why it didn't work.
pub fn set(on: bool) -> Result<(), String> {
    if !std::path::Path::new(helper_path()).exists() {
        return Err("the auto-login helper isn't installed (run install-rusttv.sh)".into());
    }
    let out = Command::new("pkexec")
        .arg(helper_path())
        .arg(if on { "enable" } else { "disable" })
        .output()
        .map_err(|e| format!("couldn't ask for permission: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if msg.is_empty() {
            "not allowed".into()
        } else {
            msg
        })
    }
}

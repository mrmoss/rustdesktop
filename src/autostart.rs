//! XDG autostart: the same ~/.config/autostart/*.desktop entries GNOME
//! starts at login (shared by rusttouch and rusttouch-settings).
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

const GROUP: &str = "Desktop Entry";
const ENABLED: &str = "X-GNOME-Autostart-enabled";

#[derive(Clone, Debug)]
pub struct Entry {
    /// file name, e.g. "rustsync.desktop"
    pub id: String,
    pub name: String,
    pub exec: String,
    /// the file in effect (user copy wins over the system one)
    pub path: PathBuf,
    pub user: bool,
    pub system: bool,
    pub enabled: bool,
}

pub fn user_dir() -> PathBuf {
    glib::user_config_dir().join("autostart")
}

fn system_dirs() -> Vec<PathBuf> {
    glib::system_config_dirs()
        .into_iter()
        .map(|d| d.join("autostart"))
        .collect()
}

fn desktop_files(dir: &PathBuf) -> Vec<(String, PathBuf)> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            id.ends_with(".desktop").then(|| (id, e.path()))
        })
        .collect()
}

fn load(path: &PathBuf) -> Option<glib::KeyFile> {
    let kf = glib::KeyFile::new();
    kf.load_from_file(
        path,
        glib::KeyFileFlags::KEEP_COMMENTS | glib::KeyFileFlags::KEEP_TRANSLATIONS,
    )
    .ok()?;
    Some(kf)
}

/// Entries for this session. `all = false` skips ones meant only for other
/// desktops (OnlyShowIn/NotShowIn), like GNOME's own helpers.
pub fn list(all: bool) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let user: Vec<(String, PathBuf)> = desktop_files(&user_dir());
    let mut system: Vec<(String, PathBuf)> = Vec::new();
    for d in system_dirs().iter().rev() {
        for (id, p) in desktop_files(d) {
            system.retain(|(i, _)| *i != id);
            system.push((id, p));
        }
    }
    let mut ids: Vec<String> = user
        .iter()
        .chain(system.iter())
        .map(|(i, _)| i.clone())
        .collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        let u = user.iter().find(|(i, _)| *i == id).map(|(_, p)| p.clone());
        let s = system
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, p)| p.clone());
        let Some(path) = u.clone().or(s.clone()) else {
            continue;
        };
        let Some(kf) = load(&path) else { continue };
        // GNOME starts these through systemd units instead
        if kf
            .boolean(GROUP, "X-GNOME-HiddenUnderSystemd")
            .unwrap_or(false)
        {
            continue;
        }
        let applies = gio::DesktopAppInfo::from_filename(&path)
            .map(|i| i.shows_in(None))
            .unwrap_or(false);
        if !all && !applies {
            continue;
        }
        let hidden = kf.boolean(GROUP, "Hidden").unwrap_or(false);
        let enabled = !hidden && kf.boolean(GROUP, ENABLED).unwrap_or(true);
        out.push(Entry {
            name: kf
                .locale_string(GROUP, "Name", None)
                .map(|s| s.to_string())
                .unwrap_or_else(|_| id.trim_end_matches(".desktop").to_string()),
            exec: kf
                .string(GROUP, "Exec")
                .map(|s| s.to_string())
                .unwrap_or_default(),
            id,
            path,
            user: u.is_some(),
            system: s.is_some(),
            enabled,
        });
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

/// Entries RustTouch picked (autostart_mode = chosen).
pub fn chosen(cfg: &crate::config::Config) -> Vec<String> {
    cfg.get("autostart_apps")
        .split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect()
}

/// Will RustTouch start this entry?
pub fn starts_in_rusttouch(cfg: &crate::config::Config, e: &Entry) -> bool {
    if cfg.get("autostart_mode") == "chosen" {
        chosen(cfg).contains(&e.id)
    } else {
        e.enabled
    }
}

/// Launch the login items (RustTouch does this once at login): everything
/// GNOME would start, or only the chosen ones.
pub fn start_all(ctx: Option<&gio::AppLaunchContext>) {
    let cfg = crate::config::Config::load();
    for e in list(false)
        .into_iter()
        .filter(|e| starts_in_rusttouch(&cfg, e))
    {
        match gio::DesktopAppInfo::from_filename(&e.path) {
            Some(info) => {
                if let Err(err) = info.launch(&[], ctx) {
                    eprintln!("rusttouch: autostart {} failed: {err}", e.id);
                } else {
                    eprintln!("rusttouch: autostarted {}", e.id);
                }
            }
            None => eprintln!("rusttouch: autostart {}: not runnable (TryExec?)", e.id),
        }
    }
}

/// Turn an entry on/off. System entries get a user copy (that's how GNOME's
/// "Startup Applications" does it too).
pub fn set_enabled(e: &Entry, on: bool) -> Result<(), String> {
    let kf = load(&e.path).ok_or("can't read entry")?;
    kf.set_boolean(GROUP, ENABLED, on);
    if on {
        let _ = kf.remove_key(GROUP, "Hidden");
    }
    fs::create_dir_all(user_dir()).map_err(|x| x.to_string())?;
    kf.save_to_file(user_dir().join(&e.id))
        .map_err(|x| x.to_string())
}

/// Start an installed app at login (copies its .desktop file).
pub fn add_app(app_id: &str) -> Result<(), String> {
    let info = gio::DesktopAppInfo::new(app_id).ok_or("app not found")?;
    let src = info.filename().ok_or("app has no file")?;
    let kf = load(&src).ok_or("can't read app")?;
    kf.set_boolean(GROUP, ENABLED, true);
    let _ = kf.remove_key(GROUP, "Hidden");
    fs::create_dir_all(user_dir()).map_err(|x| x.to_string())?;
    kf.save_to_file(user_dir().join(app_id))
        .map_err(|x| x.to_string())
}

/// Start a command at login, e.g. "rustsync service".
pub fn add_command(name: &str, command: &str) -> Result<(), String> {
    let name = name.trim();
    let command = command.trim();
    if command.is_empty() {
        return Err("enter a command".into());
    }
    let name = if name.is_empty() {
        command.split_whitespace().next().unwrap_or("command")
    } else {
        name
    };
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let kf = glib::KeyFile::new();
    kf.set_string(GROUP, "Type", "Application");
    kf.set_string(GROUP, "Name", name);
    kf.set_string(GROUP, "Exec", command);
    kf.set_string(GROUP, "Comment", "Added in rusttouch Settings");
    kf.set_boolean(GROUP, "NoDisplay", true);
    kf.set_boolean(GROUP, ENABLED, true);
    fs::create_dir_all(user_dir()).map_err(|x| x.to_string())?;
    kf.save_to_file(user_dir().join(format!("rusttouch-{slug}.desktop")))
        .map_err(|x| x.to_string())
}

/// Remove a user entry. (System entries can only be switched off.)
pub fn remove(e: &Entry) -> Result<(), String> {
    if !e.user {
        return Err("built-in entry - switch it off instead".into());
    }
    fs::remove_file(user_dir().join(&e.id)).map_err(|x| x.to_string())
}

//! Screens: RustTV is on the main one; every other screen shows a copy of
//! it (wl-mirror), like RustTouch. Its resolution can be picked.

use crate::config::Config;
use crate::sway;
use std::cell::RefCell;
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};

/// wl-mirror's window name - never one of "your" apps
pub const MIRROR_APP: &str = "at.yrlf.wl_mirror";

#[derive(Clone, Debug)]
pub struct Mode {
    pub width: i64,
    pub height: i64,
    /// mHz
    pub refresh: i64,
}

impl Mode {
    /// "1920x1080@60.000Hz" (sway's way of writing it)
    pub fn sway(&self) -> String {
        format!(
            "{}x{}@{:.3}Hz",
            self.width,
            self.height,
            self.refresh as f64 / 1000.0
        )
    }

    pub fn label(&self) -> String {
        format!(
            "{} × {}  {} Hz",
            self.width,
            self.height,
            (self.refresh as f64 / 1000.0).round()
        )
    }
}

#[derive(Clone, Debug)]
pub struct Output {
    pub name: String,
    /// "Samsung Q60" and the like
    pub label: String,
    pub active: bool,
    pub current: Option<Mode>,
    pub modes: Vec<Mode>,
}

fn mode_of(v: &serde_json::Value) -> Option<Mode> {
    Some(Mode {
        width: v["width"].as_i64()?,
        height: v["height"].as_i64()?,
        refresh: v["refresh"].as_i64().unwrap_or(60000),
    })
}

pub fn list() -> Vec<Output> {
    sway::outputs_json()
        .iter()
        .filter_map(|o| {
            let name = o["name"].as_str()?.to_string();
            let make = o["make"].as_str().unwrap_or("");
            let model = o["model"].as_str().unwrap_or("");
            let label = format!("{make} {model}").trim().to_string();
            let mut modes: Vec<Mode> = o["modes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(mode_of)
                .collect();
            modes.sort_by(|a, b| {
                (b.width * b.height, b.refresh).cmp(&(a.width * a.height, a.refresh))
            });
            modes.dedup_by(|a, b| {
                a.width == b.width && a.height == b.height && a.refresh == b.refresh
            });
            Some(Output {
                label: if label.is_empty() || label == "Unknown Unknown" {
                    name.clone()
                } else {
                    format!("{label} ({name})")
                },
                active: o["active"].as_bool().unwrap_or(false),
                current: o.get("current_mode").and_then(mode_of),
                modes,
                name,
            })
        })
        .collect()
}

/// A laptop's or tablet's own screen.
pub fn is_builtin(name: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|p| name.starts_with(p))
}

/// The screen RustTV is on: the one picked in Settings if it's there, else
/// the first one that isn't a laptop's own screen (a TV box on a laptop
/// shows on the TV), else the first.
pub fn main(cfg: &Config, outs: &[Output]) -> Option<String> {
    let on: Vec<&Output> = outs.iter().filter(|o| o.active).collect();
    if let Some(o) = on.iter().find(|o| o.name == cfg.main_output) {
        return Some(o.name.clone());
    }
    on.iter()
        .find(|o| !is_builtin(&o.name))
        .or(on.first())
        .map(|o| o.name.clone())
}

thread_local! {
    static MIRRORS: RefCell<HashMap<String, (String, Child)>> = RefCell::new(HashMap::new());
    /// the last setup put in place: (main, other screens, fit)
    static LAST: RefCell<Option<(String, Vec<String>, String)>> = const { RefCell::new(None) };
}

fn fit_arg(cfg: &Config) -> &'static str {
    match cfg.mirror_fit.as_str() {
        "fill" => "cover",
        "exact" => "exact",
        _ => "fit",
    }
}

/// Put the screens as the settings say: the saved resolutions, RustTV (the
/// home workspace) on the main screen, copies on the others. Cheap when
/// nothing changed.
pub fn refresh(cfg: &Config) {
    if !sway::available() {
        return;
    }
    let outs = list();
    let Some(main) = main(cfg, &outs) else {
        return;
    };
    let others: Vec<String> = outs
        .iter()
        .filter(|o| o.active && o.name != main)
        .map(|o| o.name.clone())
        .collect();
    let fit = fit_arg(cfg).to_string();
    let now = (main.clone(), others.clone(), fit.clone());
    let changed = LAST.with(|l| l.borrow().as_ref() != Some(&now));
    // mirrors that quit (or are for screens gone / the wrong main) go
    MIRRORS.with(|m| {
        m.borrow_mut().retain(|o, (src, child)| {
            let keep = others.contains(o)
                && *src == main
                && !changed
                && matches!(child.try_wait(), Ok(None));
            if !keep {
                let _ = child.kill();
                let _ = child.wait();
            }
            keep
        })
    });
    if changed {
        LAST.with(|l| *l.borrow_mut() = Some(now));
        // RustTV's home (and new apps) on the main screen
        sway::run(&format!(
            "workspace {}; move workspace to output {main}; focus output {main}",
            sway::HOME
        ));
    }
    for o in &others {
        if MIRRORS.with(|m| m.borrow().contains_key(o)) {
            continue;
        }
        if crate::launch::find("wl-mirror").is_none() {
            eprintln!("rusttv: showing a copy on other screens needs wl-mirror (sudo apt install wl-mirror)");
            return;
        }
        let child = Command::new("wl-mirror")
            .args(["--scaling", &fit, "--fullscreen-output", o, &main])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn();
        match child {
            Ok(c) => {
                MIRRORS.with(|m| m.borrow_mut().insert(o.clone(), (main.clone(), c)));
            }
            Err(e) => eprintln!("rusttv: couldn't start wl-mirror: {e}"),
        }
    }
}

/// Use the saved resolutions (at start-up).
pub fn apply_modes(cfg: &Config) {
    if !sway::available() {
        return;
    }
    for (name, mode) in &cfg.resolutions {
        if mode
            .chars()
            .all(|c| c.is_ascii_digit() || "x@.Hz".contains(c))
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            sway::run(&format!("output {name} mode {mode}"));
        }
    }
}

/// Start the copies over (their settings changed).
pub fn restart(cfg: &Config) {
    LAST.with(|l| *l.borrow_mut() = None);
    refresh(cfg);
}

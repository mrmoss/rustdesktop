//! Monitors via wlr-randr (shared by rusttouch and rusttouch-settings).
//! Settings live in rusttouch.conf as `output.<NAME>.enabled|mode|scale|transform|place`
//! plus `shell_output`; rusttouch re-applies them at login and on every hotplug.
#![allow(dead_code)]

use std::process::{Command, Stdio};

use crate::config::Config;

#[derive(Clone, Debug, PartialEq)]
pub struct Mode {
    pub w: i32,
    pub h: i32,
    pub hz: f64,
    pub current: bool,
    pub preferred: bool,
}

impl Mode {
    /// What wlr-randr --mode accepts.
    pub fn arg(&self) -> String {
        format!("{}x{}@{:.3}Hz", self.w, self.h, self.hz)
    }
    pub fn label(&self) -> String {
        format!("{} × {}  ·  {:.0} Hz", self.w, self.h, self.hz)
    }
}

#[derive(Clone, Debug)]
pub struct Output {
    pub name: String,
    pub desc: String,
    pub enabled: bool,
    pub modes: Vec<Mode>,
    pub pos: (i32, i32),
    pub transform: String,
    pub scale: f64,
}

impl Output {
    pub fn current_mode(&self) -> Option<&Mode> {
        self.modes
            .iter()
            .find(|m| m.current)
            .or(self.modes.iter().find(|m| m.preferred))
    }

    /// Size in layout coordinates (after scale and rotation).
    pub fn logical_size(&self) -> (i32, i32) {
        let (w, h) = self
            .current_mode()
            .map(|m| (m.w, m.h))
            .unwrap_or((1920, 1080));
        let s = if self.scale > 0.0 { self.scale } else { 1.0 };
        let (w, h) = ((f64::from(w) / s) as i32, (f64::from(h) / s) as i32);
        if matches!(
            self.transform.as_str(),
            "90" | "270" | "flipped-90" | "flipped-270"
        ) {
            (h, w)
        } else {
            (w, h)
        }
    }
}

pub fn is_builtin(name: &str) -> bool {
    name.starts_with("eDP") || name.starts_with("DSI") || name.starts_with("LVDS")
}

/// Parse plain `wlr-randr` output (works with every wlr-randr version).
pub fn list() -> Vec<Output> {
    let out = Command::new("timeout")
        .args(["3", "wlr-randr"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut outs: Vec<Output> = Vec::new();
    let mut in_modes = false;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            let (name, desc) = line.split_once(' ').unwrap_or((line, ""));
            outs.push(Output {
                name: name.to_string(),
                desc: desc.trim().trim_matches('"').to_string(),
                enabled: true,
                modes: Vec::new(),
                pos: (0, 0),
                transform: "normal".into(),
                scale: 1.0,
            });
            in_modes = false;
            continue;
        }
        let Some(o) = outs.last_mut() else { continue };
        let t = line.trim();
        if let Some(v) = t.strip_prefix("Enabled:") {
            o.enabled = v.trim() == "yes";
            in_modes = false;
        } else if t.starts_with("Modes:") {
            in_modes = true;
        } else if let Some(v) = t.strip_prefix("Position:") {
            let mut it = v.trim().split(',').map(|n| n.trim().parse().unwrap_or(0));
            o.pos = (it.next().unwrap_or(0), it.next().unwrap_or(0));
            in_modes = false;
        } else if let Some(v) = t.strip_prefix("Transform:") {
            o.transform = v.trim().to_string();
            in_modes = false;
        } else if let Some(v) = t.strip_prefix("Scale:") {
            o.scale = v.trim().parse().unwrap_or(1.0);
            in_modes = false;
        } else if in_modes {
            // "1920x1080 px, 60.000000 Hz (preferred, current)"
            let (size, rest) = t.split_once(" px,").unwrap_or((t, ""));
            let Some((w, h)) = size.trim().split_once('x') else {
                continue;
            };
            let hz: f64 = rest
                .trim()
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0.0);
            let m = Mode {
                w: w.parse().unwrap_or(0),
                h: h.parse().unwrap_or(0),
                hz,
                current: rest.contains("current"),
                preferred: rest.contains("preferred"),
            };
            if m.w > 0
                && !o
                    .modes
                    .iter()
                    .any(|x| x.w == m.w && x.h == m.h && (x.hz - m.hz).abs() < 0.01)
            {
                o.modes.push(m);
            } else if m.current {
                if let Some(x) = o
                    .modes
                    .iter_mut()
                    .find(|x| x.w == m.w && x.h == m.h && (x.hz - m.hz).abs() < 0.01)
                {
                    x.current = true;
                }
            }
        }
    }
    outs
}

/// The monitor rusttouch's bars and home live on: `shell_output`, else the
/// built-in panel, else the first one.
pub fn shell_output(cfg: &Config, outs: &[Output]) -> Option<String> {
    let want = cfg.get("shell_output");
    if want != "auto" && outs.iter().any(|o| o.name == want && o.enabled) {
        return Some(want);
    }
    outs.iter()
        .find(|o| is_builtin(&o.name) && o.enabled)
        .or(outs.iter().find(|o| o.enabled))
        .map(|o| o.name.clone())
}

fn key(name: &str, what: &str) -> String {
    format!("output.{name}.{what}")
}

/// Saved value or "" (not saved).
pub fn saved(cfg: &Config, name: &str, what: &str) -> String {
    cfg.get(&key(name, what))
}

pub fn save(name: &str, what: &str, value: &str) {
    crate::config::set(&key(name, what), value);
}

fn wlr_randr(args: &[String]) -> bool {
    Command::new("timeout")
        .arg("5")
        .arg("wlr-randr")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Apply every saved monitor setting, then lay the screens out around the
/// shell monitor. New monitors without settings go to the right of it
/// (extended desktop). `skip_transform_of_builtin`: auto-rotate owns it.
/// Blocking: call from a worker thread.
pub fn apply_saved(cfg: &Config, skip_transform_of_builtin: bool) {
    let outs = list();
    let Some(shell) = shell_output(cfg, &outs) else {
        return;
    };

    for o in &outs {
        let mut args: Vec<String> = vec!["--output".into(), o.name.clone()];
        let enabled = saved(cfg, &o.name, "enabled") != "off" || o.name == shell;
        if !enabled {
            args.push("--off".into());
            wlr_randr(&args);
            continue;
        }
        args.push("--on".into());
        let mode = saved(cfg, &o.name, "mode");
        if !mode.is_empty() {
            args.extend(["--mode".into(), mode]);
        }
        let scale = saved(cfg, &o.name, "scale");
        if !scale.is_empty() {
            args.extend(["--scale".into(), scale]);
        }
        let tf = saved(cfg, &o.name, "transform");
        if !tf.is_empty() && !(skip_transform_of_builtin && is_builtin(&o.name)) {
            args.extend(["--transform".into(), tf]);
        }
        wlr_randr(&args);
    }

    // positions (re-read: modes/scales just changed the sizes)
    let outs = list();
    let Some(main) = outs.iter().find(|o| o.name == shell) else {
        return;
    };
    let (mw, mh) = main.logical_size();
    let mut placed: Vec<(String, i32, i32)> = vec![(main.name.clone(), 0, 0)];
    let (mut right, mut left, mut above, mut below) = (mw, 0, 0, mh);
    for o in outs.iter().filter(|o| o.enabled && o.name != shell) {
        let (w, h) = o.logical_size();
        let place = saved(cfg, &o.name, "place");
        let (x, y) = match place.as_str() {
            "left" => {
                left -= w;
                (left, 0)
            }
            "above" => {
                above -= h;
                (0, above)
            }
            "below" => {
                let y = below;
                below += h;
                (0, y)
            }
            _ => {
                let x = right;
                right += w;
                (x, 0)
            }
        };
        placed.push((o.name.clone(), x, y));
    }
    // layouts start at 0,0
    let min_x = placed.iter().map(|p| p.1).min().unwrap_or(0);
    let min_y = placed.iter().map(|p| p.2).min().unwrap_or(0);
    for (name, x, y) in placed {
        wlr_randr(&[
            "--output".into(),
            name,
            "--pos".into(),
            format!("{},{}", x - min_x, y - min_y),
        ]);
    }
}

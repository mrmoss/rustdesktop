//! Everything that touches the OS lives here. It shells out to standard tools
//! so it's easy to read and swap out (D-Bus would be the "proper" way).
//!
//! Tools used (all optional - missing ones just disable that feature):
//!   nmcli, bluetoothctl, wpctl, busctl (logind brightness), wtype, wlr-randr,
//!   grim, squeekboard / wvkbd, systemctl, loginctl.
//!   X11 fallback: xdotool, xrandr.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

#[derive(Clone, Default, Debug)]
pub struct Status {
    pub wifi_on: Option<bool>,
    pub ssid: Option<String>,
    pub bt_on: Option<bool>,
    /// name of a connected Bluetooth device
    pub bt_connected: Option<String>,
    /// (percent, charging)
    pub battery: Option<(u8, bool)>,
    /// 0..100
    pub volume: Option<f64>,
}

fn is_x11() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_some()
}

pub fn which(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
        .unwrap_or(false)
}

pub fn spawn(cmd: &str, args: &[&str]) -> Option<Child> {
    match Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("rusttouch: could not run `{cmd}`: {e}");
            None
        }
    }
}

/// Fire and forget (reaped on a thread so we don't collect zombies).
pub fn run(cmd: &str, args: &[&str]) {
    if let Some(mut c) = spawn(cmd, args) {
        std::thread::spawn(move || {
            let _ = c.wait();
        });
    }
}

/// Run and capture stdout, with a 2s timeout so a hung tool can't stall us.
fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("timeout")
        .arg("2")
        .arg(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// $XDG_RUNTIME_DIR/rusttouch (tmpfs) for previews, logs, keyboard config.
pub fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let d = base.join("rusttouch");
    let _ = fs::create_dir_all(&d);
    d
}

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|p| p.join("rusttouch"))
}

// ---------------------------------------------------------------- status

/// Blocking - call from a worker thread (see shell::refresh_status).
pub fn read_status() -> Status {
    Status {
        wifi_on: wifi_on(),
        ssid: ssid(),
        bt_on: bt_on(),
        bt_connected: bt_connected(),
        battery: battery(),
        volume: volume(),
    }
}

fn wifi_on() -> Option<bool> {
    let s = output("nmcli", &["radio", "wifi"])?;
    Some(s.trim() == "enabled")
}

fn ssid() -> Option<String> {
    let s = output(
        "nmcli",
        &["-t", "-f", "NAME,TYPE", "connection", "show", "--active"],
    )?;
    s.lines().find_map(|l| {
        let (name, ty) = l.rsplit_once(':')?;
        if ty.contains("wireless") {
            Some(name.replace("\\:", ":"))
        } else {
            None
        }
    })
}

fn bt_on() -> Option<bool> {
    let s = output("bluetoothctl", &["show"])?;
    if s.contains("Powered: yes") {
        Some(true)
    } else if s.contains("Powered: no") {
        Some(false)
    } else {
        None
    }
}

fn read_trim(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// The laptop's own battery: skips pen/mouse/keyboard batteries
/// (scope=Device), which often report 0%.
/// First connected device's name ("devices Connected" needs BlueZ >= 5.65).
fn bt_connected() -> Option<String> {
    let s = output("bluetoothctl", &["devices", "Connected"])?;
    s.lines().find_map(|l| {
        let mut it = l.splitn(3, ' ');
        (it.next()? == "Device").then_some(())?;
        let mac = it.next()?;
        let fallback = it.next().unwrap_or("device").trim().to_string();
        // prefer the nickname
        let alias = output("bluetoothctl", &["info", mac]).and_then(|info| {
            info.lines().find_map(|l| {
                l.trim()
                    .strip_prefix("Alias:")
                    .map(|a| a.trim().to_string())
            })
        });
        Some(alias.filter(|a| !a.is_empty()).unwrap_or(fallback))
    })
}

fn battery() -> Option<(u8, bool)> {
    let mut best: Option<(bool, PathBuf)> = None;
    let mut plugged = false;
    for entry in fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        let p = entry.path();
        let ty = read_trim(&p.join("type")).unwrap_or_default();
        let scope = read_trim(&p.join("scope")).unwrap_or_default();
        if scope == "Device" {
            continue;
        }
        if matches!(ty.as_str(), "Mains" | "USB" | "USB_C" | "USB_PD") {
            if read_trim(&p.join("online")).as_deref() == Some("1") {
                plugged = true;
            }
            continue;
        }
        if ty == "Battery" {
            let is_bat = entry.file_name().to_string_lossy().starts_with("BAT");
            if best.as_ref().map(|(b, _)| !*b && is_bat).unwrap_or(true) {
                best = Some((is_bat, p));
            }
        }
    }
    let (_, p) = best?;
    let pct = read_num(&p.join("capacity")).or_else(|| {
        let ratio = |now: &str, full: &str| {
            let n = read_num(&p.join(now))?;
            let f = read_num(&p.join(full))?;
            (f > 0.0).then(|| n / f * 100.0)
        };
        ratio("energy_now", "energy_full").or_else(|| ratio("charge_now", "charge_full"))
    })?;
    let status = read_trim(&p.join("status")).unwrap_or_default();
    let charging = status == "Charging" || status == "Full" || plugged;
    Some((pct.round().clamp(0.0, 100.0) as u8, charging))
}

fn volume() -> Option<f64> {
    // "Volume: 0.40" or "Volume: 0.40 [MUTED]"
    let s = output("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?;
    let v: f64 = s.split_whitespace().nth(1)?.parse().ok()?;
    Some((v * 100.0).min(100.0))
}

// ---------------------------------------------------------------- brightness
// Read straight from sysfs, write through logind (works for the logged-in
// user without root or the "video" group - brightnessctl often doesn't).

fn backlight() -> Option<(String, PathBuf)> {
    let mut devs: Vec<(u8, String, PathBuf)> = fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .map(|e| {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let ty = fs::read_to_string(p.join("type")).unwrap_or_default();
            // native GPU backlights (intel_backlight, amdgpu_bl0) work best
            let rank = match ty.trim() {
                "raw" => 0,
                "platform" => 1,
                _ => 2,
            };
            (rank, name, p)
        })
        .collect();
    devs.sort();
    devs.into_iter().next().map(|(_, n, p)| (n, p))
}

fn read_num(p: &Path) -> Option<f64> {
    fs::read_to_string(p).ok()?.trim().parse().ok()
}

pub fn brightness() -> Option<f64> {
    let (_, p) = backlight()?;
    let cur = read_num(&p.join("brightness"))?;
    let max = read_num(&p.join("max_brightness"))?;
    if max <= 0.0 {
        return None;
    }
    Some(raw_to_pct(cur / max))
}

/// Our eyes see brightness on a curve: half the backlight looks nearly as
/// bright as full. The slider follows what you see, so its bottom end is
/// really dim (the backlight's lowest step) and the top end is full.
const GAMMA: f64 = 2.2;

fn raw_to_pct(frac: f64) -> f64 {
    frac.clamp(0.0, 1.0).powf(1.0 / GAMMA) * 100.0
}

pub fn set_brightness(pct: f64) {
    let Some((_, p)) = backlight() else { return };
    let Some(max) = read_num(&p.join("max_brightness")) else {
        return;
    };
    let frac = (pct.clamp(0.0, 100.0) / 100.0).powf(GAMMA);
    // never fully black from the slider: the lowest step that still lights up
    let v = (frac * max).round().max(1.0) as u32;
    set_brightness_raw(v);
}

/// Raw backlight value via logind (0 = off; used as a screen-off fallback).
pub fn set_brightness_raw(v: u32) {
    let Some((name, _)) = backlight() else { return };
    run(
        "busctl",
        &[
            "call",
            "org.freedesktop.login1",
            "/org/freedesktop/login1/session/auto",
            "org.freedesktop.login1.Session",
            "SetBrightness",
            "ssu",
            "backlight",
            &name,
            &v.to_string(),
        ],
    );
}

// ---------------------------------------------------------------- wifi / bluetooth lists
// All blocking (scans take seconds) - call from a worker thread.

/// Run with a timeout; (success, stdout, stderr).
fn run_capture(secs: u32, cmd: &str, args: &[&str]) -> (bool, String, String) {
    match Command::new("timeout")
        .arg(secs.to_string())
        .arg(cmd)
        .args(args)
        .stdin(Stdio::null())
        .output()
    {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(e) => (false, String::new(), e.to_string()),
    }
}

/// Split one line of `nmcli -t` output (':' separated, '\:' escaped).
fn nm_fields(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut esc = false;
    for c in line.chars() {
        match (esc, c) {
            (true, _) => {
                out.last_mut().unwrap().push(c);
                esc = false;
            }
            (false, '\\') => esc = true,
            (false, ':') => out.push(String::new()),
            (false, _) => out.last_mut().unwrap().push(c),
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct WifiNet {
    pub ssid: String,
    pub signal: u8,
    pub secure: bool,
    pub active: bool,
    pub known: bool,
}

pub fn wifi_list(rescan: bool) -> Vec<WifiNet> {
    let known: Vec<String> =
        run_capture(5, "nmcli", &["-t", "-f", "NAME,TYPE", "connection", "show"])
            .1
            .lines()
            .map(nm_fields)
            .filter(|f| f.len() >= 2 && f[1].contains("wireless"))
            .map(|f| f[0].clone())
            .collect();
    let list = run_capture(
        20,
        "nmcli",
        &[
            "-t",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
            "--rescan",
            if rescan { "yes" } else { "auto" },
        ],
    )
    .1;
    let mut nets: Vec<WifiNet> = Vec::new();
    for f in list.lines().map(nm_fields) {
        if f.len() < 4 || f[1].is_empty() {
            continue;
        }
        let net = WifiNet {
            ssid: f[1].clone(),
            signal: f[2].parse().unwrap_or(0),
            secure: !f[3].is_empty() && f[3] != "--",
            active: f[0].trim() == "*",
            known: known.contains(&f[1]),
        };
        match nets.iter_mut().find(|n| n.ssid == net.ssid) {
            Some(n) => {
                n.signal = n.signal.max(net.signal);
                n.active |= net.active;
            }
            None => nets.push(net),
        }
    }
    nets.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
    nets
}

/// Connect (saved network, open network, or new one with a password).
pub fn wifi_connect(net: &WifiNet, password: Option<&str>) -> Result<(), String> {
    let (ok, _, err) = if net.known && password.is_none() {
        run_capture(45, "nmcli", &["connection", "up", "id", &net.ssid])
    } else {
        let mut args = vec!["device", "wifi", "connect", net.ssid.as_str()];
        if let Some(p) = password {
            args.extend(["password", p]);
        }
        run_capture(45, "nmcli", &args)
    };
    if ok {
        Ok(())
    } else {
        Err(err
            .lines()
            .last()
            .unwrap_or("failed")
            .replace("Error: ", ""))
    }
}

pub fn wifi_disconnect(net: &WifiNet) {
    let _ = run_capture(15, "nmcli", &["connection", "down", "id", &net.ssid]);
}

pub fn wifi_forget(net: &WifiNet) {
    let _ = run_capture(15, "nmcli", &["connection", "delete", "id", &net.ssid]);
}

#[derive(Clone, Debug)]
pub struct BtDevice {
    pub mac: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
}

pub fn bt_list(scan: bool) -> Vec<BtDevice> {
    if scan {
        let _ = run_capture(10, "bluetoothctl", &["--timeout", "8", "scan", "on"]);
    }
    let out = run_capture(5, "bluetoothctl", &["devices"]).1;
    let mut devs: Vec<BtDevice> = out
        .lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, ' ');
            (it.next()? == "Device").then_some(())?;
            let mac = it.next()?.to_string();
            let name = it.next().unwrap_or("").trim().to_string();
            Some((mac, name))
        })
        .map(|(mac, name)| {
            let info = run_capture(3, "bluetoothctl", &["info", &mac]).1;
            // the nickname (Alias) if one is set
            let name = info
                .lines()
                .find_map(|l| {
                    l.trim()
                        .strip_prefix("Alias:")
                        .map(|a| a.trim().to_string())
                })
                .filter(|a| !a.is_empty())
                .unwrap_or(name);
            BtDevice {
                paired: info.contains("Paired: yes"),
                connected: info.contains("Connected: yes"),
                name: if name.is_empty() || name.replace('-', ":") == mac {
                    mac.clone()
                } else {
                    name
                },
                mac,
            }
        })
        .collect();
    devs.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.paired.cmp(&a.paired))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    devs
}

/// Tap on a device: disconnect if connected, else (pair, trust and) connect.
pub fn bt_activate(dev: &BtDevice) -> Result<(), String> {
    let step = |args: &[&str]| -> Result<(), String> {
        let (ok, out, err) = run_capture(30, "bluetoothctl", args);
        let text = format!("{out}{err}");
        if ok && !text.contains("Failed") && !text.contains("not available") {
            Ok(())
        } else {
            Err(text
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("failed")
                .trim()
                .to_string())
        }
    };
    if dev.connected {
        return step(&["disconnect", &dev.mac]);
    }
    if !dev.paired {
        step(&["pair", &dev.mac])?;
        let _ = step(&["trust", &dev.mac]);
    }
    step(&["connect", &dev.mac])?;
    // headphones/headset: send audio there right away, like a phone
    crate::audio::prefer_bluetooth(&dev.mac);
    Ok(())
}

/// Give a device a nickname (BlueZ "Alias" - shows up everywhere, not just
/// in rusttouch). Empty = back to the device's own name.
pub fn bt_rename(dev: &BtDevice, alias: &str) -> Result<(), String> {
    let tree = run_capture(5, "busctl", &["--system", "--list", "tree", "org.bluez"]).1;
    let tail = format!("dev_{}", dev.mac.replace(':', "_"));
    let Some(path) = tree.lines().map(str::trim).find(|l| l.ends_with(&tail)) else {
        return Err("device not found".into());
    };
    let (ok, _, err) = run_capture(
        5,
        "busctl",
        &[
            "--system",
            "set-property",
            "org.bluez",
            path,
            "org.bluez.Device1",
            "Alias",
            "s",
            alias,
        ],
    );
    if ok {
        Ok(())
    } else {
        Err(err.trim().to_string())
    }
}

pub fn bt_forget(dev: &BtDevice) {
    let _ = run_capture(10, "bluetoothctl", &["remove", &dev.mac]);
}

// ---------------------------------------------------------------- toggles

pub fn set_wifi(on: bool) {
    run("nmcli", &["radio", "wifi", if on { "on" } else { "off" }]);
}

pub fn set_bluetooth(on: bool) {
    run("bluetoothctl", &["power", if on { "on" } else { "off" }]);
}

pub fn toggle_mute() {
    run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]);
}

pub fn set_volume(pct: f64) {
    let v = format!("{:.2}", (pct / 100.0).clamp(0.0, 1.0));
    run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &v]);
}

// ---------------------------------------------------------------- keys / back

/// "alt+Left", "Escape", "ctrl+w", "none"
pub fn send_keys(spec: &str) {
    let spec = spec.trim();
    if spec.is_empty() || spec.eq_ignore_ascii_case("none") {
        return;
    }
    if is_x11() {
        run("xdotool", &["key", spec]);
        return;
    }
    let parts: Vec<&str> = spec.split('+').map(str::trim).collect();
    let Some((key, mods)) = parts.split_last() else {
        return;
    };
    let mut args: Vec<String> = Vec::new();
    for m in mods {
        args.push("-M".into());
        args.push(m.to_lowercase());
    }
    args.push("-k".into());
    args.push((*key).to_string());
    for m in mods.iter().rev() {
        args.push("-m".into());
        args.push(m.to_lowercase());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run("wtype", &refs);
}

/// Alt+Left is "go back" in browsers, file managers and libadwaita apps.
const BACK_DEFAULT: &str = "alt+Left";
/// Apps where Alt+Left would just type junk.
const BACK_NONE: &[&str] = &[
    "org.gnome.Ptyxis",
    "org.gnome.Console",
    "org.gnome.Terminal",
    "foot",
    "kitty",
    "Alacritty",
    "org.wezfurlong.wezterm",
];

/// Per-app back key. Override in ~/.config/rusttouch/back.conf:
///   * = alt+Left
///   org.gnome.TextEditor = Escape
pub fn back_keys_for(app_id: &str) -> String {
    let mut default = BACK_DEFAULT.to_string();
    let mut specific = None;
    if BACK_NONE.iter().any(|a| a.eq_ignore_ascii_case(app_id)) {
        specific = Some("none".to_string());
    }
    if let Some(dir) = config_dir() {
        if let Ok(s) = fs::read_to_string(dir.join("back.conf")) {
            for line in s.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let (k, v) = (k.trim(), v.trim().to_string());
                    if k == "*" {
                        default = v;
                    } else if k.eq_ignore_ascii_case(app_id) {
                        specific = Some(v);
                    }
                }
            }
        }
    }
    specific.unwrap_or(default)
}

/// Android back: first hide squeekboard if it's up, otherwise "go back".
/// (wvkbd visibility is tracked by the shell itself.)
pub fn back(app_id: Option<String>, squeekboard: bool) {
    std::thread::spawn(move || {
        if squeekboard && osk_visible() {
            osk_set_visible(false);
            return;
        }
        send_keys(&back_keys_for(app_id.as_deref().unwrap_or("")));
    });
}

// ---------------------------------------------------------------- keyboard

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Osk {
    /// RustTouch's own keyboard (kbd.rs) - the default
    RustTouch,
    /// pops up automatically on text fields (input-method protocol)
    Squeekboard,
    /// manual toggle only
    Wvkbd,
    Missing,
}

fn wvkbd_cmd() -> String {
    std::env::var("RUSTTOUCH_OSK").unwrap_or_else(|_| "wvkbd-mobintl".into())
}

// squeekboard only auto-shows when the GNOME a11y "screen keyboard" setting
// is on. We give it a private settings file instead of flipping the real
// setting, which would also turn on GNOME's keyboard in your normal session.
fn squeekboard_conf() -> PathBuf {
    runtime_dir().join("squeekboard-config")
}

pub fn set_osk_auto(on: bool) {
    let dir = squeekboard_conf().join("glib-2.0").join("settings");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(
        dir.join("keyfile"),
        format!("[org/gnome/desktop/a11y/applications]\nscreen-keyboard-enabled={on}\n"),
    );
}

pub fn start_squeekboard(auto: bool) -> Option<Child> {
    set_osk_auto(auto);
    Command::new("squeekboard")
        .env("GSETTINGS_BACKEND", "keyfile")
        .env("XDG_CONFIG_HOME", squeekboard_conf())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// wvkbd's --help text (it lists the options this version understands).
fn wvkbd_help() -> &'static str {
    static HELP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HELP.get_or_init(|| {
        Command::new("timeout")
            .args(["2", &wvkbd_cmd(), "-h"])
            .stdin(Stdio::null())
            .output()
            .map(|o| {
                format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                )
            })
            .unwrap_or_default()
    })
}

fn wvkbd_supports(flag: &str) -> bool {
    wvkbd_help().contains(&format!(" {flag} "))
}

/// wvkbd command-line options for the keyboard settings in rusttouch.conf.
/// Options this wvkbd doesn't know (e.g. -R before 0.15) are left out.
pub fn keyboard_args(cfg: &crate::config::Config) -> Vec<String> {
    let mut a = keyboard_args_basic(cfg);
    if let Ok(px) = cfg.get("keyboard_font_size").parse::<u32>() {
        a.extend(["-fn".into(), format!("Sans {px}")]);
    }
    if let Ok(px) = cfg.get("keyboard_rounding").parse::<u32>() {
        if wvkbd_supports("-R") {
            a.extend(["-R".into(), px.to_string()]);
        } else {
            eprintln!("rusttouch: this wvkbd has no key rounding option (needs 0.15+)");
        }
    }
    if cfg.get("keyboard_style") != "wvkbd" {
        let (bg, key, special, press, text) = crate::config::keyboard_colors(cfg);
        for (flag, color) in [
            ("--bg", &bg),
            ("--fg", &key),
            ("--fg-sp", &special),
            ("--press", &press),
            ("--press-sp", &press),
            ("--text", &text),
            ("--text-sp", &text),
        ] {
            a.extend([flag.to_string(), color.clone()]);
        }
    }
    a
}

/// Just the options every wvkbd version understands.
pub fn keyboard_args_basic(cfg: &crate::config::Config) -> Vec<String> {
    let mut a: Vec<String> = vec!["--hidden".into()];
    if let Ok(h) = cfg.get("keyboard_height").parse::<u32>() {
        a.extend(["-H".into(), h.to_string()]);
    }
    if let Ok(h) = cfg.get("keyboard_height_landscape").parse::<u32>() {
        a.extend(["-L".into(), h.to_string()]);
    }
    a
}

/// Started hidden; shown/hidden with signals (SIGUSR2 = show, SIGUSR1 = hide).
pub fn start_wvkbd(args: &[String]) -> Option<Child> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    spawn(&wvkbd_cmd(), &refs)
}

pub fn wvkbd_show(child: &Child, show: bool) {
    let pid = child.id().to_string();
    run("kill", &[if show { "-USR2" } else { "-USR1" }, &pid]);
}

pub fn squeekboard_show(show: bool) {
    osk_set_visible(show);
}

fn osk_visible() -> bool {
    output(
        "busctl",
        &[
            "--user",
            "get-property",
            "sm.puri.OSK0",
            "/sm/puri/OSK0",
            "sm.puri.OSK0",
            "Visible",
        ],
    )
    .map(|s| s.contains("true"))
    .unwrap_or(false)
}

fn osk_set_visible(on: bool) {
    run(
        "busctl",
        &[
            "--user",
            "call",
            "sm.puri.OSK0",
            "/sm/puri/OSK0",
            "sm.puri.OSK0",
            "SetVisible",
            "b",
            if on { "true" } else { "false" },
        ],
    );
}

// ---------------------------------------------------------------- rotation

/// Built-in panel (eDP/DSI/LVDS), else the first output wlr-randr lists.
fn builtin_output() -> Option<String> {
    let out = output("wlr-randr", &[])?;
    let names: Vec<String> = out
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with(char::is_whitespace))
        .filter_map(|l| l.split_whitespace().next().map(String::from))
        .collect();
    names
        .iter()
        .find(|n| n.starts_with("eDP") || n.starts_with("DSI") || n.starts_with("LVDS"))
        .or(names.first())
        .cloned()
}

/// Map an iio-sensor-proxy orientation to an output transform.
/// If your device rotates the wrong way, set RUSTTOUCH_ROTATE_INVERT=1.
pub fn rotate_to(orientation: &str) {
    let mut tf = match orientation {
        "left-up" => "90",
        "right-up" => "270",
        "bottom-up" => "180",
        _ => "normal",
    };
    if std::env::var_os("RUSTTOUCH_ROTATE_INVERT").is_some() {
        tf = match tf {
            "90" => "270",
            "270" => "90",
            x => x,
        };
    }
    std::thread::spawn(move || {
        if is_x11() {
            let x = match tf {
                "90" => "left",
                "270" => "right",
                "180" => "inverted",
                _ => "normal",
            };
            run("xrandr", &["-o", x]);
            return;
        }
        if let Some(name) = builtin_output() {
            run("wlr-randr", &["--output", &name, "--transform", tf]);
        }
    });
}

// ---------------------------------------------------------------- previews

/// Screenshot `geometry` ("x,y wxh", layout coordinates) at full resolution.
/// Blocking. Err = a short reason to show the user.
pub fn grim(path: &Path, geometry: &str) -> Result<(), String> {
    let out = Command::new("timeout")
        .args(["3", "grim", "-t", "ppm", "-g", geometry])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() && path.exists() => Ok(()),
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
            eprintln!("rusttouch: screenshot for recents failed ({geometry}): {err}");
            Err(if err.is_empty() {
                "Screenshot failed (grim)".to_string()
            } else {
                format!("Screenshot failed:\n{err}")
            })
        }
        Err(e) => {
            eprintln!("rusttouch: can't run grim ({e})");
            Err(format!("Can't run grim: {e}"))
        }
    }
}

// ---------------------------------------------------------------- power

/// rusttouch-settings from PATH, or next to our own binary (dev builds).
pub fn open_settings() {
    let exe = if which("rusttouch-settings") {
        Some(PathBuf::from("rusttouch-settings"))
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("rusttouch-settings")))
            .filter(|p| p.is_file())
    };
    let Some(exe) = exe else {
        eprintln!("rusttouch: rusttouch-settings not found");
        return;
    };
    // GDK_DEBUG/GTK_A11Y are only meant for the shell itself
    match Command::new(exe)
        .env_remove("GDK_DEBUG")
        .env_remove("GTK_A11Y")
        .env_remove("GTK_USE_PORTAL")
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(mut c) => {
            std::thread::spawn(move || {
                let _ = c.wait();
            });
        }
        Err(e) => eprintln!("rusttouch: can't start settings: {e}"),
    }
}

/// Tell GTK/libadwaita apps (via the settings portal) to use dark or light.
pub fn set_color_scheme(dark: bool) {
    run(
        "gsettings",
        &[
            "set",
            "org.gnome.desktop.interface",
            "color-scheme",
            if dark { "prefer-dark" } else { "default" },
        ],
    );
}

pub fn suspend() {
    run("systemctl", &["suspend"]);
}

pub fn reboot() {
    run("systemctl", &["reboot"]);
}

pub fn poweroff() {
    run("systemctl", &["poweroff"]);
}

pub fn logout() {
    if std::env::var_os("LABWC_PID").is_some() {
        run("labwc", &["--exit"]);
    } else if let Ok(id) = std::env::var("XDG_SESSION_ID") {
        run("loginctl", &["terminate-session", &id]);
    } else {
        std::process::exit(0);
    }
}

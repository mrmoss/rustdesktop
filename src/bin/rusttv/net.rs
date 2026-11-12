//! Wi-Fi (NetworkManager), Bluetooth (BlueZ) and sound (WirePlumber), the
//! same way RustTouch does them (much of this is RustTouch's own code). All
//! blocking - call from a worker thread.

use std::process::{Command, Stdio};

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
    /// what nmcli lists under SECURITY, e.g. "WPA2 WPA3"
    pub security: String,
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
            security: f[3].clone(),
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
/// A password is handed to NetworkManager over D-Bus from inside RustTV -
/// never on a command line, where other programs could read it (/proc).
pub fn wifi_connect(net: &WifiNet, password: Option<String>) -> Result<(), String> {
    if let Some(pw) = password {
        let r = crate::nm::connect_with_password(&net.ssid, &net.security, &pw);
        wipe(pw);
        return r;
    }
    if net.known {
        // by network name, Wi-Fi profiles only (`nmcli connection up id`
        // would go by profile name, which a VPN could share)
        match crate::nm::activate_saved(&net.ssid) {
            Err(e) if e == crate::nm::NOT_SAVED => {} // (a VPN etc. of that name)
            r => return r,
        }
    }
    // an open network: nmcli takes the name as a network name here
    let (ok, _, err) = run_capture(
        45,
        "nmcli",
        &["device", "wifi", "connect", net.ssid.as_str()],
    );
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

/// (Both go by the network's own name and only touch Wi-Fi profiles.)
pub fn wifi_disconnect(net: &WifiNet) {
    if let Err(e) = crate::nm::deactivate(&net.ssid) {
        eprintln!("rusttv: Wi-Fi disconnect: {e}");
    }
}

pub fn wifi_forget(net: &WifiNet) {
    if let Err(e) = crate::nm::forget_wifi(&net.ssid) {
        eprintln!("rusttv: Wi-Fi forget: {e}");
    }
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
        // A trusted device may reconnect by itself later. Keyboards, mice and
        // game pads are never trusted automatically: a fake "keyboard" that
        // reconnects on its own could type into the computer.
        if !bt_is_input(&dev.mac) {
            let _ = step(&["trust", &dev.mac]);
        }
    }
    step(&["connect", &dev.mac])?;
    // headphones/headset: send audio there right away, like a phone
    crate::audio::prefer_bluetooth(&dev.mac);
    Ok(())
}

/// Is it an input device (keyboard, mouse, game pad, pen...)? Unknown
/// counts as yes, to be safe.
fn bt_is_input(mac: &str) -> bool {
    let (ok, info, _) = run_capture(10, "bluetoothctl", &["info", mac]);
    if !ok {
        return true;
    }
    let icon = info
        .lines()
        .find_map(|l| l.trim().strip_prefix("Icon:"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    // the device picks its own Icon, so the services it offers count too:
    // classic Bluetooth HID (0x1124) and Bluetooth Low Energy HID (HOGP, 0x1812)
    let info = info.to_ascii_lowercase();
    icon.is_empty()
        || icon.starts_with("input-")
        || info.contains("00001124-0000-1000-8000-00805f9b34fb")
        || info.contains("00001812-0000-1000-8000-00805f9b34fb")
}

/// Give a device a nickname (BlueZ "Alias" - shows up everywhere, not just
/// in RustTV). Empty = back to the device's own name.
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
    let _ = run_capture(
        10,
        "nmcli",
        &["radio", "wifi", if on { "on" } else { "off" }],
    );
}

pub fn set_bluetooth(on: bool) {
    let _ = run_capture(
        10,
        "bluetoothctl",
        &["power", if on { "on" } else { "off" }],
    );
}

/// Wi-Fi switched on? (None: no NetworkManager)
pub fn wifi_on() -> Option<bool> {
    let (ok, out, _) = run_capture(3, "nmcli", &["radio", "wifi"]);
    ok.then(|| out.trim() == "enabled")
}

/// Bluetooth switched on? (None: no Bluetooth)
pub fn bt_on() -> Option<bool> {
    let (_, s, _) = run_capture(3, "bluetoothctl", &["show"]);
    if s.contains("Powered: yes") {
        Some(true)
    } else if s.contains("Powered: no") {
        Some(false)
    } else {
        None
    }
}

/// The connected network's name, if any.
pub fn ssid() -> Option<String> {
    let (_, s, _) = run_capture(
        3,
        "nmcli",
        &["-t", "-f", "NAME,TYPE", "connection", "show", "--active"],
    );
    s.lines().find_map(|l| {
        let f = nm_fields(l);
        (f.len() >= 2 && f[1].contains("wireless")).then(|| f[0].clone())
    })
}

/// Overwrite a password's bytes before it's dropped.
fn wipe(s: String) {
    let mut v = s.into_bytes();
    for b in v.iter_mut() {
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

// ---------------------------------------------------------------- sound

/// The volume (0..150) and whether it's muted.
pub fn volume() -> Option<(u32, bool)> {
    let (_, s, _) = run_capture(2, "wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]);
    let rest = s.trim().strip_prefix("Volume:")?.trim();
    let v: f64 = rest.split_whitespace().next()?.parse().ok()?;
    Some(((v * 100.0).round() as u32, rest.contains("MUTED")))
}

pub fn set_volume(pct: u32) {
    let v = format!("{:.2}", (pct as f64 / 100.0).clamp(0.0, 1.5));
    let _ = run_capture(2, "wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &v]);
    let _ = run_capture(2, "wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "0"]);
}

pub fn set_mute(on: bool) {
    let _ = run_capture(
        2,
        "wpctl",
        &[
            "set-mute",
            "@DEFAULT_AUDIO_SINK@",
            if on { "1" } else { "0" },
        ],
    );
}

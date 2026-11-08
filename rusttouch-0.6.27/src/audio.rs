//! Audio devices through `pactl` (works with PipeWire's pulse layer, which
//! Ubuntu uses). Blocking - call from worker threads.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

#[derive(Clone, Debug)]
pub struct Device {
    pub name: String,
    pub desc: String,
    pub is_default: bool,
    pub bluetooth: bool,
}

/// A Bluetooth headset that can switch between "music" (A2DP, no mic) and
/// "headset" (mic, phone-call quality) modes.
#[derive(Clone, Debug)]
pub struct Headset {
    pub card: String,
    pub desc: String,
    /// true = in headset (mic) mode now
    pub mic_mode: bool,
    pub mic_profile: Option<String>,
    pub music_profile: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
    pub headsets: Vec<Headset>,
}

fn pactl(args: &[&str]) -> Option<String> {
    let out = Command::new("timeout")
        .arg("4")
        .arg("pactl")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn json(args: &[&str]) -> Vec<Value> {
    let mut a = vec!["-f", "json"];
    a.extend_from_slice(args);
    pactl(&a)
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn prop(v: &Value, k: &str) -> String {
    v.get("properties").map(|p| s(p, k)).unwrap_or_default()
}

/// The Bluetooth nickname (BlueZ Alias) for a device address, if set.
/// The audio system only knows the device's own name.
fn bt_alias(addr: &str) -> Option<String> {
    if addr.is_empty() {
        return None;
    }
    let out = Command::new("timeout")
        .args(["2", "bluetoothctl", "info", addr])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("Alias:")
                .map(|a| a.trim().to_string())
        })
        .filter(|a| !a.is_empty())
}

/// "bluez_output.AA_BB_CC_DD_EE_FF.1" / "bluez_card.AA_BB..." -> "AA:BB:..."
fn addr_from_name(name: &str) -> String {
    name.split('.').nth(1).unwrap_or("").replace('_', ":")
}

fn devices(kind: &str, default: &str) -> Vec<Device> {
    json(&["list", kind])
        .iter()
        .filter(|d| !s(d, "name").ends_with(".monitor") && prop(d, "device.class") != "monitor")
        .map(|d| {
            let name = s(d, "name");
            let bluetooth = prop(d, "device.bus") == "bluetooth" || name.starts_with("bluez");
            let mut desc = s(d, "description");
            if bluetooth {
                let addr = Some(prop(d, "api.bluez5.address"))
                    .filter(|a| !a.is_empty())
                    .unwrap_or_else(|| addr_from_name(&name));
                if let Some(alias) = bt_alias(&addr) {
                    desc = alias;
                }
            }
            Device {
                is_default: name == default,
                name,
                desc,
                bluetooth,
            }
        })
        .collect()
}

pub fn state() -> State {
    let ds = pactl(&["get-default-sink"]).unwrap_or_default();
    let dsrc = pactl(&["get-default-source"]).unwrap_or_default();
    let mut headsets = Vec::new();
    for c in json(&["list", "cards"]) {
        let card = s(&c, "name");
        if !card.starts_with("bluez_card") {
            continue;
        }
        let active = s(&c, "active_profile");
        let profiles: Vec<String> = c
            .get("profiles")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter(|(_, p)| p.get("available").and_then(Value::as_bool).unwrap_or(true))
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default();
        let pick = |pref: &[&str]| {
            pref.iter()
                .find_map(|p| profiles.iter().find(|x| x.starts_with(p)).cloned())
        };
        let mic_profile = pick(&[
            "headset-head-unit-msbc",
            "headset-head-unit",
            "headset_head_unit",
        ]);
        let music_profile = pick(&["a2dp-sink", "a2dp_sink"]);
        if mic_profile.is_none() {
            continue;
        }
        let addr = Some(prop(&c, "api.bluez5.address"))
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| addr_from_name(&card));
        headsets.push(Headset {
            desc: bt_alias(&addr).unwrap_or_else(|| prop(&c, "device.description")),
            mic_mode: active.starts_with("headset"),
            card,
            mic_profile,
            music_profile,
        });
    }
    State {
        outputs: devices("sinks", ds.trim()),
        inputs: devices("sources", dsrc.trim()),
        headsets,
    }
}

pub fn set_output(name: &str) {
    let _ = pactl(&["set-default-sink", name]);
}

pub fn set_input(name: &str) {
    let _ = pactl(&["set-default-source", name]);
}

/// Switch a headset to mic mode (or back to music mode) and make it the
/// default in and out.
pub fn headset_mode(h: &Headset, mic: bool) {
    let profile = if mic {
        &h.mic_profile
    } else {
        &h.music_profile
    };
    if let Some(p) = profile {
        let _ = pactl(&["set-card-profile", &h.card, p]);
    }
    let mac = h.card.trim_start_matches("bluez_card.").to_string();
    prefer_bluetooth_mac_underscored(&mac, Duration::from_secs(4));
}

/// After a Bluetooth device connects: make its speaker (and mic, if it has
/// one right now) the default, like a phone does. Waits for it to appear.
pub fn prefer_bluetooth(mac: &str) {
    prefer_bluetooth_mac_underscored(&mac.replace(':', "_"), Duration::from_secs(8));
}

fn prefer_bluetooth_mac_underscored(mac_u: &str, wait: Duration) {
    let start = Instant::now();
    while start.elapsed() < wait {
        let st = state();
        let out = st.outputs.iter().find(|d| d.name.contains(mac_u));
        if let Some(o) = out {
            set_output(&o.name);
            if let Some(i) = st.inputs.iter().find(|d| d.name.contains(mac_u)) {
                set_input(&i.name);
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

//! Joining a Wi-Fi network with a password, through NetworkManager's D-Bus
//! interface. The password goes from RustTouch straight to NetworkManager
//! over the system bus; it never appears on a command line (`nmcli ...
//! password X` would show it to every program that can read
//! /proc/<pid>/cmdline while nmcli runs).
//!
//! Blocking - call from a worker thread.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gtk::gio;
use gtk::glib::{self, prelude::ToVariant, variant::ObjectPath, Variant};

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const DEVICE_TYPE_WIFI: u32 = 2;
const ACTIVE_ACTIVATED: u32 = 2;
const ACTIVE_DEACTIVATING: u32 = 3;
const ACTIVE_DEACTIVATED: u32 = 4;

fn call(
    bus: &gio::DBusConnection,
    path: &str,
    iface: &str,
    method: &str,
    args: Option<&Variant>,
    timeout_ms: i32,
) -> Result<Variant, String> {
    bus.call_sync(
        Some(NM),
        path,
        iface,
        method,
        args,
        None,
        gio::DBusCallFlags::NONE,
        timeout_ms,
        None::<&gio::Cancellable>,
    )
    .map_err(|e| {
        // "GDBus.Error:org.freedesktop.NetworkManager...: reason" -> "reason"
        let m = e.message().to_string();
        m.rsplit(": ").next().unwrap_or(&m).to_string()
    })
}

/// A D-Bus property of a NetworkManager object.
fn prop(bus: &gio::DBusConnection, path: &str, iface: &str, name: &str) -> Option<Variant> {
    let r = call(
        bus,
        path,
        "org.freedesktop.DBus.Properties",
        "Get",
        Some(&(iface, name).to_variant()),
        5000,
    )
    .ok()?;
    r.child_value(0).as_variant()
}

/// The first Wi-Fi device.
fn wifi_device(bus: &gio::DBusConnection) -> Result<ObjectPath, String> {
    let r = call(bus, NM_PATH, NM, "GetDevices", None, 5000)?;
    let (devices,): (Vec<ObjectPath>,) = r.get().ok_or("NetworkManager gave no device list")?;
    devices
        .into_iter()
        .find(|d| {
            prop(
                bus,
                d.as_str(),
                "org.freedesktop.NetworkManager.Device",
                "DeviceType",
            )
            .and_then(|v| v.get::<u32>())
                == Some(DEVICE_TYPE_WIFI)
        })
        .ok_or_else(|| "no Wi-Fi device".to_string())
}

/// The security settings for what the network says it uses (nmcli's
/// SECURITY column: "WPA2", "WPA2 WPA3", "WPA3", "WEP", "WPA2 802.1X"...).
fn security(kind: &str, password: &str) -> Result<HashMap<String, Variant>, String> {
    let has = |w: &str| kind.split_whitespace().any(|t| t == w);
    let mut s = HashMap::new();
    if has("802.1X") {
        return Err(
            "this network needs a user name as well (802.1X), which RustTouch can't set up yet"
                .into(),
        );
    }
    if has("WEP") {
        s.insert("key-mgmt".into(), "none".to_variant());
        s.insert("wep-key0".into(), password.to_variant());
        s.insert("wep-key-type".into(), 1u32.to_variant()); // a key or passphrase
    } else {
        // WPA3-only networks use SAE; mixed WPA2/WPA3 ones accept WPA2's PSK
        let sae = has("WPA3") && !has("WPA2") && !has("WPA1");
        s.insert(
            "key-mgmt".into(),
            if sae { "sae" } else { "wpa-psk" }.to_variant(),
        );
        s.insert("psk".into(), password.to_variant());
    }
    Ok(s)
}

/// Save the network with its password and connect to it. If it doesn't
/// connect (wrong password...), the saved network is removed again.
pub fn connect_with_password(ssid: &str, kind: &str, password: &str) -> Result<(), String> {
    let bus = gio::bus_get_sync(gio::BusType::System, None::<&gio::Cancellable>)
        .map_err(|e| e.message().to_string())?;
    let device = wifi_device(&bus)?;

    // a network with this name saved before (perhaps with an old password)
    // is replaced, like `nmcli device wifi connect` does
    crate::sys::wifi_forget_by_name(ssid);

    let mut connection: HashMap<String, Variant> = HashMap::new();
    connection.insert("id".into(), ssid.to_variant());
    connection.insert("type".into(), "802-11-wireless".to_variant());
    let mut wireless: HashMap<String, Variant> = HashMap::new();
    wireless.insert(
        "ssid".into(),
        Variant::array_from_fixed_array(ssid.as_bytes()),
    );
    wireless.insert("mode".into(), "infrastructure".to_variant());
    let mut settings: HashMap<String, HashMap<String, Variant>> = HashMap::new();
    settings.insert("connection".into(), connection);
    settings.insert("802-11-wireless".into(), wireless);
    settings.insert("802-11-wireless-security".into(), security(kind, password)?);

    let root = ObjectPath::try_from("/".to_string()).map_err(|_| "bad path")?;
    let args = (settings, device, root).to_variant();
    let r = call(
        &bus,
        NM_PATH,
        NM,
        "AddAndActivateConnection",
        Some(&args),
        30_000,
    )?;
    let (saved, active): (ObjectPath, ObjectPath) =
        r.get().ok_or("NetworkManager gave an odd answer")?;

    // wait until it's connected, or gave up
    let start = Instant::now();
    let iface = "org.freedesktop.NetworkManager.Connection.Active";
    let result = loop {
        if start.elapsed() > Duration::from_secs(45) {
            break Err("timed out".to_string());
        }
        match prop(&bus, active.as_str(), iface, "State").and_then(|v| v.get::<u32>()) {
            Some(ACTIVE_ACTIVATED) => break Ok(()),
            Some(ACTIVE_DEACTIVATING) | Some(ACTIVE_DEACTIVATED) | None => {
                break Err("wrong password, or the network didn't answer".to_string())
            }
            _ => std::thread::sleep(Duration::from_millis(300)),
        }
    };
    if result.is_err() {
        let _ = call(
            &bus,
            saved.as_str(),
            "org.freedesktop.NetworkManager.Settings.Connection",
            "Delete",
            None,
            5000,
        );
    }
    result
}

#[allow(dead_code)]
fn _assert_send() {
    fn is_send<T: Send>() {}
    is_send::<glib::Variant>();
}

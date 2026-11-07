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

/// activate_saved(): there's no saved Wi-Fi network of that name.
pub const NOT_SAVED: &str = "this network isn't saved";

const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const SETTINGS: &str = "org.freedesktop.NetworkManager.Settings";
const CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";

fn system_bus() -> Result<gio::DBusConnection, String> {
    gio::bus_get_sync(gio::BusType::System, None::<&gio::Cancellable>)
        .map_err(|e| e.message().to_string())
}

/// The saved Wi-Fi networks for this network name: only Wi-Fi ones, matched
/// on the network name itself (its bytes) - never on the saved profile's
/// name, which a VPN or wired profile could share (a network called "Work
/// VPN" must not touch the "Work VPN" VPN profile).
fn saved_wifi(bus: &gio::DBusConnection, ssid: &str) -> Vec<String> {
    let Ok(r) = call(bus, SETTINGS_PATH, SETTINGS, "ListConnections", None, 5000) else {
        return Vec::new();
    };
    let Some((paths,)) = r.get::<(Vec<ObjectPath>,)>() else {
        return Vec::new();
    };
    paths
        .into_iter()
        .filter(|p| {
            let Ok(r) = call(bus, p.as_str(), CONNECTION, "GetSettings", None, 5000) else {
                return false;
            };
            let settings = r.child_value(0);
            if !settings.is_type(glib::VariantTy::new("a{sa{sv}}").unwrap_or(glib::VariantTy::ANY))
            {
                return false;
            }
            let is_wifi = section(&settings, "connection")
                .and_then(|c| c.lookup_value("type", Some(glib::VariantTy::STRING)))
                .and_then(|t| t.str().map(|s| s == "802-11-wireless"))
                .unwrap_or(false);
            let same_name = section(&settings, "802-11-wireless")
                .and_then(|w| w.lookup_value("ssid", Some(glib::VariantTy::BYTE_STRING)))
                .map(|v| v.data() == ssid.as_bytes())
                .unwrap_or(false);
            is_wifi && same_name
        })
        .map(|p| p.as_str().to_string())
        .collect()
}

/// One section ("connection", "802-11-wireless"...) of a saved profile's
/// settings (a{sa{sv}}, already type-checked), to look values up in.
fn section(settings: &Variant, name: &str) -> Option<glib::VariantDict> {
    (0..settings.n_children()).find_map(|k| {
        let entry = settings.child_value(k);
        (entry.child_value(0).str() == Some(name))
            .then(|| glib::VariantDict::new(Some(&entry.child_value(1))))
    })
}

/// Forget the saved Wi-Fi network(s) for this network name (Wi-Fi only).
pub fn forget_wifi(ssid: &str) -> Result<(), String> {
    let bus = system_bus()?;
    forget_on(&bus, ssid);
    Ok(())
}

fn forget_on(bus: &gio::DBusConnection, ssid: &str) {
    for p in saved_wifi(bus, ssid) {
        let _ = call(bus, &p, CONNECTION, "Delete", None, 5000);
    }
}

/// Connect to a saved Wi-Fi network (by network name, Wi-Fi profiles only).
pub fn activate_saved(ssid: &str) -> Result<(), String> {
    let bus = system_bus()?;
    let saved = saved_wifi(&bus, ssid);
    let Some(conn) = saved.first() else {
        return Err(NOT_SAVED.into());
    };
    let device = wifi_device(&bus)?;
    let conn = ObjectPath::try_from(conn.clone()).map_err(|_| "bad path")?;
    let root = ObjectPath::try_from("/".to_string()).map_err(|_| "bad path")?;
    let r = call(
        &bus,
        NM_PATH,
        NM,
        "ActivateConnection",
        Some(&(conn, device, root).to_variant()),
        30_000,
    )?;
    let (active,): (ObjectPath,) = r.get().ok_or("NetworkManager gave an odd answer")?;
    wait_active(&bus, active.as_str())
}

/// Disconnect from this Wi-Fi network (Wi-Fi profiles only).
pub fn deactivate(ssid: &str) -> Result<(), String> {
    let bus = system_bus()?;
    let saved = saved_wifi(&bus, ssid);
    let Some(list) =
        prop(&bus, NM_PATH, NM, "ActiveConnections").and_then(|v| v.get::<Vec<ObjectPath>>())
    else {
        return Ok(());
    };
    for a in list {
        let conn = prop(&bus, a.as_str(), ACTIVE, "Connection").and_then(|v| v.get::<ObjectPath>());
        if conn
            .map(|c| saved.iter().any(|s| s == c.as_str()))
            .unwrap_or(false)
        {
            call(
                &bus,
                NM_PATH,
                NM,
                "DeactivateConnection",
                Some(&(a,).to_variant()),
                10_000,
            )?;
        }
    }
    Ok(())
}

/// Wait until a connection is up (Ok) or has given up.
fn wait_active(bus: &gio::DBusConnection, active: &str) -> Result<(), String> {
    let start = Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(45) {
            return Err("timed out".to_string());
        }
        match prop(bus, active, ACTIVE, "State").and_then(|v| v.get::<u32>()) {
            Some(ACTIVE_ACTIVATED) => return Ok(()),
            Some(ACTIVE_DEACTIVATING) | Some(ACTIVE_DEACTIVATED) | None => {
                return Err("wrong password, or the network didn't answer".to_string())
            }
            _ => std::thread::sleep(Duration::from_millis(300)),
        }
    }
}

/// Save the network with its password and connect to it. If it doesn't
/// connect (wrong password...), the saved network is removed again.
pub fn connect_with_password(ssid: &str, kind: &str, password: &str) -> Result<(), String> {
    let bus = system_bus()?;
    let device = wifi_device(&bus)?;

    // a Wi-Fi network with this name saved before (perhaps with an old
    // password) is replaced, like `nmcli device wifi connect` does
    forget_on(&bus, ssid);

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

    // wait until it's connected, or gave up; a failed one isn't kept (only
    // the profile just made is removed, by its own path)
    let result = wait_active(&bus, active.as_str());
    if result.is_err() {
        let _ = call(&bus, saved.as_str(), CONNECTION, "Delete", None, 5000);
    }
    result
}

#[allow(dead_code)]
fn _assert_send() {
    fn is_send<T: Send>() {}
    is_send::<glib::Variant>();
}

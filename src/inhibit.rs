//! "Don't turn the screen off": is something playing or asking to stay awake?
//!
//! Checked when the screen-off / sleep timer runs out (no polling):
//! * a media player says it's playing (MPRIS - browsers do this for YouTube),
//! * an app asked through org.freedesktop.ScreenSaver.Inhibit (served here;
//!   Chrome and others use it outside GNOME/KDE),
//! * something holds a systemd "idle" inhibitor (systemd-inhibit).
//!
//! Apps using the Wayland idle-inhibit protocol are already handled by the
//! compositor (swayidle never fires while they inhibit).

use std::cell::RefCell;

use gtk::prelude::*;
use gtk::{gio, glib};

const SS_XML: &str = r#"<node>
  <interface name="org.freedesktop.ScreenSaver">
    <method name="Inhibit">
      <arg type="s" name="application" direction="in"/>
      <arg type="s" name="reason" direction="in"/>
      <arg type="u" name="cookie" direction="out"/>
    </method>
    <method name="UnInhibit">
      <arg type="u" name="cookie" direction="in"/>
    </method>
    <method name="SimulateUserActivity"/>
    <method name="GetActive">
      <arg type="b" name="active" direction="out"/>
    </method>
  </interface>
</node>"#;

thread_local! {
    /// (cookie, bus name of the app that asked)
    static COOKIES: RefCell<Vec<(u32, String)>> = const { RefCell::new(Vec::new()) };
    static NEXT: std::cell::Cell<u32> = const { std::cell::Cell::new(1) };
}

/// Serve org.freedesktop.ScreenSaver on the session bus.
pub fn start() {
    let owner = gio::bus_own_name(
        gio::BusType::Session,
        "org.freedesktop.ScreenSaver",
        gio::BusNameOwnerFlags::NONE,
        |conn, _| serve(&conn),
        |_, _| {},
        |_, _| eprintln!("rusttouch: another program serves org.freedesktop.ScreenSaver"),
    );
    std::mem::forget(owner);
}

fn serve(conn: &gio::DBusConnection) {
    let Some(iface) = gio::DBusNodeInfo::for_xml(SS_XML)
        .ok()
        .and_then(|n| n.lookup_interface("org.freedesktop.ScreenSaver"))
    else {
        return;
    };
    for path in ["/org/freedesktop/ScreenSaver", "/ScreenSaver"] {
        let reg = conn
            .register_object(path, &iface)
            .method_call(
                |_conn, sender, _path, _iface, method, params, invocation| match method {
                    "Inhibit" => {
                        let app: String = params.child_value(0).get().unwrap_or_default();
                        let cookie = NEXT.with(|n| {
                            let c = n.get();
                            n.set(c.wrapping_add(1).max(1));
                            c
                        });
                        let who = sender.unwrap_or("").to_string();
                        COOKIES.with(|c| c.borrow_mut().push((cookie, who)));
                        eprintln!("rusttouch: {app} keeps the screen on");
                        invocation.return_value(Some(&(cookie,).to_variant()));
                    }
                    "UnInhibit" => {
                        let cookie: u32 = params.child_value(0).get().unwrap_or_default();
                        COOKIES.with(|c| c.borrow_mut().retain(|e| e.0 != cookie));
                        invocation.return_value(None);
                    }
                    "GetActive" => invocation.return_value(Some(&(false,).to_variant())),
                    _ => invocation.return_value(None),
                },
            )
            .build();
        if let Err(e) = reg {
            eprintln!("rusttouch: ScreenSaver: {e}");
        }
    }
    // apps that quit without un-inhibiting
    let _ = conn.signal_subscribe(
        Some("org.freedesktop.DBus"),
        Some("org.freedesktop.DBus"),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        None,
        gio::DBusSignalFlags::NONE,
        |_, _, _, _, _, params| {
            let name: String = params.child_value(0).get().unwrap_or_default();
            let new: String = params.child_value(2).get().unwrap_or_default();
            if new.is_empty() {
                COOKIES.with(|c| c.borrow_mut().retain(|e| e.1 != name));
            }
        },
    );
}

/// Should the screen stay on right now?
pub async fn busy() -> bool {
    if COOKIES.with(|c| !c.borrow().is_empty()) {
        return true;
    }
    if let Ok(conn) = gio::bus_get_future(gio::BusType::Session).await {
        if media_playing(&conn).await {
            return true;
        }
    }
    if let Ok(sys) = gio::bus_get_future(gio::BusType::System).await {
        let r = sys
            .call_future(
                Some("org.freedesktop.login1"),
                "/org/freedesktop/login1",
                "org.freedesktop.DBus.Properties",
                "Get",
                Some(&("org.freedesktop.login1.Manager", "BlockInhibited").to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                1000,
            )
            .await;
        if let Some((v,)) = r.ok().and_then(|r| r.get::<(glib::Variant,)>()) {
            if v.str()
                .map(|s| s.split(':').any(|w| w == "idle"))
                .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

async fn media_playing(conn: &gio::DBusConnection) -> bool {
    let names = conn
        .call_future(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "ListNames",
            None,
            None,
            gio::DBusCallFlags::NONE,
            1000,
        )
        .await
        .ok()
        .and_then(|v| v.get::<(Vec<String>,)>())
        .map(|v| v.0)
        .unwrap_or_default();
    for n in names
        .iter()
        .filter(|n| n.starts_with("org.mpris.MediaPlayer2."))
    {
        let r = conn
            .call_future(
                Some(n.as_str()),
                "/org/mpris/MediaPlayer2",
                "org.freedesktop.DBus.Properties",
                "Get",
                Some(&("org.mpris.MediaPlayer2.Player", "PlaybackStatus").to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                1000,
            )
            .await;
        if let Some((v,)) = r.ok().and_then(|r| r.get::<(glib::Variant,)>()) {
            if v.str() == Some("Playing") {
                return true;
            }
        }
    }
    false
}

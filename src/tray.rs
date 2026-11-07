//! System tray (StatusNotifierItem - what GNOME's AppIndicator extension,
//! KDE and waybar use). RustTouch owns org.kde.StatusNotifierWatcher, so apps
//! with tray icons (e.g. rustsync-gui --tray) register with it and show up
//! in the top bar. Tapping an icon opens that app's page in quick settings
//! (Open + the app's own menu); quick settings also lists all of them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

const WATCHER_XML: &str = r#"
<node>
  <interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem"><arg name="service" type="s" direction="in"/></method>
    <method name="RegisterStatusNotifierHost"><arg name="service" type="s" direction="in"/></method>
    <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
    <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
    <property name="ProtocolVersion" type="i" access="read"/>
    <signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
    <signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
    <signal name="StatusNotifierHostRegistered"/>
  </interface>
</node>"#;

const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const WATCHER_IFACE: &str = "org.kde.StatusNotifierWatcher";
const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";
const MENU_IFACE: &str = "com.canonical.dbusmenu";

/// Value for `key` in an a{sv} dictionary.
fn lookup(dict: &glib::Variant, key: &str) -> Option<glib::Variant> {
    glib::VariantDict::new(Some(dict)).lookup_value(key, None)
}

#[derive(Clone)]
enum IconSrc {
    Name(String),
    File(String),
    Texture(gdk::Texture),
}

struct Item {
    /// what the app registered ("bus/path")
    id: String,
    bus: String,
    /// unique bus name (":1.42") - signals come from this
    owner: RefCell<String>,
    path: String,
    button: gtk::Button,
    title: RefCell<String>,
    icon: RefCell<Option<IconSrc>>,
    visible: Cell<bool>,
    menu: RefCell<Option<String>>,
}

pub struct TrayInner {
    area: gtk::Box,
    conn: RefCell<Option<gio::DBusConnection>>,
    items: RefCell<Vec<Rc<Item>>>,
    on_tap: RefCell<Option<Rc<dyn Fn(String)>>>,
    listeners: RefCell<Vec<Box<dyn Fn()>>>,
}

pub type Tray = Rc<TrayInner>;

/// A tray app, for listing in quick settings.
pub struct TrayApp {
    pub id: String,
    pub title: String,
}

/// One line of an app's tray menu.
#[derive(Clone)]
pub struct MenuEntry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub depth: usize,
}

/// Start the tray; icons are added to `area` (in the top bar).
pub fn start(area: &gtk::Box) -> Tray {
    let tray = Rc::new(TrayInner {
        area: area.clone(),
        conn: RefCell::new(None),
        items: RefCell::new(Vec::new()),
        on_tap: RefCell::new(None),
        listeners: RefCell::new(Vec::new()),
    });
    let t1 = tray.clone();
    let owner = gio::bus_own_name(
        gio::BusType::Session,
        WATCHER_IFACE,
        gio::BusNameOwnerFlags::NONE,
        move |conn, _| bus_acquired(&t1, conn),
        |conn, _| {
            eprintln!("rusttouch: tray ready (StatusNotifierWatcher)");
            let _ = conn.emit_signal(
                None,
                WATCHER_PATH,
                WATCHER_IFACE,
                "StatusNotifierHostRegistered",
                None,
            );
        },
        |_, _| {
            eprintln!(
                "rusttouch: another tray host owns StatusNotifierWatcher - no tray icons here"
            )
        },
    );
    std::mem::forget(owner);
    tray
}

/// Called with the app id when a top-bar tray icon is tapped.
pub fn on_tap(t: &Tray, f: impl Fn(String) + 'static) {
    *t.on_tap.borrow_mut() = Some(Rc::new(f));
}

/// Called whenever tray apps appear, disappear or change.
pub fn on_change(t: &Tray, f: impl Fn() + 'static) {
    t.listeners.borrow_mut().push(Box::new(f));
}

fn changed(t: &Tray) {
    for f in t.listeners.borrow().iter() {
        f();
    }
}

pub fn apps(t: &Tray) -> Vec<TrayApp> {
    t.items
        .borrow()
        .iter()
        .filter(|i| i.visible.get())
        .map(|i| TrayApp {
            id: i.id.clone(),
            title: {
                let title = i.title.borrow().clone();
                if title.is_empty() {
                    i.bus.trim_start_matches(':').to_string()
                } else {
                    title
                }
            },
        })
        .collect()
}

fn find(t: &Tray, id: &str) -> Option<Rc<Item>> {
    t.items.borrow().iter().find(|i| i.id == id).cloned()
}

fn image_for(icon: &Option<IconSrc>, px: i32) -> gtk::Image {
    let img = match icon {
        Some(IconSrc::Name(n)) => gtk::Image::from_icon_name(n),
        Some(IconSrc::File(f)) => gtk::Image::from_file(f),
        Some(IconSrc::Texture(t)) => gtk::Image::from_paintable(Some(t)),
        None => gtk::Image::from_icon_name("application-x-executable"),
    };
    img.set_pixel_size(px);
    img
}

/// A fresh image of the app's current icon.
pub fn image(t: &Tray, id: &str, px: i32) -> gtk::Image {
    let icon = find(t, id).and_then(|i| i.icon.borrow().clone());
    image_for(&icon, px)
}

/// "Open": the app's primary action (usually shows its window).
pub fn activate(t: &Tray, id: &str) {
    let (Some(i), Some(c)) = (find(t, id), t.conn.borrow().clone()) else {
        return;
    };
    call(
        &c,
        &i.bus,
        &i.path,
        ITEM_IFACE,
        "Activate",
        Some((0i32, 0i32).to_variant()),
    );
}

pub fn has_menu(t: &Tray, id: &str) -> bool {
    find(t, id)
        .map(|i| i.menu.borrow().is_some())
        .unwrap_or(false)
}

/// Fetch the app's menu and hand it to `done`.
pub fn menu(t: &Tray, id: &str, done: impl FnOnce(Vec<MenuEntry>) + 'static) {
    let (Some(i), Some(c)) = (find(t, id), t.conn.borrow().clone()) else {
        return;
    };
    let Some(menu) = i.menu.borrow().clone() else {
        done(Vec::new());
        return;
    };
    glib::spawn_future_local(async move {
        let _ = c
            .call_future(
                Some(i.bus.as_str()),
                &menu,
                MENU_IFACE,
                "AboutToShow",
                Some(&(0i32,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                1000,
            )
            .await;
        let props: Vec<String> = Vec::new();
        let r = c
            .call_future(
                Some(i.bus.as_str()),
                &menu,
                MENU_IFACE,
                "GetLayout",
                Some(&(0i32, -1i32, props).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                3000,
            )
            .await;
        let mut entries = Vec::new();
        match r {
            Ok(v) => collect(&v.child_value(1), 0, &mut entries),
            Err(e) => eprintln!("rusttouch: tray menu: {e}"),
        }
        done(entries);
    });
}

/// Choose a menu entry.
pub fn click(t: &Tray, id: &str, entry: i32) {
    let (Some(i), Some(c)) = (find(t, id), t.conn.borrow().clone()) else {
        return;
    };
    let Some(menu) = i.menu.borrow().clone() else {
        return;
    };
    call(
        &c,
        &i.bus,
        &menu,
        MENU_IFACE,
        "Event",
        Some((entry, "clicked", 0i32.to_variant(), 0u32).to_variant()),
    );
}

fn bus_acquired(tray: &Tray, conn: gio::DBusConnection) {
    *tray.conn.borrow_mut() = Some(conn.clone());
    let Some(iface) = gio::DBusNodeInfo::for_xml(WATCHER_XML)
        .ok()
        .and_then(|n| n.lookup_interface(WATCHER_IFACE))
    else {
        return;
    };
    let (t1, t2) = (tray.clone(), tray.clone());
    let reg =
        conn.register_object(WATCHER_PATH, &iface)
            .method_call(
                move |conn, sender, _path, _iface, method, params, invocation| match method {
                    "RegisterStatusNotifierItem" => {
                        let service: String = params.child_value(0).get().unwrap_or_default();
                        let sender = sender.unwrap_or("").to_string();
                        let (bus, path) = if service.starts_with('/') {
                            (sender.clone(), service.clone())
                        } else {
                            (service.clone(), "/StatusNotifierItem".to_string())
                        };
                        let id = format!("{bus}{path}");
                        invocation.return_value(None);
                        add_item(&t1, &conn, id.clone(), bus, path, sender);
                        let _ = conn.emit_signal(
                            None,
                            WATCHER_PATH,
                            WATCHER_IFACE,
                            "StatusNotifierItemRegistered",
                            Some(&(id,).to_variant()),
                        );
                    }
                    "RegisterStatusNotifierHost" => invocation.return_value(None),
                    _ => invocation
                        .return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method),
                },
            )
            .property(move |_conn, _sender, _path, _iface, prop| match prop {
                "RegisteredStatusNotifierItems" => {
                    let ids: Vec<String> = t2.items.borrow().iter().map(|i| i.id.clone()).collect();
                    ids.to_variant()
                }
                "IsStatusNotifierHostRegistered" => true.to_variant(),
                _ => 0i32.to_variant(),
            })
            .build();
    if let Err(e) = reg {
        eprintln!("rusttouch: tray: {e}");
        return;
    }

    // apps that quit: drop their icons
    let t = tray.clone();
    // subscribe_to_signal() unsubscribes when its handle drops; these are meant to
    // live for the whole session, so keep the old call.
    #[allow(deprecated)]
    let _ = conn.signal_subscribe(
        Some("org.freedesktop.DBus"),
        Some("org.freedesktop.DBus"),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        None,
        gio::DBusSignalFlags::NONE,
        move |conn, _, _, _, _, params| {
            let name: String = params.child_value(0).get().unwrap_or_default();
            let new: String = params.child_value(2).get().unwrap_or_default();
            if new.is_empty() {
                remove_items(&t, conn, &name);
            }
        },
    );

    // icon/status/title changed
    let t = tray.clone();
    // subscribe_to_signal() unsubscribes when its handle drops; these are meant to
    // live for the whole session, so keep the old call.
    #[allow(deprecated)]
    let _ = conn.signal_subscribe(
        None,
        Some(ITEM_IFACE),
        None,
        None,
        None,
        gio::DBusSignalFlags::NONE,
        move |conn, sender, path, _, _, _| {
            let found = t
                .items
                .borrow()
                .iter()
                .find(|i| (*i.owner.borrow() == sender || i.bus == sender) && i.path == path)
                .cloned();
            if let Some(item) = found {
                refresh(&t, &item, conn);
            }
        },
    );
}

fn add_item(
    tray: &Tray,
    conn: &gio::DBusConnection,
    id: String,
    bus: String,
    path: String,
    sender: String,
) {
    if tray.items.borrow().iter().any(|i| i.id == id) {
        return;
    }
    let button = gtk::Button::new();
    button.add_css_class("tray-icon");
    button.set_focusable(false);
    let item = Rc::new(Item {
        id: id.clone(),
        bus,
        owner: RefCell::new(sender),
        path,
        button: button.clone(),
        title: RefCell::new(String::new()),
        icon: RefCell::new(None),
        visible: Cell::new(true),
        menu: RefCell::new(None),
    });

    // tap: open this app's page in quick settings
    let t = tray.clone();
    button.connect_clicked(move |_| {
        let f = t.on_tap.borrow().clone();
        if let Some(f) = f {
            f(id.clone());
        }
    });

    tray.area.append(&button);
    tray.items.borrow_mut().push(item.clone());

    // learn the unique name behind a well-known one (signals use it)
    if !item.bus.starts_with(':') {
        let (i, c) = (item.clone(), conn.clone());
        glib::spawn_future_local(async move {
            let r = c
                .call_future(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus",
                    "GetNameOwner",
                    Some(&(i.bus.as_str(),).to_variant()),
                    glib::VariantTy::new("(s)").ok(),
                    gio::DBusCallFlags::NONE,
                    2000,
                )
                .await;
            if let Ok(v) = r {
                if let Some(o) = v.child_value(0).get::<String>() {
                    *i.owner.borrow_mut() = o;
                }
            }
        });
    }
    refresh(tray, &item, conn);
    changed(tray);
}

fn remove_items(tray: &Tray, conn: &gio::DBusConnection, name: &str) {
    let gone: Vec<Rc<Item>> = tray
        .items
        .borrow()
        .iter()
        .filter(|i| i.bus == name || *i.owner.borrow() == name)
        .cloned()
        .collect();
    if gone.is_empty() {
        return;
    }
    for i in gone {
        tray.area.remove(&i.button);
        tray.items.borrow_mut().retain(|x| x.id != i.id);
        let _ = conn.emit_signal(
            None,
            WATCHER_PATH,
            WATCHER_IFACE,
            "StatusNotifierItemUnregistered",
            Some(&(i.id.clone(),).to_variant()),
        );
    }
    changed(tray);
}

fn call(
    conn: &gio::DBusConnection,
    bus: &str,
    path: &str,
    iface: &str,
    method: &str,
    args: Option<glib::Variant>,
) {
    let (c, bus, path, iface, method) = (
        conn.clone(),
        bus.to_string(),
        path.to_string(),
        iface.to_string(),
        method.to_string(),
    );
    glib::spawn_future_local(async move {
        let r = c
            .call_future(
                Some(bus.as_str()),
                &path,
                &iface,
                &method,
                args.as_ref(),
                None,
                gio::DBusCallFlags::NONE,
                3000,
            )
            .await;
        if let Err(e) = r {
            eprintln!("rusttouch: tray {method}: {e}");
        }
    });
}

/// Re-read the item's properties and update its icon.
fn refresh(tray: &Tray, item: &Rc<Item>, conn: &gio::DBusConnection) {
    let (t, i, c) = (tray.clone(), item.clone(), conn.clone());
    glib::spawn_future_local(async move {
        let r = c
            .call_future(
                Some(i.bus.as_str()),
                &i.path,
                "org.freedesktop.DBus.Properties",
                "GetAll",
                Some(&(ITEM_IFACE,).to_variant()),
                glib::VariantTy::new("(a{sv})").ok(),
                gio::DBusCallFlags::NONE,
                3000,
            )
            .await;
        let Ok(v) = r else { return };
        let props = v.child_value(0);
        let get_s = |k: &str| -> Option<String> {
            lookup(&props, k).and_then(|x| x.str().map(String::from))
        };

        let status = get_s("Status").unwrap_or_else(|| "Active".to_string());
        i.visible.set(status != "Passive");
        i.button.set_visible(status != "Passive");
        *i.menu.borrow_mut() = get_s("Menu").filter(|m| !m.is_empty() && m != "/");
        let title = get_s("Title").unwrap_or_default();
        i.button.set_tooltip_text(Some(title.as_str()));
        *i.title.borrow_mut() = title;

        let attention = status == "NeedsAttention";
        let name: Option<String> = if attention {
            get_s("AttentionIconName")
        } else {
            None
        }
        .filter(|n| !n.is_empty())
        .or_else(|| get_s("IconName").filter(|n| !n.is_empty()));
        let pix_key = if attention {
            "AttentionIconPixmap"
        } else {
            "IconPixmap"
        };
        let pix = pixmap(&props, pix_key).or_else(|| pixmap(&props, "IconPixmap"));
        if let (Some(dir), Some(d)) = (get_s("IconThemePath"), gdk::Display::default()) {
            if !dir.is_empty() {
                gtk::IconTheme::for_display(&d).add_search_path(&dir);
            }
        }
        let has_icon = |n: &str| {
            gdk::Display::default()
                .map(|d| gtk::IconTheme::for_display(&d).has_icon(n))
                .unwrap_or(false)
        };
        let src = match (name, pix) {
            (Some(n), _) if std::path::Path::new(&n).is_file() => Some(IconSrc::File(n)),
            (Some(n), Some(tex)) if !has_icon(&n) => Some(IconSrc::Texture(tex)),
            (Some(n), _) => Some(IconSrc::Name(n)),
            (None, Some(tex)) => Some(IconSrc::Texture(tex)),
            (None, None) => None,
        };
        i.button.set_child(Some(&image_for(&src, 18)));
        *i.icon.borrow_mut() = src;
        changed(&t);
    });
}

/// IconPixmap: a(iiay), ARGB32 in network byte order; take the largest.
fn pixmap(props: &glib::Variant, key: &str) -> Option<gdk::Texture> {
    let arr = lookup(props, key)?;
    let mut best: Option<(i32, i32, Vec<u8>)> = None;
    for k in 0..arr.n_children() {
        let e = arr.child_value(k);
        let w = e.child_value(0).get::<i32>()?;
        let h = e.child_value(1).get::<i32>()?;
        let data = e.child_value(2).data().to_vec();
        if w <= 0 || h <= 0 || data.len() < (w * h * 4) as usize {
            continue;
        }
        if best.as_ref().map(|b| w * h > b.0 * b.1).unwrap_or(true) {
            best = Some((w, h, data));
        }
    }
    let (w, h, data) = best?;
    let bytes = glib::Bytes::from_owned(data);
    Some(
        gdk::MemoryTexture::new(w, h, gdk::MemoryFormat::A8r8g8b8, &bytes, (w * 4) as usize)
            .upcast(),
    )
}

fn strip_mnemonic(s: &str) -> String {
    s.replace("__", "\u{0}")
        .replace('_', "")
        .replace('\u{0}', "_")
}

/// Flatten a dbusmenu layout node (i a{sv} av) into entries.
fn collect(node: &glib::Variant, depth: usize, out: &mut Vec<MenuEntry>) {
    let children = node.child_value(2);
    for k in 0..children.n_children() {
        let Some(child) = children.child_value(k).as_variant() else {
            continue;
        };
        let id = child.child_value(0).get::<i32>().unwrap_or(0);
        let props = child.child_value(1);
        let b = |key: &str, def: bool| {
            lookup(&props, key)
                .and_then(|x| x.get::<bool>())
                .unwrap_or(def)
        };
        let s = |key: &str| -> Option<String> {
            lookup(&props, key).and_then(|x| x.str().map(String::from))
        };
        if !b("visible", true) {
            continue;
        }
        let separator = s("type").as_deref() == Some("separator");
        let mut label = strip_mnemonic(&s("label").unwrap_or_default());
        if s("toggle-type").is_some() {
            let on = lookup(&props, "toggle-state")
                .and_then(|x| x.get::<i32>())
                .unwrap_or(0)
                == 1;
            label = format!("{} {label}", if on { "✓" } else { "  " });
        }
        out.push(MenuEntry {
            id,
            label,
            enabled: b("enabled", true),
            separator,
            depth,
        });
        if s("children-display").as_deref() == Some("submenu") {
            collect(&child, depth + 1, out);
        }
    }
}

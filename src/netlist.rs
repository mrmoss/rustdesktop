//! Wi-Fi / Bluetooth list page inside quick settings (long-press the tiles).
//! Tap a network to connect (asks for a password if it needs one), tap the
//! connected one to disconnect, × forgets it. Same for Bluetooth devices
//! (new ones are paired + trusted + connected in one tap).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};

use crate::audio;
use crate::icons::{Glyph, Icon};
use crate::sys::{self, BtDevice, WifiNet};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Wifi,
    Bluetooth,
    Audio,
}

/// One line of the Sound page.
#[derive(Clone)]
enum AudioItem {
    Header(&'static str),
    Out(audio::Device),
    In(audio::Device),
    /// headset, switch to mic mode (true) or music mode (false)
    Headset(audio::Headset, bool),
}

pub struct NetList {
    pub root: gtk::Box,
    title: gtk::Label,
    list: gtk::ListBox,
    status: gtk::Label,
    spinner: gtk::Spinner,
    pass_row: gtk::Revealer,
    pass: gtk::PasswordEntry,
    /// "Join <network>" at the top of the password card
    pass_title: gtk::Label,
    /// the list, dimmed while the password card is up
    scroll: gtk::ScrolledWindow,
    kind: Cell<Kind>,
    wifi: RefCell<Vec<WifiNet>>,
    bt: RefCell<Vec<BtDevice>>,
    audio: RefCell<Vec<AudioItem>>,
    scan: gtk::Button,
    pending: RefCell<Option<WifiNet>>,
    rename_row: gtk::Revealer,
    rename: gtk::Entry,
    renaming: RefCell<Option<BtDevice>>,
    busy: Cell<bool>,
    on_change: Rc<dyn Fn()>,
    on_typing: Rc<dyn Fn(bool)>,
}

pub type List = Rc<NetList>;

pub fn build(
    on_back: impl Fn() + 'static,
    on_change: Rc<dyn Fn()>,
    on_typing: Rc<dyn Fn(bool)>,
) -> List {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let back = gtk::Button::with_label("‹");
    back.add_css_class("qs-round");
    back.set_focusable(false);
    let title = gtk::Label::new(None);
    title.add_css_class("qs-list-title");
    title.set_hexpand(true);
    title.set_halign(gtk::Align::Start);
    let spinner = gtk::Spinner::new();
    let scan = gtk::Button::with_label("Scan");
    scan.add_css_class("pill");
    scan.set_focusable(false);
    header.append(&back);
    header.append(&title);
    header.append(&spinner);
    header.append(&scan);
    root.append(&header);

    // Password for a new secured network: its own highlighted card, so it's
    // clear you've picked a network and are now joining it (the list below
    // dims meanwhile).
    let pass_row = gtk::Revealer::new();
    pass_row.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("join-card");
    let pass_title = gtk::Label::new(None);
    pass_title.add_css_class("join-title");
    pass_title.set_halign(gtk::Align::Start);
    pass_title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let hint = gtk::Label::new(Some("Type the Wi-Fi password"));
    hint.add_css_class("qs-sub");
    hint.set_halign(gtk::Align::Start);
    let pass = gtk::PasswordEntry::new();
    pass.set_show_peek_icon(true);
    pass.set_hexpand(true);
    pass.add_css_class("lock-entry");
    pass.add_css_class("join-entry");
    pass.set_property("placeholder-text", "Password");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("pill");
    cancel.set_focusable(false);
    let join = gtk::Button::with_label("Connect");
    join.add_css_class("pill");
    join.add_css_class("join-btn");
    join.set_focusable(false);
    buttons.append(&cancel);
    buttons.append(&join);
    card.append(&pass_title);
    card.append(&hint);
    card.append(&pass);
    card.append(&buttons);
    pass_row.set_child(Some(&card));
    root.append(&pass_row);

    // nickname prompt for Bluetooth devices
    let rename_row = gtk::Revealer::new();
    let rbox = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let rename = gtk::Entry::new();
    rename.set_hexpand(true);
    rename.set_placeholder_text(Some("Nickname"));
    rename.add_css_class("lock-entry");
    let rsave = gtk::Button::with_label("Save");
    rsave.add_css_class("pill");
    rbox.append(&rename);
    rbox.append(&rsave);
    rename_row.set_child(Some(&rbox));
    root.append(&rename_row);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("qs-list");
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(260)
        .max_content_height(360)
        .propagate_natural_height(true)
        .build();
    root.append(&scroll);

    let status = gtk::Label::new(None);
    status.add_css_class("qs-sub");
    status.set_wrap(true);
    root.append(&status);

    let nl = Rc::new(NetList {
        root,
        title,
        list: list.clone(),
        status,
        spinner,
        pass_row,
        pass: pass.clone(),
        pass_title,
        scroll: scroll.clone(),
        kind: Cell::new(Kind::Wifi),
        wifi: RefCell::new(Vec::new()),
        bt: RefCell::new(Vec::new()),
        audio: RefCell::new(Vec::new()),
        scan: scan.clone(),
        pending: RefCell::new(None),
        rename_row,
        rename: rename.clone(),
        renaming: RefCell::new(None),
        busy: Cell::new(false),
        on_change,
        on_typing,
    });

    back.connect_clicked(move |_| on_back());
    let n = nl.clone();
    scan.connect_clicked(move |_| refresh(&n, true));
    let n = nl.clone();
    list.connect_row_activated(move |_, row| activate(&n, row.index().max(0) as usize));
    let n = nl.clone();
    join.connect_clicked(move |_| join_pending(&n));
    let n = nl.clone();
    cancel.connect_clicked(move |_| {
        n.pending.borrow_mut().take();
        show_join(&n, None);
    });
    let n = nl.clone();
    pass.connect_activate(move |_| join_pending(&n));
    let n = nl.clone();
    rsave.connect_clicked(move |_| save_rename(&n));
    let n = nl.clone();
    rename.connect_activate(move |_| save_rename(&n));
    nl
}

/// Show the password card for `net` (or put it away: None). The list dims
/// while it shows.
fn show_join(nl: &List, net: Option<&WifiNet>) {
    match net {
        Some(n) => {
            nl.pass_title.set_text(&format!("Join “{}”", n.ssid));
            nl.pass.set_text("");
            nl.status.set_text("");
            nl.pass_row.set_reveal_child(true);
            nl.scroll.set_sensitive(false);
            nl.scroll.set_opacity(0.35);
            (nl.on_typing)(true);
            nl.pass.grab_focus();
        }
        None => {
            if nl.pass_row.reveals_child() {
                (nl.on_typing)(false);
            }
            nl.pass_row.set_reveal_child(false);
            nl.pass.set_text("");
            nl.scroll.set_sensitive(true);
            nl.scroll.set_opacity(1.0);
        }
    }
}

fn ask_rename(nl: &List, dev: BtDevice) {
    show_join(nl, None);
    nl.rename.set_text(&dev.name);
    nl.status.set_text(&format!("Nickname for {}", dev.mac));
    *nl.renaming.borrow_mut() = Some(dev);
    nl.rename_row.set_reveal_child(true);
    (nl.on_typing)(true);
    nl.rename.grab_focus();
}

fn save_rename(nl: &List) {
    let Some(dev) = nl.renaming.borrow_mut().take() else {
        return;
    };
    let alias = nl.rename.text().trim().to_string();
    nl.rename_row.set_reveal_child(false);
    (nl.on_typing)(false);
    run(nl, format!("Renaming to {alias}…"), move || {
        sys::bt_rename(&dev, &alias)
    });
}

/// Show the page for `kind` and load the list.
pub fn open(nl: &List, kind: Kind) {
    nl.kind.set(kind);
    nl.title.set_text(match kind {
        Kind::Wifi => "Wi-Fi",
        Kind::Bluetooth => "Bluetooth",
        Kind::Audio => "Sound",
    });
    nl.scan.set_label(if kind == Kind::Audio {
        "Refresh"
    } else {
        "Scan"
    });
    nl.status.set_text("");
    show_join(nl, None);
    nl.pending.borrow_mut().take();
    nl.rename_row.set_reveal_child(false);
    (nl.on_typing)(false);
    clear(nl);
    refresh(nl, kind == Kind::Bluetooth); // bluetooth: also look for new devices
}

fn clear(nl: &List) {
    while let Some(c) = nl.list.first_child() {
        nl.list.remove(&c);
    }
}

fn set_busy(nl: &List, busy: bool, msg: &str) {
    nl.busy.set(busy);
    if busy {
        nl.spinner.start();
    } else {
        nl.spinner.stop();
    }
    nl.status.set_text(msg);
}

fn refresh(nl: &List, scan: bool) {
    let kind = nl.kind.get();
    set_busy(nl, true, if scan { "Scanning…" } else { "" });
    let n = nl.clone();
    glib::spawn_future_local(async move {
        match kind {
            Kind::Wifi => {
                let nets = gio::spawn_blocking(move || sys::wifi_list(scan))
                    .await
                    .unwrap_or_default();
                *n.wifi.borrow_mut() = nets;
            }
            Kind::Bluetooth => {
                let devs = gio::spawn_blocking(move || sys::bt_list(scan))
                    .await
                    .unwrap_or_default();
                *n.bt.borrow_mut() = devs;
            }
            Kind::Audio => {
                let st = gio::spawn_blocking(audio::state).await.unwrap_or_default();
                let mut items = vec![AudioItem::Header("Output")];
                items.extend(st.outputs.into_iter().map(AudioItem::Out));
                items.push(AudioItem::Header("Input"));
                items.extend(st.inputs.into_iter().map(AudioItem::In));
                if !st.headsets.is_empty() {
                    items.push(AudioItem::Header("Bluetooth headsets"));
                    for h in st.headsets {
                        let to_mic = !h.mic_mode;
                        items.push(AudioItem::Headset(h, to_mic));
                    }
                }
                *n.audio.borrow_mut() = items;
            }
        }
        if n.kind.get() == kind {
            render(&n);
            set_busy(&n, false, "");
        }
    });
}

fn row(
    nl: &List,
    glyph: Glyph,
    name: &str,
    sub: &str,
    forget: Option<Box<dyn Fn()>>,
    rename: Option<BtDevice>,
) -> gtk::ListBoxRow {
    let hb = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    hb.add_css_class("qs-list-row");
    hb.append(&Icon::new(glyph, 20).area);
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 1);
    vb.set_hexpand(true);
    let t = gtk::Label::new(Some(name));
    t.set_halign(gtk::Align::Start);
    t.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let s = gtk::Label::new(Some(sub));
    s.set_halign(gtk::Align::Start);
    s.add_css_class("qs-sub");
    vb.append(&t);
    vb.append(&s);
    hb.append(&vb);
    if let Some(dev) = rename {
        let e = gtk::Button::with_label("✎");
        e.add_css_class("qs-round");
        e.set_focusable(false);
        e.set_tooltip_text(Some("Nickname"));
        e.set_valign(gtk::Align::Center);
        let n = nl.clone();
        e.connect_clicked(move |_| ask_rename(&n, dev.clone()));
        hb.append(&e);
    }
    if let Some(f) = forget {
        let x = gtk::Button::with_label("×");
        x.add_css_class("qs-round");
        x.set_focusable(false);
        x.set_tooltip_text(Some("Forget"));
        x.set_valign(gtk::Align::Center);
        let n = nl.clone();
        x.connect_clicked(move |_| {
            f();
            refresh(&n, false);
        });
        hb.append(&x);
    }
    let r = gtk::ListBoxRow::new();
    r.set_child(Some(&hb));
    r
}

fn header_row(text: &str) -> gtk::ListBoxRow {
    let l = gtk::Label::new(Some(text));
    l.set_halign(gtk::Align::Start);
    l.add_css_class("qs-list-header");
    let r = gtk::ListBoxRow::new();
    r.set_child(Some(&l));
    r.set_activatable(false);
    r.set_selectable(false);
    r
}

fn render(nl: &List) {
    clear(nl);
    match nl.kind.get() {
        Kind::Audio => {
            let items = nl.audio.borrow().clone();
            for it in items {
                let r = match it {
                    AudioItem::Header(t) => header_row(t),
                    AudioItem::Out(d) => row(
                        nl,
                        Glyph::Volume,
                        &d.desc,
                        match (d.is_default, d.bluetooth) {
                            (true, true) => "In use · Bluetooth",
                            (true, false) => "In use",
                            (false, true) => "Bluetooth",
                            (false, false) => "",
                        },
                        None,
                        None,
                    ),
                    AudioItem::In(d) => row(
                        nl,
                        Glyph::Mic,
                        &d.desc,
                        match (d.is_default, d.bluetooth) {
                            (true, true) => "In use · Bluetooth",
                            (true, false) => "In use",
                            (false, true) => "Bluetooth",
                            (false, false) => "",
                        },
                        None,
                        None,
                    ),
                    AudioItem::Headset(h, to_mic) => {
                        let (t, sub) = if to_mic {
                            (
                                format!("Use the mic on {}", h.desc),
                                "Call quality while the mic is on",
                            )
                        } else {
                            (
                                format!("High-quality audio on {}", h.desc),
                                "Turns the headset mic off",
                            )
                        };
                        row(
                            nl,
                            if to_mic { Glyph::Mic } else { Glyph::Volume },
                            &t,
                            sub,
                            None,
                            None,
                        )
                    }
                };
                nl.list.append(&r);
            }
        }
        Kind::Wifi => {
            let nets = nl.wifi.borrow().clone();
            if nets.is_empty() {
                nl.status.set_text("No networks found (is Wi-Fi on?)");
            }
            for net in nets {
                let bars = match net.signal {
                    0..=30 => 1,
                    31..=60 => 2,
                    _ => 3,
                };
                let sub = match (net.active, net.known, net.secure) {
                    (true, _, _) => "Connected · tap to disconnect".to_string(),
                    (false, true, _) => "Saved".to_string(),
                    (false, false, true) => "Secured".to_string(),
                    (false, false, false) => "Open".to_string(),
                };
                let forget: Option<Box<dyn Fn()>> = if net.known {
                    let n2 = net.clone();
                    Some(Box::new(move || sys::wifi_forget(&n2)))
                } else {
                    None
                };
                let r = row(nl, Glyph::Wifi(bars), &net.ssid, &sub, forget, None);
                nl.list.append(&r);
            }
        }
        Kind::Bluetooth => {
            let devs = nl.bt.borrow().clone();
            if devs.is_empty() {
                nl.status
                    .set_text("No devices found - put the device in pairing mode and tap Scan");
            }
            for d in devs {
                let sub = match (d.connected, d.paired) {
                    (true, _) => "Connected · tap to disconnect",
                    (false, true) => "Paired · tap to connect",
                    (false, false) => "Tap to pair",
                };
                let forget: Option<Box<dyn Fn()>> = if d.paired {
                    let d2 = d.clone();
                    Some(Box::new(move || sys::bt_forget(&d2)))
                } else {
                    None
                };
                let rename = d.paired.then(|| d.clone());
                let r = row(nl, Glyph::Bluetooth, &d.name, sub, forget, rename);
                nl.list.append(&r);
            }
        }
    }
}

fn activate(nl: &List, i: usize) {
    if nl.busy.get() {
        return;
    }
    match nl.kind.get() {
        Kind::Wifi => {
            let Some(net) = nl.wifi.borrow().get(i).cloned() else {
                return;
            };
            if net.active {
                run(
                    nl,
                    format!("Disconnecting from {}…", net.ssid),
                    move || {
                        sys::wifi_disconnect(&net);
                        Ok(())
                    },
                );
            } else if net.secure && !net.known {
                show_join(nl, Some(&net));
                *nl.pending.borrow_mut() = Some(net);
            } else {
                run(nl, format!("Connecting to {}…", net.ssid), move || {
                    sys::wifi_connect(&net, None)
                });
            }
        }
        Kind::Audio => {
            let Some(item) = nl.audio.borrow().get(i).cloned() else {
                return;
            };
            match item {
                AudioItem::Header(_) => {}
                AudioItem::Out(d) => run(nl, format!("Switching to {}…", d.desc), move || {
                    audio::set_output(&d.name);
                    Ok(())
                }),
                AudioItem::In(d) => run(nl, format!("Switching to {}…", d.desc), move || {
                    audio::set_input(&d.name);
                    Ok(())
                }),
                AudioItem::Headset(h, to_mic) => {
                    run(nl, "Switching headset mode…".into(), move || {
                        audio::headset_mode(&h, to_mic);
                        Ok(())
                    })
                }
            }
        }
        Kind::Bluetooth => {
            let Some(dev) = nl.bt.borrow().get(i).cloned() else {
                return;
            };
            let msg = if dev.connected {
                format!("Disconnecting {}…", dev.name)
            } else {
                format!("Connecting {}…", dev.name)
            };
            run(nl, msg, move || sys::bt_activate(&dev));
        }
    }
}

fn join_pending(nl: &List) {
    let Some(net) = nl.pending.borrow_mut().take() else {
        return;
    };
    let pw = nl.pass.text().to_string();
    show_join(nl, None);
    run(nl, format!("Connecting to {}…", net.ssid), move || {
        sys::wifi_connect(&net, Some(pw))
    });
}

/// Run a blocking network action on a worker thread, then refresh.
fn run(nl: &List, msg: String, f: impl FnOnce() -> Result<(), String> + Send + 'static) {
    set_busy(nl, true, &msg);
    let n = nl.clone();
    glib::spawn_future_local(async move {
        let res = gio::spawn_blocking(f)
            .await
            .unwrap_or_else(|_| Err("failed".into()));
        set_busy(&n, false, "");
        match res {
            Ok(()) => n.status.set_text("Done"),
            Err(e) => n.status.set_text(&format!("Didn't work: {e}")),
        }
        (n.on_change)();
        refresh(&n, false);
    });
}

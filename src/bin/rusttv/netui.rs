//! Settings pages for Wi-Fi, Bluetooth, Sound and Screens - RustTouch's
//! features, made for the remote.

use crate::net::{self, BtDevice, WifiNet};
use crate::{keyboard, screens, ui, Tv};
use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Run `work` on a worker thread, then `done` with its result here.
pub fn in_background<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    let done = RefCell::new(Some(done));
    glib::timeout_add_local(Duration::from_millis(150), move || match rx.try_recv() {
        Ok(v) => {
            if let Some(f) = done.borrow_mut().take() {
                f(v);
            }
            glib::ControlFlow::Break
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(_) => glib::ControlFlow::Break,
    });
}

fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

/// A row that only says something (focusable, so the list scrolls to it).
fn note_row(body: &gtk::Box, text: &str) {
    let (b, _) = ui::row(text, "", || {});
    b.add_css_class("dim");
    body.append(&b);
}

/// A list page that fills itself in the background: `load` fetches (on a
/// worker thread), `show` draws the rows. Returns what draws it again.
fn live_page<T: Send + 'static>(
    tv: &Tv,
    title: &str,
    load: fn(bool) -> T,
    show: impl Fn(&Tv, &gtk::Box, T, Rc<dyn Fn(bool)>) + 'static,
) {
    let body = ui::list();
    note_row(&body, "Looking...");
    let pg = ui::page(title, &body);
    let show = Rc::new(show);
    let reload: Rc<RefCell<Option<Rc<dyn Fn(bool)>>>> = Rc::new(RefCell::new(None));
    let r: Rc<dyn Fn(bool)> = {
        let (t, body, show, reload) = (tv.clone(), body.clone(), show.clone(), reload.clone());
        Rc::new(move |deep: bool| {
            let (t, body, show, reload) = (t.clone(), body.clone(), show.clone(), reload.clone());
            in_background(
                move || load(deep),
                move |v| {
                    // (where the focus was: back on the same row after)
                    let at = t.focused().filter(|f| f.is_ancestor(&body)).and_then(|f| {
                        let mut n = 0;
                        let mut c = body.first_child();
                        while let Some(w) = c {
                            if f == w || f.is_ancestor(&w) {
                                return Some(n);
                            }
                            n += 1;
                            c = w.next_sibling();
                        }
                        None
                    });
                    clear(&body);
                    let again = reload.borrow().clone();
                    if let Some(again) = again {
                        show(&t, &body, v, again);
                    }
                    let mut c = body.first_child();
                    let mut n = 0;
                    while let Some(w) = c {
                        if Some(n) == at.or(Some(0)) {
                            // (a row with keys in it, like the volume's: its first key)
                            if w.is_focusable() {
                                w.grab_focus();
                            } else {
                                w.child_focus(gtk::DirectionType::TabForward);
                            }
                            break;
                        }
                        n += 1;
                        c = w.next_sibling();
                    }
                },
            );
        })
    };
    *reload.borrow_mut() = Some(r.clone());
    let r2 = r.clone();
    tv.push_page(&pg, None, Some(Rc::new(move || r2(false))));
    r(false);
}

// ---------------------------------------------------------------- Wi-Fi

fn bars(signal: u8) -> &'static str {
    match signal {
        75.. => "▂▄▆█",
        50..=74 => "▂▄▆",
        25..=49 => "▂▄",
        _ => "▂",
    }
}

pub fn wifi_page(tv: &Tv) {
    live_page(
        tv,
        "Wi-Fi",
        |rescan| (net::wifi_on(), net::wifi_list(rescan)),
        |t, body, (on, nets): (Option<bool>, Vec<WifiNet>), reload| {
            let Some(on) = on else {
                note_row(body, "No Wi-Fi here (NetworkManager isn't running)");
                return;
            };
            let (t2, r2) = (t.clone(), reload.clone());
            let (power, _) = ui::row("Wi-Fi", if on { "On" } else { "Off" }, move || {
                let r = r2.clone();
                t2.toast(if on {
                    "Turning Wi-Fi off..."
                } else {
                    "Turning Wi-Fi on..."
                });
                in_background(move || net::set_wifi(!on), move |_| r(true));
            });
            body.append(&power);
            if !on {
                return;
            }
            let r2 = reload.clone();
            let (scan, _) = ui::row("Scan again", "", move || r2(true));
            body.append(&scan);
            body.append(&ui::heading("Networks"));
            if nets.is_empty() {
                note_row(body, "None found");
            }
            for n in nets {
                let state = if n.active {
                    "Connected".to_string()
                } else if n.known {
                    "Saved".to_string()
                } else {
                    String::new()
                };
                let value = format!(
                    "{}{}  {}",
                    if state.is_empty() {
                        String::new()
                    } else {
                        format!("{state}  ")
                    },
                    if n.secure { "🔒" } else { "" },
                    bars(n.signal)
                );
                let (t2, r2, n2) = (t.clone(), reload.clone(), n.clone());
                let (b, _) = ui::row(&n.ssid, value.trim(), move || {
                    wifi_menu(&t2, &n2, r2.clone())
                });
                if n.active {
                    b.add_css_class("current");
                }
                body.append(&b);
            }
        },
    );
}

fn wifi_menu(t: &Tv, n: &WifiNet, reload: Rc<dyn Fn(bool)>) {
    let mut items: Vec<(String, Rc<dyn Fn(&Tv)>)> = Vec::new();
    if n.active {
        let (n2, r) = (n.clone(), reload.clone());
        items.push((
            "Disconnect".into(),
            Rc::new(move |_t: &Tv| {
                let (n3, r) = (n2.clone(), r.clone());
                in_background(move || net::wifi_disconnect(&n3), move |_| r(false));
            }),
        ));
    } else {
        let (n2, r) = (n.clone(), reload.clone());
        items.push((
            "Connect".into(),
            Rc::new(move |t: &Tv| connect(t, &n2, r.clone())),
        ));
    }
    if n.known {
        let (n2, r) = (n.clone(), reload.clone());
        items.push((
            "Forget".into(),
            Rc::new(move |t: &Tv| {
                let (n3, r) = (n2.clone(), r.clone());
                ui::confirm(t, &format!("Forget {}?", n2.ssid), "Forget", move |_t| {
                    let (n4, r) = (n3.clone(), r.clone());
                    in_background(move || net::wifi_forget(&n4), move |_| r(false));
                });
            }),
        ));
    }
    ui::actions(t, &n.ssid, items);
}

fn connect(t: &Tv, n: &WifiNet, reload: Rc<dyn Fn(bool)>) {
    let go = move |t: &Tv, n: WifiNet, pw: Option<String>, reload: Rc<dyn Fn(bool)>| {
        t.toast(&format!("Connecting to {}...", n.ssid));
        let name = n.ssid.clone();
        in_background(
            move || net::wifi_connect(&n, pw),
            move |r| {
                crate::with_tv(|t| match r {
                    Ok(()) => t.toast(&format!("Connected to {name}")),
                    Err(e) => t.toast(&format!("Couldn't connect to {name}: {e}")),
                });
                reload(false);
            },
        );
    };
    if n.secure && !n.known {
        let n2 = n.clone();
        keyboard::ask_secret(t, &format!("Password for {}", n.ssid), move |t, pw| {
            if !pw.is_empty() {
                go(t, n2.clone(), Some(pw), reload.clone());
            }
        });
    } else {
        go(t, n.clone(), None, reload);
    }
}

// ---------------------------------------------------------------- Bluetooth

pub fn bluetooth_page(tv: &Tv) {
    live_page(
        tv,
        "Bluetooth",
        |scan| (net::bt_on(), net::bt_list(scan)),
        |t, body, (on, devs): (Option<bool>, Vec<BtDevice>), reload| {
            let Some(on) = on else {
                note_row(body, "No Bluetooth here");
                return;
            };
            let (t2, r2) = (t.clone(), reload.clone());
            let (power, _) = ui::row("Bluetooth", if on { "On" } else { "Off" }, move || {
                let r = r2.clone();
                t2.toast(if on {
                    "Turning Bluetooth off..."
                } else {
                    "Turning Bluetooth on..."
                });
                in_background(move || net::set_bluetooth(!on), move |_| r(false));
            });
            body.append(&power);
            if !on {
                return;
            }
            let (t2, r2) = (t.clone(), reload.clone());
            let (scan, _) = ui::row("Search for devices", "", move || {
                t2.toast("Searching for 8 seconds - put the device in pairing mode");
                r2(true)
            });
            body.append(&scan);
            body.append(&ui::heading("Devices"));
            if devs.is_empty() {
                note_row(body, "None yet - Search for devices");
            }
            for d in devs {
                let value = if d.connected {
                    "Connected"
                } else if d.paired {
                    "Paired"
                } else {
                    ""
                };
                let (t2, r2, d2) = (t.clone(), reload.clone(), d.clone());
                let (b, _) = ui::row(&d.name, value, move || bt_menu(&t2, &d2, r2.clone()));
                if d.connected {
                    b.add_css_class("current");
                }
                body.append(&b);
            }
        },
    );
}

fn bt_menu(t: &Tv, d: &BtDevice, reload: Rc<dyn Fn(bool)>) {
    let mut items: Vec<(String, Rc<dyn Fn(&Tv)>)> = Vec::new();
    let label = if d.connected { "Disconnect" } else { "Connect" };
    let (d2, r) = (d.clone(), reload.clone());
    items.push((
        label.into(),
        Rc::new(move |t: &Tv| {
            let (d3, r) = (d2.clone(), r.clone());
            let name = d2.name.clone();
            let was = d2.connected;
            t.toast(&if was {
                format!("Disconnecting {name}...")
            } else {
                format!("Connecting {name}...")
            });
            in_background(
                move || net::bt_activate(&d3),
                move |res| {
                    if let Err(e) = res {
                        crate::with_tv(|t| t.toast(&format!("{name}: {e}")));
                    }
                    r(false);
                },
            );
        }),
    ));
    if d.paired {
        let (d2, r) = (d.clone(), reload.clone());
        items.push((
            "Rename".into(),
            Rc::new(move |t: &Tv| {
                let (d3, r) = (d2.clone(), r.clone());
                keyboard::ask(t, "Name", &d2.name, move |_t, name| {
                    let (d4, r) = (d3.clone(), r.clone());
                    in_background(move || net::bt_rename(&d4, &name), move |_| r(false));
                });
            }),
        ));
        let (d2, r) = (d.clone(), reload.clone());
        items.push((
            "Forget".into(),
            Rc::new(move |t: &Tv| {
                let (d3, r) = (d2.clone(), r.clone());
                ui::confirm(t, &format!("Forget {}?", d2.name), "Forget", move |_t| {
                    let (d4, r) = (d3.clone(), r.clone());
                    in_background(move || net::bt_forget(&d4), move |_| r(false));
                });
            }),
        ));
    }
    ui::actions(t, &d.name, items);
}

// ---------------------------------------------------------------- Sound

type SoundState = (Option<(u32, bool)>, crate::audio::State);

thread_local! {
    /// the volume rows on screen (level, bar, mute value) - kept in step
    /// with the remote's volume buttons
    static VOLUME: RefCell<Vec<(glib::WeakRef<gtk::Label>, glib::WeakRef<gtk::LevelBar>, Option<glib::WeakRef<gtk::Label>>)>> =
        const { RefCell::new(Vec::new()) };
}

/// The volume changed (any way: the remote, these buttons): show it here.
pub fn volume_changed(level: f64, muted: bool) {
    VOLUME.with(|x| {
        for (l, b, m) in x.borrow().iter() {
            if let Some(l) = l.upgrade() {
                l.set_text(&format!("{:.0}%", level * 100.0));
            }
            if let Some(b) = b.upgrade() {
                b.set_value(level.min(1.0));
            }
            if let Some(m) = m.as_ref().and_then(|m| m.upgrade()) {
                m.set_text(if muted { "On" } else { "Off" });
            }
        }
    });
}

/// "Volume  [ − ]  ▬▬▬▬───  [ + ]  45%": OK on − or + turns it down or up
/// (5% each, like the remote).
fn volume_row(v: u32, _muted: bool) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 18);
    row.add_css_class("vol-row");
    let l = gtk::Label::new(Some("Volume"));
    l.set_hexpand(true);
    l.set_xalign(0.0);
    let down = gtk::Button::with_label("−");
    down.add_css_class("key");
    let bar = gtk::LevelBar::for_interval(0.0, 1.0);
    bar.set_mode(gtk::LevelBarMode::Continuous);
    bar.set_size_request(320, 12);
    bar.set_valign(gtk::Align::Center);
    bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_LOW));
    bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_HIGH));
    bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_FULL));
    bar.set_value((v as f64 / 100.0).min(1.0));
    let up = gtk::Button::with_label("+");
    up.add_css_class("key");
    let pct = gtk::Label::new(Some(&format!("{v}%")));
    pct.add_css_class("value");
    pct.set_width_chars(5);
    pct.set_xalign(1.0);
    down.connect_clicked(|_| crate::osd::volume("volume-down"));
    up.connect_clicked(|_| crate::osd::volume("volume-up"));
    row.append(&l);
    row.append(&down);
    row.append(&bar);
    row.append(&up);
    row.append(&pct);
    VOLUME.with(|x| {
        x.borrow_mut()
            .push((pct.downgrade(), bar.downgrade(), None))
    });
    row
}

pub fn sound_page(tv: &Tv) {
    live_page(
        tv,
        "Sound",
        |_| (net::volume(), crate::audio::state()),
        |t, body, (vol, st): SoundState, reload| {
            if let Some((v, muted)) = vol {
                body.append(&volume_row(v, muted));
                let (m, mv) = ui::row("Mute", if muted { "On" } else { "Off" }, || {
                    crate::osd::volume("mute");
                });
                body.append(&m);
                VOLUME.with(|x| {
                    let mut x = x.borrow_mut();
                    x.retain(|w| w.0.upgrade().is_some());
                    if let Some(last) = x.last_mut() {
                        last.2 = Some(mv.downgrade());
                    }
                });
            } else {
                note_row(body, "No sound system found (WirePlumber)");
            }
            let pick = |kind: &str, devs: &[crate::audio::Device], input: bool| {
                if devs.is_empty() {
                    return;
                }
                let cur = devs.iter().find(|d| d.is_default).cloned();
                let opts: Vec<(String, String)> = devs
                    .iter()
                    .map(|d| (d.name.clone(), d.desc.clone()))
                    .collect();
                let (t2, r2, title) = (t.clone(), reload.clone(), kind.to_string());
                let (b, _) = ui::row(
                    kind,
                    &cur.as_ref().map(|d| d.desc.clone()).unwrap_or_default(),
                    move || {
                        let r = r2.clone();
                        let cur = cur.as_ref().map(|d| d.name.clone()).unwrap_or_default();
                        ui::choose(&t2, &title, opts.clone(), &cur, move |_t, name| {
                            let r = r.clone();
                            in_background(
                                move || {
                                    if input {
                                        crate::audio::set_input(&name)
                                    } else {
                                        crate::audio::set_output(&name)
                                    }
                                },
                                move |_| r(false),
                            );
                        });
                    },
                );
                body.append(&b);
            };
            pick("Output", &st.outputs, false);
            pick("Input", &st.inputs, true);
            for h in st.headsets {
                let r2 = reload.clone();
                let mic = h.mic_mode;
                let (b, _) = ui::row(
                    &format!("{} microphone", h.desc),
                    if mic { "On" } else { "Off" },
                    move || {
                        let (h2, r) = (h.clone(), r2.clone());
                        in_background(
                            move || crate::audio::headset_mode(&h2, !mic),
                            move |_| r(false),
                        );
                    },
                );
                body.append(&b);
            }
        },
    );
}

// ---------------------------------------------------------------- Screens

fn fit_name(f: &str) -> &'static str {
    match f {
        "fill" => "Fill",
        "exact" => "Sharp",
        _ => "Fit",
    }
}

pub fn display_page(tv: &Tv) {
    live_page(
        tv,
        "Screens",
        |_| screens::list(),
        |t, body, outs: Vec<screens::Output>, reload| {
            let cfg = t.cfg();
            let main = screens::main(&cfg, &outs);
            let active: Vec<&screens::Output> = outs.iter().filter(|o| o.active).collect();
            if active.is_empty() {
                note_row(body, "No screens found (sway isn't running)");
                return;
            }
            let label_of = |name: &str| {
                outs.iter()
                    .find(|o| o.name == name)
                    .map(|o| o.label.clone())
                    .unwrap_or_else(|| name.to_string())
            };
            // which screen RustTV is on (only with more than one)
            if active.len() > 1 {
                let opts: Vec<(String, String)> = std::iter::once((
                    String::new(),
                    "Automatic (the TV over a laptop's own screen)".to_string(),
                ))
                .chain(active.iter().map(|o| (o.name.clone(), o.label.clone())))
                .collect();
                let (t2, r2) = (t.clone(), reload.clone());
                let cur = cfg.main_output.clone();
                let shown = main.as_deref().map(label_of).unwrap_or_default();
                let (b, _) = ui::row("Main screen", &shown, move || {
                    let r = r2.clone();
                    ui::choose(&t2, "Main screen", opts.clone(), &cur, move |t, v| {
                        t.change(|c| c.main_output = v);
                        screens::restart(&t.cfg());
                        r(false);
                    });
                });
                body.append(&b);
                let (t2, r2) = (t.clone(), reload.clone());
                let cur = cfg.mirror_fit.clone();
                let (b, _) = ui::row("Other screens", fit_name(&cur), move || {
                    let r = r2.clone();
                    let opts = vec![
                        ("fit".into(), "Fit - all of it, with black bars".into()),
                        ("fill".into(), "Fill - edge to edge, edges cut off".into()),
                        (
                            "exact".into(),
                            "Sharp - whole-number size, black bars".into(),
                        ),
                    ];
                    ui::choose(&t2, "Other screens", opts, &cur, move |t, v| {
                        t.change(|c| c.mirror_fit = v);
                        screens::restart(&t.cfg());
                        r(false);
                    });
                });
                body.append(&b);
            }
            // the resolution of each screen
            body.append(&ui::heading("Resolution"));
            for o in active {
                if o.modes.is_empty() {
                    continue;
                }
                let cur = o.current.as_ref().map(|m| m.sway()).unwrap_or_default();
                let shown = o.current.as_ref().map(|m| m.label()).unwrap_or_default();
                let opts: Vec<(String, String)> =
                    o.modes.iter().map(|m| (m.sway(), m.label())).collect();
                let (t2, r2, name, title) =
                    (t.clone(), reload.clone(), o.name.clone(), o.label.clone());
                let (b, _) = ui::row(&o.label, &shown, move || {
                    let (r, name) = (r2.clone(), name.clone());
                    ui::choose(&t2, &title, opts.clone(), &cur, move |t, mode| {
                        crate::sway::run(&format!("output {name} mode {mode}"));
                        let (n2, m2) = (name.clone(), mode.clone());
                        t.change(|c| {
                            c.resolutions.retain(|(n, _)| *n != n2);
                            c.resolutions.push((n2, m2));
                        });
                        screens::restart(&t.cfg());
                        r(false);
                    });
                });
                body.append(&b);
            }
        },
    );
}

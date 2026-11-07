//! Pull-down "shade": time/date, toggles, sliders, power.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

use crate::clock;
use crate::gestures::{self, Spec};
use crate::icons::{Glyph, Icon};
use crate::netlist::{self, Kind};
use crate::shell::{self, Overlay, Shell};
use crate::sys;

struct Tile {
    button: gtk::ToggleButton,
    sub: gtk::Label,
    /// round frame around the button: everything drawn inside it (the
    /// theme's pressed look, the hold glow) is clipped to the rounded corners
    frame: gtk::Box,
}

fn tile_content(icon: Glyph, title: &str) -> (gtk::Box, gtk::Label) {
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let img = Icon::new(icon, 22).area;
    img.set_halign(gtk::Align::Start);
    let t = gtk::Label::new(Some(title));
    t.set_halign(gtk::Align::Start);
    t.add_css_class("qs-title");
    let sub = gtk::Label::new(None);
    sub.set_halign(gtk::Align::Start);
    sub.add_css_class("qs-sub");
    sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
    vb.append(&img);
    vb.append(&t);
    vb.append(&sub);
    (vb, sub)
}

fn toggle_tile(icon: Glyph, title: &str) -> Tile {
    let (content, sub) = tile_content(icon, title);
    let button = gtk::ToggleButton::new();
    button.set_child(Some(&content));
    button.add_css_class("qs-tile");
    button.set_hexpand(true);
    button.set_focusable(false);
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("qs-tile-frame");
    frame.set_overflow(gtk::Overflow::Hidden);
    frame.set_hexpand(true);
    frame.append(&button);
    Tile { button, sub, frame }
}

fn action_tile(icon: Glyph, title: &str, sub_text: &str) -> (gtk::Button, gtk::Label) {
    let (content, sub) = tile_content(icon, title);
    sub.set_text(sub_text);
    let b = gtk::Button::builder().child(&content).hexpand(true).build();
    b.add_css_class("qs-tile");
    b.set_focusable(false);
    (b, sub)
}

fn slider_row(icon: Glyph) -> (gtk::Box, gtk::Scale) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("qs-slider");
    let img = Icon::new(icon, 20).area;
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    row.append(&img);
    row.append(&scale);
    (row, scale)
}

/// Apply slider changes live while dragging, at most every ~60ms,
/// always ending on the final value.
fn throttle(scale: &gtk::Scale, guard: Rc<Cell<bool>>, f: fn(f64)) {
    let last = Rc::new(Cell::new(0i64));
    let pending = Rc::new(Cell::new(false));
    scale.connect_value_changed(move |s| {
        if guard.get() {
            return;
        }
        let now = glib::monotonic_time();
        if now - last.get() > 60_000 {
            last.set(now);
            f(s.value());
        } else if !pending.replace(true) {
            let (s2, last2, pending2) = (s.clone(), last.clone(), pending.clone());
            let _ = glib::timeout_add_local_once(Duration::from_millis(70), move || {
                pending2.set(false);
                last2.set(glib::monotonic_time());
                f(s2.value());
            });
        }
    });
}

fn power_content(icon: Glyph, text: &str) -> (gtk::Box, gtk::Label) {
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let img = Icon::new(icon, 22).area;
    img.set_halign(gtk::Align::Center);
    let lbl = gtk::Label::new(Some(text));
    lbl.add_css_class("qs-sub");
    vb.append(&img);
    vb.append(&lbl);
    (vb, lbl)
}

fn power_button(icon: Glyph, text: &str, action: impl Fn() + 'static) -> gtk::Button {
    let (vb, _) = power_content(icon, text);
    let b = gtk::Button::builder().child(&vb).hexpand(true).build();
    b.add_css_class("qs-tile");
    b.add_css_class("power-btn");
    b.set_focusable(false);
    b.connect_clicked(move |_| action());
    b
}

/// Destructive actions need two taps within 3s.
fn confirm_button(icon: Glyph, text: &'static str, action: fn()) -> gtk::Button {
    let (vb, lbl) = power_content(icon, text);
    let b = gtk::Button::builder().child(&vb).hexpand(true).build();
    b.add_css_class("qs-tile");
    b.add_css_class("power-btn");
    b.set_focusable(false);

    let armed = Rc::new(Cell::new(false));
    b.connect_clicked(move |_| {
        if armed.get() {
            action();
            return;
        }
        armed.set(true);
        lbl.set_text("Tap again");
        let (a, l) = (armed.clone(), lbl.clone());
        let _ = glib::timeout_add_local_once(Duration::from_secs(3), move || {
            a.set(false);
            l.set_text(text);
        });
    });
    b
}

/// Returns (whole sheet content, the sliding panel).
pub fn build(shell: &Shell) -> (gtk::Box, gtk::Box) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 14);
    panel.add_css_class("shade");
    panel.set_valign(gtk::Align::Start);
    root.append(&panel);

    // Tapping the dimmed area under the shade closes it.
    let filler = gtk::Box::new(gtk::Orientation::Vertical, 0);
    filler.set_vexpand(true);
    let click = gtk::GestureClick::new();
    let s = shell.clone();
    click.connect_released(move |_, _, _, _| s.hide_all());
    filler.add_controller(click);
    root.append(&filler);

    // ---- header: time / date / battery
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let tv = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let time = gtk::Label::new(None);
    time.add_css_class("qs-time");
    time.set_halign(gtk::Align::Start);
    clock::bind(&time, clock::Kind::Time);
    let date = gtk::Label::new(None);
    date.add_css_class("qs-date");
    date.set_halign(gtk::Align::Start);
    clock::bind(&date, clock::Kind::Date);
    tv.append(&time);
    tv.append(&date);
    header.append(&tv);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    header.append(&spacer);
    let batt = gtk::Label::new(None);
    batt.add_css_class("qs-batt");
    batt.set_valign(gtk::Align::End);
    header.append(&batt);
    // the shade has two pages: the normal one and the Wi-Fi/Bluetooth list
    let main = gtk::Box::new(gtk::Orientation::Vertical, 14);
    main.append(&header);

    // ---- toggles
    let syncing = Rc::new(Cell::new(false));
    let grid = gtk::Grid::new();
    grid.set_column_spacing(10);
    grid.set_row_spacing(10);
    grid.set_column_homogeneous(true);

    let wifi = toggle_tile(Glyph::Wifi(3), "Wi-Fi");
    let bt = toggle_tile(Glyph::Bluetooth, "Bluetooth");
    let rot = toggle_tile(Glyph::Rotate, "Rotate");
    let (settings, _) = action_tile(Glyph::Settings, "Settings", "");
    grid.attach(&wifi.frame, 0, 0, 1, 1);
    grid.attach(&bt.frame, 1, 0, 1, 1);
    grid.attach(&rot.frame, 2, 0, 1, 1);
    grid.attach(&settings, 3, 0, 1, 1);
    main.append(&grid);

    // apps running in the background (tray icons) - tap for their menu
    let bg_title = gtk::Label::new(Some("Running in the background"));
    bg_title.add_css_class("qs-section");
    bg_title.set_halign(gtk::Align::Start);
    let bg_apps = gtk::FlowBox::new();
    bg_apps.set_selection_mode(gtk::SelectionMode::None);
    bg_apps.set_max_children_per_line(6);
    bg_apps.set_min_children_per_line(1);
    bg_apps.set_halign(gtk::Align::Start); // chips only as wide as they need
    bg_apps.set_column_spacing(8);
    bg_apps.set_row_spacing(8);
    main.append(&bg_title);
    main.append(&bg_apps);

    // Wi-Fi / Bluetooth: apply + remember in rusttouch.conf
    let toggles: [(&gtk::ToggleButton, &'static str, fn(bool)); 2] = [
        (&wifi.button, "wifi", sys::set_wifi),
        (&bt.button, "bluetooth", sys::set_bluetooth),
    ];
    for (button, key, action) in toggles {
        let (s, g) = (shell.clone(), syncing.clone());
        button.connect_toggled(move |b| {
            if g.get() {
                return;
            }
            action(b.is_active());
            s.save(key, if b.is_active() { "on" } else { "off" });
            shell::refresh_soon(&s);
        });
    }

    // auto-rotate: follow the accelerometer (iio-sensor-proxy)
    {
        let (s, g, sub) = (shell.clone(), syncing.clone(), rot.sub.clone());
        rot.button.connect_toggled(move |b| {
            if g.get() {
                return;
            }
            let on = b.is_active();
            s.auto_rotate.set(on);
            s.save("auto_rotate", if on { "on" } else { "off" });
            sub.set_text(if on { "On" } else { "Locked" });
            if on {
                if let Some(o) = s.orientation() {
                    sys::rotate_to(&o);
                }
            }
        });
    }

    let s = shell.clone();
    settings.connect_clicked(move |_| {
        sys::open_settings();
        s.go_app();
    });

    // ---- sliders
    let (bright_row, bright) = slider_row(Glyph::Sun);
    let (vol_row, vol) = slider_row(Glyph::Volume);
    let sound_btn = gtk::Button::with_label("›");
    sound_btn.add_css_class("qs-round");
    sound_btn.set_focusable(false);
    sound_btn.set_tooltip_text(Some("Choose speakers / mic"));
    vol_row.append(&sound_btn);
    main.append(&bright_row);
    main.append(&vol_row);
    throttle(&bright, syncing.clone(), sys::set_brightness);
    throttle(&vol, syncing.clone(), sys::set_volume);
    // follow the hardware volume buttons while the shade is open
    {
        let (v2, g) = (vol.clone(), syncing.clone());
        shell.on_volume(move |level| {
            g.set(true);
            v2.set_value(level);
            g.set(false);
        });
    }

    // ---- power
    let power = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    power.set_homogeneous(true);
    let s = shell.clone();
    power.append(&power_button(Glyph::Lock, "Lock", move || {
        let _ = s.lock();
    }));
    let s = shell.clone();
    power.append(&power_button(Glyph::Moon, "Sleep", move || {
        s.run_power_action("sleep")
    }));
    power.append(&confirm_button(Glyph::Restart, "Restart", sys::reboot));
    power.append(&confirm_button(Glyph::Power, "Power off", sys::poweroff));
    power.append(&confirm_button(Glyph::Logout, "Log out", sys::logout));
    main.append(&power);

    let pages = gtk::Stack::new();
    pages.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    pages.set_transition_duration(180);
    pages.set_vhomogeneous(false);
    pages.set_interpolate_size(true);
    pages.add_named(&main, Some("main"));
    let p2 = pages.clone();
    let s = shell.clone();
    let s_back = shell.clone();
    let s_typing = shell.clone();
    let nets = netlist::build(
        move || {
            s_back.sheet_keyboard(shell::Overlay::Quick, false);
            p2.set_visible_child_name("main");
        },
        Rc::new(move || shell::refresh_soon(&s)),
        Rc::new(move |on: bool| s_typing.sheet_keyboard(shell::Overlay::Quick, on)),
    );
    pages.add_named(&nets.root, Some("list"));

    // a tray app's page: Open + the app's own menu
    let app_page = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let app_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let app_back = gtk::Button::with_label("‹");
    app_back.add_css_class("qs-round");
    app_back.set_focusable(false);
    let app_title = gtk::Label::new(None);
    app_title.add_css_class("qs-list-title");
    app_title.set_hexpand(true);
    app_title.set_halign(gtk::Align::Start);
    app_title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    app_header.append(&app_back);
    app_header.append(&app_title);
    let app_entries = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let app_scroll = gtk::ScrolledWindow::builder()
        .child(&app_entries)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(160)
        .max_content_height(360)
        .propagate_natural_height(true)
        .build();
    app_page.append(&app_header);
    app_page.append(&app_scroll);
    pages.add_named(&app_page, Some("app"));
    let p = pages.clone();
    app_back.connect_clicked(move |_| p.set_visible_child_name("main"));

    let show_app: Rc<dyn Fn(String)> = {
        let (s, p, title, entries) = (
            shell.clone(),
            pages.clone(),
            app_title.clone(),
            app_entries.clone(),
        );
        Rc::new(move |id: String| {
            let Some(tr) = s.tray() else { return };
            let name = crate::tray::apps(&tr)
                .into_iter()
                .find(|a| a.id == id)
                .map(|a| a.title)
                .unwrap_or_default();
            title.set_text(&name);
            while let Some(c) = entries.first_child() {
                entries.remove(&c);
            }
            let open = gtk::Button::with_label("Open");
            open.add_css_class("qs-menu-item");
            open.set_focusable(false);
            let (s2, tr2, id2) = (s.clone(), tr.clone(), id.clone());
            open.connect_clicked(move |_| {
                crate::tray::activate(&tr2, &id2);
                s2.hide_all();
            });
            entries.append(&open);
            if !crate::tray::has_menu(&tr, &id) {
                let l = gtk::Label::new(Some("This app has no menu"));
                l.add_css_class("qs-sub");
                entries.append(&l);
            }
            let (e2, s2, tr2, id2) = (entries.clone(), s.clone(), tr.clone(), id.clone());
            crate::tray::menu(&tr, &id, move |items| {
                for it in items {
                    if it.separator {
                        e2.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
                        continue;
                    }
                    let b = gtk::Button::with_label(&format!(
                        "{}{}",
                        "    ".repeat(it.depth),
                        it.label
                    ));
                    b.add_css_class("qs-menu-item");
                    b.set_focusable(false);
                    b.set_sensitive(it.enabled);
                    if let Some(l) = b.child().and_downcast::<gtk::Label>() {
                        l.set_xalign(0.0);
                    }
                    let (s3, tr3, id3, entry) = (s2.clone(), tr2.clone(), id2.clone(), it.id);
                    b.connect_clicked(move |_| {
                        crate::tray::click(&tr3, &id3, entry);
                        s3.hide_all();
                    });
                    e2.append(&b);
                }
            });
            p.set_visible_child_name("app");
        })
    };
    shell.set_tray_opener(show_app.clone());

    // keep the "Running in the background" row up to date
    if let Some(tr) = shell.tray() {
        let rebuild: Rc<dyn Fn()> = {
            let (tr, bg, title, show) = (
                tr.clone(),
                bg_apps.clone(),
                bg_title.clone(),
                show_app.clone(),
            );
            Rc::new(move || {
                while let Some(c) = bg.first_child() {
                    bg.remove(&c);
                }
                let apps = crate::tray::apps(&tr);
                title.set_visible(!apps.is_empty());
                bg.set_visible(!apps.is_empty());
                for a in apps {
                    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                    row.append(&crate::tray::image(&tr, &a.id, 20));
                    let l = gtk::Label::new(Some(&a.title));
                    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    l.set_max_width_chars(10);
                    row.append(&l);
                    let b = gtk::Button::builder().child(&row).build();
                    b.add_css_class("qs-bg-app");
                    b.set_focusable(false);
                    let (show, id) = (show.clone(), a.id.clone());
                    b.connect_clicked(move |_| show(id.clone()));
                    bg.insert(&b, -1);
                }
            })
        };
        rebuild();
        crate::tray::on_change(&tr, move || rebuild());
    } else {
        bg_title.set_visible(false);
        bg_apps.set_visible(false);
    }
    panel.append(&pages);

    // long-press Wi-Fi / Bluetooth -> network / device list
    for (tile, kind) in [(&wifi, Kind::Wifi), (&bt, Kind::Bluetooth)] {
        let button = &tile.button;
        let sub = tile.sub.clone();
        let saved: std::rc::Rc<std::cell::RefCell<String>> = Default::default();
        // The page only changes once the finger is lifted: swapping pages
        // while the touch is still on the button left GTK holding a touch
        // that never ended, and the pull-down stopped responding.
        let lp = gtk::GestureLongPress::new();
        let held = std::rc::Rc::new(std::cell::Cell::new(false));
        // where the finger is now (relative to the tile) and whether it's on it
        let inside = std::rc::Rc::new(std::cell::Cell::new(true));
        // feedback while holding: an outline grows in; once held long enough
        // the tile glows and says "Edit settings". Sliding off the tile takes
        // the glow away again, and letting go there does nothing.
        {
            let b = button.clone();
            lp.connect_begin(move |_, _| b.add_css_class("lp-pressing"));
        }
        let (h, b, sl, sv, ins) = (
            held.clone(),
            button.clone(),
            sub.clone(),
            saved.clone(),
            inside.clone(),
        );
        lp.connect_pressed(move |g, _, _| {
            let _ = g.set_state(gtk::EventSequenceState::Claimed); // don't toggle
            h.set(true);
            ins.set(true);
            b.add_css_class("lp-ready");
            *sv.borrow_mut() = sl.text().to_string();
            sl.set_text("Edit settings");
        });
        // the finger came up (or the touch was cancelled): open the list only
        // if it was lifted on the tile
        let finish: std::rc::Rc<dyn Fn(bool)> = {
            let (h, b, sl, sv, p, n) = (
                held.clone(),
                button.clone(),
                sub.clone(),
                saved.clone(),
                pages.clone(),
                nets.clone(),
            );
            std::rc::Rc::new(move |open: bool| {
                if !h.replace(false) {
                    return;
                }
                b.remove_css_class("lp-pressing");
                b.remove_css_class("lp-ready");
                sl.set_text(&sv.borrow());
                if open {
                    let (p, n) = (p.clone(), n.clone());
                    let _ = glib::idle_add_local_once(move || {
                        p.set_visible_child_name("list");
                        netlist::open(&n, kind);
                    });
                }
            })
        };
        // Follows the finger for the whole touch: a plain event watcher sees
        // the touch even after the long press has taken it, and it can't be
        // stopped by other gestures.
        let track = gtk::EventControllerLegacy::new();
        {
            let (h, ins, b, sl, sv, f) = (
                held.clone(),
                inside.clone(),
                button.clone(),
                sub.clone(),
                saved.clone(),
                finish.clone(),
            );
            track.connect_event(move |_, ev| {
                use gtk::gdk::EventType as T;
                let kind_ = ev.event_type();
                if !h.get()
                    || !matches!(
                        kind_,
                        T::MotionNotify
                            | T::TouchUpdate
                            | T::ButtonRelease
                            | T::TouchEnd
                            | T::TouchCancel
                    )
                {
                    return glib::Propagation::Proceed;
                }
                if kind_ == T::TouchCancel {
                    f(false);
                    return glib::Propagation::Proceed;
                }
                // where the finger is, in the tile's own coordinates
                let on = (|| {
                    let (x, y) = ev.position()?;
                    let native = b.native()?;
                    let (sx, sy) = native.surface_transform();
                    let p = native.compute_point(
                        &b,
                        &gtk::graphene::Point::new((x - sx) as f32, (y - sy) as f32),
                    )?;
                    let (x, y) = (f64::from(p.x()), f64::from(p.y()));
                    Some(
                        x >= 0.0
                            && y >= 0.0
                            && x <= f64::from(b.width())
                            && y <= f64::from(b.height()),
                    )
                })()
                .unwrap_or(ins.get());
                if matches!(kind_, T::ButtonRelease | T::TouchEnd) {
                    f(on);
                    return glib::Propagation::Proceed;
                }
                if on != ins.replace(on) {
                    if on {
                        b.add_css_class("lp-ready");
                        sl.set_text("Edit settings");
                    } else {
                        b.remove_css_class("lp-ready");
                        sl.set_text(&sv.borrow());
                    }
                }
                glib::Propagation::Proceed
            });
        }
        // the long press also "ends" when the finger moves a little (GTK
        // stops counting it as a long press) - that's not letting go, so the
        // watcher above decides. A long press that never got held just loses
        // its outline.
        let (b, f, ins, h) = (button.clone(), finish.clone(), inside.clone(), held.clone());
        lp.connect_end(move |g, _| {
            if !h.get() {
                b.remove_css_class("lp-pressing");
                return;
            }
            // (backup in case the watcher missed the release)
            let up = matches!(
                g.current_event().map(|e| e.event_type()),
                Some(gtk::gdk::EventType::ButtonRelease) | Some(gtk::gdk::EventType::TouchEnd)
            );
            if up {
                let (f, ins) = (f.clone(), ins.clone());
                let _ = glib::idle_add_local_once(move || f(ins.get()));
            }
        });
        button.add_controller(lp);
        button.add_controller(track);
    }
    let p = pages.clone();
    shell
        .quick
        .on_open(move || p.set_visible_child_name("main"));

    // › next to the volume slider: the Sound page
    let (p, n) = (pages.clone(), nets.clone());
    sound_btn.connect_clicked(move |_| {
        p.set_visible_child_name("list");
        netlist::open(&n, Kind::Audio);
    });

    let handle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    handle.add_css_class("handle");
    handle.set_halign(gtk::Align::Center);
    panel.append(&handle);

    // ---- keep network toggles in sync with reality
    {
        let g = syncing.clone();
        let (wb, wsub) = (wifi.button.clone(), wifi.sub.clone());
        let (bb, bsub) = (bt.button.clone(), bt.sub.clone());
        shell.on_status(move |st| {
            g.set(true);

            wb.set_active(st.wifi_on == Some(true));
            wb.set_sensitive(st.wifi_on.is_some());
            wsub.set_text(match (st.wifi_on, &st.ssid) {
                (Some(true), Some(n)) => n.as_str(),
                (Some(true), None) => "On",
                (Some(false), _) => "Off",
                (None, _) => "Unavailable",
            });

            bb.set_active(st.bt_on == Some(true));
            bb.set_sensitive(st.bt_on.is_some());
            bsub.set_text(match (st.bt_on, &st.bt_connected) {
                (Some(true), Some(name)) => name.as_str(),
                (Some(true), None) => "On",
                (Some(false), _) => "Off",
                (None, _) => "Unavailable",
            });

            batt.set_text(&match st.battery {
                Some((c, true)) => format!("{c}% ⚡"),
                Some((c, false)) => format!("{c}%"),
                None => String::new(),
            });

            g.set(false);
        });
    }

    // Everything else is synced when the shade opens (never fights a drag).
    {
        let s = shell.clone();
        let g = syncing.clone();
        let (rb, rsub) = (rot.button.clone(), rot.sub.clone());
        shell.quick.on_open(move || {
            g.set(true);
            match sys::brightness() {
                Some(v) => {
                    bright_row.set_visible(true);
                    bright.set_value(v);
                }
                None => bright_row.set_visible(false),
            }
            match s.status().volume {
                Some(_) => {
                    vol_row.set_visible(true);
                    vol.set_value(s.volume_level());
                }
                None => vol_row.set_visible(false),
            }

            rb.set_sensitive(s.has_accel.get());
            rb.set_active(s.auto_rotate.get());
            rsub.set_text(if !s.has_accel.get() {
                "No sensor"
            } else if s.auto_rotate.get() {
                "On"
            } else {
                "Locked"
            });

            g.set(false);
            shell::refresh_status(&s);
        });
    }

    // Drag up anywhere on the shade closes it with the finger. Attached to
    // the sheet's stage, which doesn't move, so the drag can't jitter.
    let s = shell.clone();
    let p = pages.clone();
    gestures::attach(
        &shell.quick.stage(),
        Spec {
            pick: Box::new(move |_dx: f64, dy: f64| {
                // on the list page, vertical drags scroll the list
                if dy < 0.0 && p.visible_child_name().as_deref() == Some("main") {
                    s.target(Overlay::Quick, 1.0, 1.0)
                } else {
                    None
                }
            }),
            tap: None,
        },
    );

    (root, panel)
}

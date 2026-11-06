//! Top status bar and bottom 3-button navigation bar.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::gestures::{self, Spec};
use crate::icons::{Glyph, Icon};
use crate::shell::{self, Overlay, Shell};
use crate::{clock, sys};

/// The status row: clock, (tray), Bluetooth, Wi-Fi, battery. Used by the
/// top bar and, identically, as the fixed header of the pull-down.
pub fn status_row(shell: &Shell, with_tray: bool) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar.add_css_class("topbar");

    let time = gtk::Label::new(None);
    time.add_css_class("bar-clock");
    clock::bind(&time, clock::Kind::Bar);
    bar.append(&time);

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    bar.append(&spacer);

    // system tray (apps like rustsync-gui --tray), created by the shell
    if with_tray {
        if let Some(area) = shell.tray_area() {
            bar.append(&area);
        }
    }

    let bt = Icon::new(Glyph::Bluetooth, 16);
    let wifi = Icon::new(Glyph::Wifi(0), 16);
    let batt_icon = Icon::new(Glyph::Battery(0, false), 22);
    let batt = gtk::Label::new(None);
    batt.add_css_class("bar-batt");
    bar.append(&bt.area);
    bar.append(&wifi.area);
    bar.append(&batt_icon.area);
    bar.append(&batt);

    shell.on_status(move |st| {
        bt.area.set_visible(st.bt_on == Some(true));

        wifi.area.set_visible(st.wifi_on.is_some());
        wifi.set(Glyph::Wifi(match (st.wifi_on, st.ssid.is_some()) {
            (Some(true), true) => 3,
            (Some(true), false) => 1,
            _ => 0,
        }));

        match st.battery {
            Some((pct, charging)) => {
                batt_icon.set(Glyph::Battery(pct, charging));
                batt.set_text(&format!("{pct}%"));
                batt_icon.area.set_visible(true);
                batt.set_visible(true);
            }
            None => {
                batt_icon.area.set_visible(false);
                batt.set_visible(false);
            }
        }
    });
    bar
}

pub fn topbar(shell: &Shell) -> gtk::Box {
    let bar = status_row(shell, true);

    // pull down to open quick settings (follows the finger), tap to toggle
    let (s1, s2) = (shell.clone(), shell.clone());
    gestures::attach(
        &bar,
        Spec {
            pick: Box::new(move |_dx: f64, dy: f64| {
                if dy > 0.0 {
                    s1.target(Overlay::Quick, 1.0, 0.0)
                } else {
                    None
                }
            }),
            tap: Some(Box::new(move || s2.toggle(Overlay::Quick))),
        },
    );
    dismiss_on_touch(shell, &bar);

    // Pulling the shade down fades the bar away to nothing - everything on
    // it is in the shade, no need to see it twice.
    let b = bar.clone();
    shell
        .quick
        .on_progress(move |p| b.set_opacity((1.0 - p * 1.6).clamp(0.0, 1.0)));
    bar
}

/// Touching rusttouch's own surfaces puts the on-screen keyboard away.
pub fn dismiss_on_touch(shell: &Shell, w: &impl IsA<gtk::Widget>) {
    let c = gtk::GestureClick::new();
    c.set_propagation_phase(gtk::PropagationPhase::Capture);
    let s = shell.clone();
    c.connect_pressed(move |_, _, _, _| s.dismiss_keyboard());
    w.add_controller(c);
}

fn nav_button(glyph: &str) -> gtk::Button {
    let b = gtk::Button::with_label(glyph);
    b.set_halign(gtk::Align::Center);
    b.set_focusable(false);
    b
}

pub fn navbar(shell: &Shell) -> gtk::Box {
    // [slot] [ ◀  ●  ■ ] [⌨] - the empty slot on the left keeps ◀ ● ■ centered
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.add_css_class("navbar");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    buttons.set_homogeneous(true);
    buttons.set_hexpand(true);

    let back = nav_button("◀");
    let home = nav_button("●");
    let recents = nav_button("■");

    // tap = back (Alt+Left / hide keyboard / close sheet), long-press = Escape
    let long_fired = Rc::new(Cell::new(false));
    let lp = gtk::GestureLongPress::new();
    let lf = long_fired.clone();
    lp.connect_pressed(move |_, _, _| {
        lf.set(true);
        sys::send_keys("Escape");
    });
    back.add_controller(lp);

    // Android-style: while the keyboard is up, Back shows ⌄ and hides it
    let b2 = back.clone();
    // Back shows what it will do: ⌄ hide keyboard, ▼ close the app drawer,
    // ▲ close the pull-down, ◀ go back
    let update: Rc<dyn Fn()> = {
        let s = shell.clone();
        Rc::new(move || {
            let glyph = if s.keyboard_up() {
                "⌄"
            } else if s.drawer.is_open() && s.drawer.progress() > 0.3 {
                "▼"
            } else if s.quick.is_open() && s.quick.progress() > 0.3 {
                "▲"
            } else {
                "◀"
            };
            if b2.label().as_deref() != Some(glyph) {
                b2.set_label(glyph);
            }
        })
    };
    let u = update.clone();
    shell.on_keyboard(move |_| u());
    let u = update.clone();
    shell.drawer.on_progress(move |_| u());
    let u = update.clone();
    shell.quick.on_progress(move |_| u());

    let s = shell.clone();
    back.connect_clicked(move |_| {
        if long_fired.replace(false) {
            return;
        }
        s.back();
    });

    let s = shell.clone();
    home.connect_clicked(move |_| shell::go_home(&s));

    let s = shell.clone();
    recents.connect_clicked(move |_| shell::toggle_recents(&s));

    buttons.append(&back);
    buttons.append(&home);
    buttons.append(&recents);

    // keyboard: open it by hand, close it again (⌃ / ⌄)
    let left_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    left_slot.add_css_class("nav-slot");
    let kb = gtk::Button::with_label("⌨");
    kb.add_css_class("nav-kb");
    kb.set_focusable(false);
    kb.set_valign(gtk::Align::Center);
    kb.set_tooltip_text(Some("Keyboard"));
    let s = shell.clone();
    kb.connect_clicked(move |_| {
        let up = s.keyboard_up();
        if !up {
            s.hide_all();
        }
        s.show_keyboard(!up);
    });
    let kb2 = kb.clone();
    // while the keyboard is up, Back (⌄) closes it - no need for this button
    shell.on_keyboard(move |up| kb2.set_opacity(if up { 0.0 } else { 1.0 }));
    let kb3 = kb.clone();
    shell.on_keyboard(move |up| kb3.set_sensitive(!up));
    let has_kb = shell.keyboard != crate::sys::Osk::Missing;
    left_slot.set_visible(has_kb);
    kb.set_visible(has_kb);
    bar.append(&left_slot);
    bar.append(&buttons);
    bar.append(&kb);

    // swipe up from the nav bar pulls up the app drawer over anything
    let s = shell.clone();
    gestures::attach(
        &bar,
        Spec {
            pick: Box::new(move |_dx: f64, dy: f64| {
                if dy < 0.0 {
                    s.target(Overlay::Drawer, -1.0, 0.0)
                } else {
                    None
                }
            }),
            tap: None,
        },
    );
    bar
}

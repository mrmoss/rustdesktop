//! The battery (laptops and the like used as a TV box): its level at the
//! top of the home screen, and a warning over everything when it runs low
//! (Settings > Battery). Nothing shows without a battery.

use crate::Tv;
use gtk::glib;
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    /// 0..100
    pub pct: u32,
    /// on the charger (charging, or full)
    pub plugged: bool,
}

/// The battery now (None: there isn't one). Several are averaged.
pub fn read() -> Option<State> {
    let mut levels = Vec::new();
    let mut plugged = false;
    for e in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        let p = e.path();
        let get = |f: &str| {
            std::fs::read_to_string(p.join(f))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        match get("type").as_str() {
            // (a mouse's or a remote's battery says scope=Device: not ours)
            "Battery" if get("scope") != "Device" => {
                if let Ok(c) = get("capacity").parse::<u32>() {
                    levels.push(c.min(100));
                }
                if matches!(get("status").as_str(), "Charging" | "Full" | "Not charging") {
                    plugged = true;
                }
            }
            "Mains" | "USB" if get("online") == "1" => plugged = true,
            _ => {}
        }
    }
    if levels.is_empty() {
        return None;
    }
    let pct = levels.iter().sum::<u32>() / levels.len() as u32;
    Some(State { pct, plugged })
}

/// The battery at the top of the home screen: a little drawing and the %.
pub struct Indicator {
    pub root: gtk::Box,
    draw: gtk::DrawingArea,
    label: gtk::Label,
    state: Rc<Cell<Option<State>>>,
    low: Rc<Cell<u32>>,
}

pub fn indicator() -> Indicator {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    root.add_css_class("battery");
    root.set_valign(gtk::Align::Center);
    let draw = gtk::DrawingArea::new();
    draw.set_content_width(38);
    draw.set_content_height(20);
    let label = gtk::Label::new(None);
    root.append(&draw);
    root.append(&label);
    let state: Rc<Cell<Option<State>>> = Rc::new(Cell::new(None));
    let low = Rc::new(Cell::new(15u32));
    let (st, lw) = (state.clone(), low.clone());
    draw.set_draw_func(move |_, cr, w, h| {
        let Some(s) = st.get() else { return };
        let (w, h) = (w as f64, h as f64);
        let tip = 4.0;
        let body = w - tip - 1.0;
        // outline, and the tip
        cr.set_source_rgba(0.95, 0.95, 0.96, 0.9);
        cr.set_line_width(2.0);
        cr.rectangle(1.0, 1.0, body - 1.0, h - 2.0);
        let _ = cr.stroke();
        cr.rectangle(body, h * 0.3, tip, h * 0.4);
        let _ = cr.fill();
        // the level (red when low and not charging)
        if s.pct <= lw.get() && !s.plugged {
            cr.set_source_rgb(0.95, 0.27, 0.25);
        } else if s.plugged {
            cr.set_source_rgb(0.35, 0.85, 0.45);
        } else {
            cr.set_source_rgba(0.95, 0.95, 0.96, 0.9);
        }
        let inner = (body - 6.0) * s.pct as f64 / 100.0;
        cr.rectangle(4.0, 4.0, inner.max(1.0), h - 8.0);
        let _ = cr.fill();
    });
    Indicator {
        root,
        draw,
        label,
        state,
        low,
    }
}

impl Indicator {
    pub fn update(&self, s: Option<State>, show: bool, low: u32) {
        self.low.set(low);
        self.state.set(s);
        match s.filter(|_| show) {
            Some(s) => {
                self.label
                    .set_text(&format!("{}{}%", if s.plugged { "⚡ " } else { "" }, s.pct));
                self.root.set_visible(true);
                self.draw.queue_draw();
            }
            None => self.root.set_visible(false),
        }
    }
}

thread_local! {
    /// the warnings given while it ran down (again after it's charged)
    static WARNED: Cell<(bool, bool)> = const { Cell::new((false, false)) };
}

/// Look at the battery: redraw the level, warn when it's low.
pub fn check(t: &Tv) {
    let s = read();
    let cfg = t.cfg();
    if let Some(h) = t.home.borrow().as_ref() {
        crate::home::battery(h, s, cfg.battery_show, cfg.battery_warn);
    }
    let Some(s) = s else { return };
    let warn = cfg.battery_warn;
    if s.plugged || warn == 0 || s.pct > warn + 2 {
        WARNED.with(|w| w.set((false, false)));
        return;
    }
    let (low, very) = WARNED.with(|w| w.get());
    let critical = s.pct <= 5.min(warn);
    if critical && !very {
        WARNED.with(|w| w.set((true, true)));
        crate::osd::message(
            t,
            "🪫",
            &format!("Battery very low: {}% - plug it in", s.pct),
        );
    } else if s.pct <= warn && !low {
        WARNED.with(|w| w.set((true, very)));
        crate::osd::message(t, "🪫", &format!("Battery low: {}%", s.pct));
    }
}

/// Look every 30 seconds.
pub fn watch(t: &Tv) {
    check(t);
    let w = Rc::downgrade(t);
    glib::timeout_add_seconds_local(30, move || match w.upgrade() {
        Some(t) => {
            check(&t);
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });
}

//! Clock labels. Formats come from rusttouch.conf (12/24 hour, top-bar
//! style, date style) and update live when the settings change.

use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;

use crate::config::Config;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// top bar: time, optionally with day/date
    Bar,
    /// a plain time (quick settings, lock screen)
    Time,
    /// the date (quick settings, lock screen)
    Date,
}

struct Prefs {
    h24: bool,
    bar: String,
    date: String,
}

thread_local! {
    static PREFS: RefCell<Prefs> = RefCell::new(Prefs {
        h24: false,
        bar: "time".into(),
        date: "full".into(),
    });
    static LABELS: RefCell<Vec<(glib::WeakRef<gtk::Label>, Kind)>> = const { RefCell::new(Vec::new()) };
}

/// strftime-style format for `kind` with the current settings.
fn format(kind: Kind) -> String {
    PREFS.with(|p| {
        let p = p.borrow();
        let time = if p.h24 { "%H:%M" } else { "%-I:%M %p" };
        let date = match p.date.as_str() {
            "long" => "%A, %B %-e",
            "short" => "%a, %b %-e",
            "numeric" => "%m/%d/%Y",
            "iso" => "%Y-%m-%d",
            _ => "%A %B %-e, %Y", // full: Thursday November 5, 2026
        };
        match kind {
            Kind::Time => time.to_string(),
            Kind::Date => date.to_string(),
            Kind::Bar => match p.bar.as_str() {
                "day_time" => format!("%a  {time}"),
                "date_time" => format!("%a, %b %-e  {time}"),
                _ => time.to_string(),
            },
        }
    })
}

fn update(label: &gtk::Label, kind: Kind) {
    if let Ok(now) = glib::DateTime::now_local() {
        if let Ok(s) = now.format(&format(kind)) {
            label.set_text(&s);
        }
    }
}

fn update_all() {
    LABELS.with(|l| {
        let mut l = l.borrow_mut();
        l.retain(|(w, _)| w.upgrade().is_some());
        for (w, k) in l.iter() {
            if let Some(label) = w.upgrade() {
                update(&label, *k);
            }
        }
    });
}

/// Read the clock settings (at startup and whenever the config changes).
pub fn set_prefs(cfg: &Config) {
    PREFS.with(|p| {
        *p.borrow_mut() = Prefs {
            h24: cfg.on("time_24h"),
            bar: cfg.get("bar_clock"),
            date: cfg.get("date_format"),
        };
    });
    update_all();
}

/// Keep `label` showing the time/date.
pub fn bind(label: &gtk::Label, kind: Kind) {
    update(label, kind);
    let first = LABELS.with(|l| {
        let mut l = l.borrow_mut();
        l.push((label.downgrade(), kind));
        l.len() == 1
    });
    if first {
        // one shared timer for every clock label
        let _ = glib::timeout_add_seconds_local(1, || {
            update_all();
            glib::ControlFlow::Continue
        });
    }
}

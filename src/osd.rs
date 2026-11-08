//! Small volume/brightness popup at the top of the screen. While it shows,
//! you can slide a finger along it to set the level.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

use crate::icons::{Glyph, Icon};
use crate::layer::{self, Anchors, KeyboardMode, Layer, Zone};

/// Called with (volume or brightness, 0..100) when the popup is slid.
type OnSet = Rc<RefCell<Option<Rc<dyn Fn(Glyph, f64)>>>>;

pub struct Osd {
    window: Option<gtk::ApplicationWindow>,
    icon: Icon,
    bar: gtk::ProgressBar,
    label: gtk::Label,
    gen: Rc<Cell<u32>>,
    glyph: Rc<Cell<Glyph>>,
    on_set: OnSet,
}

const SHOW_MS: u64 = 1500;

/// Hide `w` after a while, unless something showed it again meanwhile.
fn hide_later(w: &gtk::ApplicationWindow, gen: &Rc<Cell<u32>>) {
    hide_after(w, gen, SHOW_MS);
}

fn hide_after(w: &gtk::ApplicationWindow, gen: &Rc<Cell<u32>>, ms: u64) {
    let g = gen.get().wrapping_add(1);
    gen.set(g);
    let (gen, w) = (gen.clone(), w.clone());
    let _ = glib::timeout_add_local_once(Duration::from_millis(ms), move || {
        if gen.get() == g {
            w.set_visible(false);
        }
    });
}

impl Osd {
    pub fn new(app: &gtk::Application, layered: bool) -> Osd {
        let icon = Icon::new(Glyph::Volume, 22);
        let bar = gtk::ProgressBar::new();
        bar.set_hexpand(true);
        bar.set_valign(gtk::Align::Center);
        let label = gtk::Label::new(None);
        label.set_justify(gtk::Justification::Center);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("osd");
        row.set_size_request(300, -1);
        row.append(&icon.area);
        row.append(&bar);
        row.append(&label);
        let gen = Rc::new(Cell::new(0u32));
        let glyph = Rc::new(Cell::new(Glyph::Volume));
        let on_set: OnSet = Rc::new(RefCell::new(None));

        let window = layered.then(|| {
            let w = gtk::ApplicationWindow::builder()
                .application(app)
                .title("rusttouch osd")
                .build();
            w.add_css_class("rusttouch-transparent");
            let top_only = Anchors {
                top: true,
                bottom: false,
                left: false,
                right: false,
            };
            layer::setup(
                &w,
                Layer::Overlay,
                top_only,
                Zone::Fixed(0),
                KeyboardMode::None,
            );
            layer::set_margins(&w, 56, 0, 0, 0);
            w.set_child(Some(&row));
            w
        });

        // slide (or tap) along the popup: set the level where the finger is
        if let Some(w) = &window {
            let drag = gtk::GestureDrag::new();
            let start = Rc::new(Cell::new(0.0f64));
            let last_sent = Rc::new(Cell::new(0i64));
            let set_at = {
                let (bar, row, gen, glyph, on_set, last_sent) = (
                    bar.clone(),
                    row.clone(),
                    gen.clone(),
                    glyph.clone(),
                    on_set.clone(),
                    last_sent.clone(),
                );
                Rc::new(move |x: f64, force: bool| {
                    if !bar.is_visible() {
                        return; // showing "Mute"
                    }
                    // stay up while the finger is on it
                    gen.set(gen.get().wrapping_add(1));
                    let p = row
                        .compute_point(&bar, &gtk::graphene::Point::new(x as f32, 0.0))
                        .map(|p| f64::from(p.x()))
                        .unwrap_or(0.0);
                    let frac = (p / f64::from(bar.width().max(1))).clamp(0.0, 1.0);
                    bar.set_fraction(frac);
                    // the volume/backlight tools are run at most every 60ms
                    let now = glib::monotonic_time();
                    if force || now - last_sent.get() > 60_000 {
                        last_sent.set(now);
                        if let Some(f) = on_set.borrow().clone() {
                            f(glyph.get(), (frac * 100.0).round());
                        }
                    }
                })
            };
            {
                let (start, set_at) = (start.clone(), set_at.clone());
                drag.connect_drag_begin(move |g, x, _| {
                    let _ = g.set_state(gtk::EventSequenceState::Claimed);
                    start.set(x);
                    set_at(x, false);
                });
            }
            {
                let (start, set_at) = (start.clone(), set_at.clone());
                drag.connect_drag_update(move |_, dx, _| set_at(start.get() + dx, false));
            }
            {
                let (w, gen) = (w.clone(), gen.clone());
                drag.connect_drag_end(move |_, dx, _| {
                    set_at(start.get() + dx, true);
                    hide_later(&w, &gen);
                });
            }
            row.add_controller(drag);
        }
        Osd {
            window,
            icon,
            bar,
            label,
            gen,
            glyph,
            on_set,
        }
    }

    /// What sliding the popup does: `f(Glyph::Volume or Glyph::Sun, 0..100)`.
    pub fn on_set(&self, f: impl Fn(Glyph, f64) + 'static) {
        *self.on_set.borrow_mut() = Some(Rc::new(f));
    }

    /// Show for ~1.5s. `label` replaces the bar when not empty (e.g. "Mute").
    pub fn show(&self, glyph: Glyph, fraction: f64, label: &str) {
        self.show_for(glyph, fraction, label, SHOW_MS);
    }

    /// Like show(), for `ms` milliseconds.
    pub fn show_for(&self, glyph: Glyph, fraction: f64, label: &str, ms: u64) {
        let Some(w) = &self.window else { return };
        self.glyph.set(glyph);
        self.icon.set(glyph);
        self.bar.set_fraction(fraction.clamp(0.0, 1.0));
        self.bar.set_visible(label.is_empty());
        self.label.set_text(label);
        self.label.set_visible(!label.is_empty());
        w.present();
        hide_after(w, &self.gen, ms);
    }
}

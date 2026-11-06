//! Small volume/brightness popup at the top of the screen.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

use crate::icons::{Glyph, Icon};
use crate::layer::{self, Anchors, KeyboardMode, Layer, Zone};

pub struct Osd {
    window: Option<gtk::ApplicationWindow>,
    icon: Icon,
    bar: gtk::ProgressBar,
    label: gtk::Label,
    gen: Rc<Cell<u32>>,
}

impl Osd {
    pub fn new(app: &gtk::Application, layered: bool) -> Osd {
        let icon = Icon::new(Glyph::Volume, 22);
        let bar = gtk::ProgressBar::new();
        bar.set_hexpand(true);
        bar.set_valign(gtk::Align::Center);
        let label = gtk::Label::new(None);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("osd");
        row.set_size_request(260, -1);
        row.append(&icon.area);
        row.append(&bar);
        row.append(&label);

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
            w.set_can_target(false);
            w
        });
        Osd {
            window,
            icon,
            bar,
            label,
            gen: Rc::new(Cell::new(0)),
        }
    }

    /// Show for ~1.5s. `label` replaces the bar when not empty (e.g. "Mute").
    pub fn show(&self, glyph: Glyph, fraction: f64, label: &str) {
        let Some(w) = &self.window else { return };
        self.icon.set(glyph);
        self.bar.set_fraction(fraction.clamp(0.0, 1.0));
        self.bar.set_visible(label.is_empty());
        self.label.set_text(label);
        self.label.set_visible(!label.is_empty());
        w.present();
        let gen = self.gen.get().wrapping_add(1);
        self.gen.set(gen);
        let (g, w2) = (self.gen.clone(), w.clone());
        let _ = glib::timeout_add_local_once(Duration::from_millis(1500), move || {
            if g.get() == gen {
                w2.set_visible(false);
            }
        });
    }
}

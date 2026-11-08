//! A "sheet" is a panel that slides in from the top or bottom edge and can be
//! dragged with a finger (app drawer, quick settings, recents).
//! In layer-shell mode each sheet is its own Overlay-layer window; in windowed
//! mode it is a hidden child of the main window's GtkOverlay.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

use crate::slide::Slide;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    FromBottom,
    FromTop,
}

pub struct SheetInner {
    window: Option<gtk::ApplicationWindow>,
    root: gtk::Overlay,
    slot: gtk::Box,
    slide: Slide,
    scrim: gtk::Box,
    panel: RefCell<Option<gtk::Widget>>,
    dir: Dir,
    progress: Cell<f64>,
    last_travel: Cell<f64>,
    shown: Cell<bool>,
    anim: RefCell<Option<gtk::TickCallbackId>>,
    open_hooks: RefCell<Vec<Box<dyn Fn()>>>,
    progress_hooks: RefCell<Vec<Box<dyn Fn(f64)>>>,
}

pub type Sheet = Rc<SheetInner>;

pub fn new(dir: Dir, window: Option<gtk::ApplicationWindow>) -> Sheet {
    let scrim = gtk::Box::new(gtk::Orientation::Vertical, 0);
    scrim.add_css_class("scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_opacity(0.0);

    let slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
    slot.set_hexpand(true);
    slot.set_vexpand(true);

    let slide = Slide::new(&slot);
    slide.set_hexpand(true);
    slide.set_vexpand(true);
    slide.set_overflow(gtk::Overflow::Hidden);

    let root = gtk::Overlay::new();
    root.set_child(Some(&scrim));
    root.add_overlay(&slide);

    match &window {
        Some(w) => w.set_child(Some(&root)),
        None => root.set_visible(false),
    }

    Rc::new(SheetInner {
        window,
        root,
        slot,
        slide,
        scrim,
        panel: RefCell::new(None),
        dir,
        progress: Cell::new(0.0),
        last_travel: Cell::new(0.0),
        shown: Cell::new(false),
        anim: RefCell::new(None),
        open_hooks: RefCell::new(Vec::new()),
        progress_hooks: RefCell::new(Vec::new()),
    })
}

impl SheetInner {
    /// `panel` is the part that actually moves in/out (its height = drag distance).
    pub fn set_content(&self, content: &impl IsA<gtk::Widget>, panel: &impl IsA<gtk::Widget>) {
        content.set_hexpand(true);
        content.set_vexpand(true);
        self.slot.append(content);
        *self.panel.borrow_mut() = Some(panel.clone().upcast::<gtk::Widget>());
    }

    pub fn window(&self) -> Option<&gtk::ApplicationWindow> {
        self.window.as_ref()
    }

    /// The widget to put in the main window when there is no layer-shell.
    pub fn embedded(&self) -> Option<gtk::Widget> {
        if self.window.is_none() {
            Some(self.root.clone().upcast())
        } else {
            None
        }
    }

    pub fn is_open(&self) -> bool {
        self.shown.get()
    }

    pub fn progress(&self) -> f64 {
        self.progress.get()
    }

    /// The part of the sheet that never moves. Attach drag gestures here:
    /// coordinates measured on the moving panel would shift under the
    /// finger and make the drag jitter.
    pub fn stage(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// Pixels the finger must travel to fully open/close the sheet. Stays
    /// the same for the whole drag, even before the panel is laid out.
    pub fn travel(&self) -> f64 {
        let panel = self.panel.borrow();
        let h = panel.as_ref().map(|p| p.height()).unwrap_or(0);
        if h > 50 {
            self.last_travel.set(f64::from(h));
            return f64::from(h);
        }
        if self.last_travel.get() > 50.0 {
            return self.last_travel.get();
        }
        // first time: ask the panel how tall it wants to be
        let (_, _, sw, sh) = crate::shell::screen_size();
        let t = match (self.dir, panel.as_ref()) {
            (Dir::FromTop, Some(p)) => f64::from(p.measure(gtk::Orientation::Vertical, sw).1),
            _ => f64::from(sh) * 0.8,
        };
        if t > 50.0 {
            t
        } else {
            f64::from(sh) * 0.8
        }
    }

    fn apply(&self, p: f64) {
        self.progress.set(p);
        let off = (1.0 - p) * self.travel();
        self.slide.set_offset(if self.dir == Dir::FromBottom {
            off
        } else {
            -off
        });
        self.scrim.set_opacity(0.55 * p);
        for f in self.progress_hooks.borrow().iter() {
            f(p);
        }
    }

    /// Run `f(progress 0..1)` every time the sheet moves (each frame).
    pub fn on_progress(&self, f: impl Fn(f64) + 'static) {
        self.progress_hooks.borrow_mut().push(Box::new(f));
    }

    fn stop_anim(&self) {
        let id = self.anim.borrow_mut().take();
        if let Some(id) = id {
            id.remove();
        }
    }

    /// Make the sheet see-through while an app is dragged out of it.
    pub fn set_ghost(&self, on: bool) {
        self.slide.set_opacity(if on { 0.0 } else { 1.0 });
        self.scrim
            .set_opacity(if on { 0.0 } else { 0.55 * self.progress.get() });
    }

    fn map(&self) {
        if self.shown.replace(true) {
            return;
        }
        self.slide.set_opacity(1.0);
        for h in self.open_hooks.borrow().iter() {
            h();
        }
        self.apply(0.0);
        match &self.window {
            Some(w) => {
                w.present();
                // don't focus the first entry (would pop up the keyboard)
                GtkWindowExt::set_focus(w, None::<&gtk::Widget>);
            }
            None => self.root.set_visible(true),
        }
    }

    fn unmap(&self) {
        self.shown.set(false);
        match &self.window {
            Some(w) => w.set_visible(false),
            None => self.root.set_visible(false),
        }
    }

    /// Run `f` every time the sheet starts to appear.
    pub fn on_open(&self, f: impl Fn() + 'static) {
        self.open_hooks.borrow_mut().push(Box::new(f));
    }

    pub fn begin_drag(&self) {
        self.stop_anim();
        self.map();
    }

    pub fn drag(&self, p: f64) {
        self.apply(p.clamp(0.0, 1.0));
    }
}

/// Animate to fully open (1.0) or closed (0.0). Short distances finish faster.
pub fn animate_to(sheet: &Sheet, target: f64) {
    sheet.stop_anim();
    let from = sheet.progress.get();
    let dist = (target - from).abs();
    if dist < 0.001 {
        sheet.apply(target);
        if target <= 0.0 {
            sheet.unmap();
        }
        return;
    }
    let ms = (110.0 + 170.0 * dist) as u32;
    let s = sheet.clone();
    let s2 = sheet.clone();
    let id = crate::anim::run(
        &sheet.slide,
        ms,
        move |e| s.apply(from + (target - from) * e),
        move || {
            s2.anim.borrow_mut().take();
            if target <= 0.0 {
                s2.unmap();
            }
        },
    );
    *sheet.anim.borrow_mut() = Some(id);
}

pub fn open(sheet: &Sheet) {
    sheet.begin_drag();
    animate_to(sheet, 1.0);
}

pub fn close(sheet: &Sheet) {
    if sheet.is_open() {
        animate_to(sheet, 0.0);
    }
}

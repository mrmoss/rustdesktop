//! Tiny frame-clock animation helper.

use std::cell::{Cell, RefCell};

use gtk::glib;
use gtk::prelude::*;

/// Fast start, gentle stop - feels "physical" for things that slide in/out.
pub fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

/// Call `step(eased progress 0..1)` once per frame for `ms`, then `done`.
/// Runs on the widget's frame clock, so it is vsync'd and stops if unmapped.
pub fn run(
    widget: &impl IsA<gtk::Widget>,
    ms: u32,
    step: impl Fn(f64) + 'static,
    done: impl FnOnce() + 'static,
) -> gtk::TickCallbackId {
    let start = Cell::new(None::<i64>);
    let done: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(Some(Box::new(done)));
    let dur = f64::from(ms.max(1)) * 1000.0;
    widget.add_tick_callback(move |_, clock| {
        let now = clock.frame_time();
        let t0 = match start.get() {
            Some(t) => t,
            None => {
                start.set(Some(now));
                now
            }
        };
        let t = ((now - t0) as f64 / dur).clamp(0.0, 1.0);
        step(ease_out(t));
        if t >= 1.0 {
            let d = done.borrow_mut().take();
            if let Some(d) = d {
                d();
            }
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    })
}

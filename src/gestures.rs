//! Finger-tracking drags for sheets. While you drag, the sheet follows your
//! finger 1:1; on release it settles open or closed based on how far you went
//! and how fast you flicked. Works with touch and mouse.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::sheet::{self, Sheet};

/// Which sheet a drag controls.
/// progress = from + sign * dy / travel   (sign -1: dragging up opens)
pub struct Target {
    pub sheet: Sheet,
    pub sign: f64,
    pub from: f64,
}

pub struct Spec {
    /// Called once the drag is clearly vertical. Return None to let the
    /// widget underneath handle it (scrolling, sliders...).
    pub pick: Box<dyn Fn(f64, f64) -> Option<Target>>,
    pub tap: Option<Box<dyn Fn()>>,
}

struct State {
    spec: Spec,
    decided: Cell<bool>,
    target: RefCell<Option<Target>>,
    samples: RefCell<Vec<(i64, f64)>>,
}

/// Movement (px) before we decide what kind of drag this is.
const SLOP: f64 = 10.0;
/// Flick speed (sheet-heights per second) that always completes the gesture.
const FLING: f64 = 1.2;

pub fn attach(w: &impl IsA<gtk::Widget>, spec: Spec) {
    let g = gtk::GestureDrag::new();
    // Capture phase: we see drags that start on buttons too. We only claim
    // the touch once it's clearly a vertical swipe, which cancels the button.
    g.set_propagation_phase(gtk::PropagationPhase::Capture);

    let st = Rc::new(State {
        spec,
        decided: Cell::new(false),
        target: RefCell::new(None),
        samples: RefCell::new(Vec::new()),
    });

    let s = st.clone();
    g.connect_drag_begin(move |_, _, _| {
        s.decided.set(false);
        s.target.borrow_mut().take();
        s.samples.borrow_mut().clear();
    });

    let s = st.clone();
    g.connect_drag_update(move |g, dx, dy| {
        if !s.decided.get() {
            if dx.abs() < SLOP && dy.abs() < SLOP {
                return;
            }
            s.decided.set(true);
            let picked = if dy.abs() > dx.abs() {
                (s.spec.pick)(dx, dy)
            } else {
                None
            };
            match picked {
                Some(t) => {
                    let _ = g.set_state(gtk::EventSequenceState::Claimed);
                    t.sheet.begin_drag();
                    *s.target.borrow_mut() = Some(t);
                }
                None => {
                    let _ = g.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
            }
        }
        let target = s.target.borrow();
        if let Some(t) = target.as_ref() {
            t.sheet.drag(t.from + t.sign * dy / t.sheet.travel());
            let mut v = s.samples.borrow_mut();
            v.push((glib::monotonic_time(), dy));
            if v.len() > 10 {
                v.remove(0);
            }
        }
    });

    let s = st.clone();
    g.connect_drag_end(move |_, dx, dy| {
        let target = s.target.borrow_mut().take();
        if let Some(t) = target {
            let vel = velocity(&s.samples.borrow()) * t.sign / t.sheet.travel();
            let p = t.sheet.progress();
            let open = if vel > FLING {
                true
            } else if vel < -FLING {
                false
            } else {
                p > 0.45
            };
            sheet::animate_to(&t.sheet, if open { 1.0 } else { 0.0 });
        } else if !s.decided.get() && dx.abs() < SLOP && dy.abs() < SLOP {
            if let Some(f) = &s.spec.tap {
                f();
            }
        }
    });

    let s = st.clone();
    g.connect_cancel(move |_, _| {
        // someone else (e.g. a scroller) took the touch: never treat it as a tap
        s.decided.set(true);
        let target = s.target.borrow_mut().take();
        if let Some(t) = target {
            let p = t.sheet.progress();
            sheet::animate_to(&t.sheet, if p > 0.5 { 1.0 } else { 0.0 });
        }
    });

    w.add_controller(g);

    // Mouse wheel / touchpad: scrolling down = pulling down, scrolling up =
    // pushing up, so the same sheets open and close without a touch screen.
    // Bubble phase: lists inside a sheet scroll first.
    let sc = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    let acc = Rc::new(Cell::new(0.0f64));
    let last = Rc::new(Cell::new(0i64));
    sc.connect_scroll(move |_, _, dy| {
        acc.set(acc.get() + dy);
        let now = glib::monotonic_time();
        if acc.get().abs() < 1.0 || now - last.get() < 350_000 {
            return glib::Propagation::Stop;
        }
        let d = acc.get().signum() * SLOP * 3.0;
        acc.set(0.0);
        last.set(now);
        if let Some(t) = (st.spec.pick)(0.0, d) {
            let opening = t.sign * d > 0.0;
            sheet::animate_to(&t.sheet, if opening { 1.0 } else { 0.0 });
        }
        glib::Propagation::Stop
    });
    w.add_controller(sc);
}

/// px/s over roughly the last 100ms of the drag.
pub fn velocity(samples: &[(i64, f64)]) -> f64 {
    let Some(&(t1, y1)) = samples.last() else {
        return 0.0;
    };
    let (t0, y0) = samples
        .iter()
        .find(|(t, _)| t1 - t < 100_000)
        .copied()
        .unwrap_or((t1, y1));
    let dt = (t1 - t0) as f64 / 1e6;
    if dt <= 0.0 {
        0.0
    } else {
        (y1 - y0) / dt
    }
}

/// Long-press-then-drag (moving icons around). After ~0.4s without moving,
/// `start(x, y)` is asked whether a drag begins; if so we claim the touch
/// (cancels buttons, scrolling and swipes) and report `motion` / `end`.
/// Coordinates are in `w`'s space - use a widget that doesn't move.
pub struct LongDrag {
    pub start: Box<dyn Fn(f64, f64) -> bool>,
    pub motion: Box<dyn Fn(f64, f64)>,
    pub end: Box<dyn Fn(f64, f64)>,
    pub cancel: Box<dyn Fn()>,
    /// With a mouse, a drag that starts upwards is left alone (recents: that
    /// flicks the card away to close it); other directions, or holding
    /// still first, pick the thing up.
    pub mouse_up_passes: bool,
}

struct LdState {
    spec: LongDrag,
    pressed: Cell<bool>,
    moved: Cell<bool>,
    active: Cell<bool>,
    start: Cell<(f64, f64)>,
    cur: Cell<(f64, f64)>,
    gen: Cell<u32>,
    /// pressed with a mouse: dragging starts right away, no hold needed
    mouse: Cell<bool>,
}

const HOLD_MS: u64 = 420;

pub fn attach_long_drag(w: &impl IsA<gtk::Widget>, spec: LongDrag) {
    let g = gtk::GestureDrag::new();
    g.set_propagation_phase(gtk::PropagationPhase::Capture);
    let st = Rc::new(LdState {
        spec,
        pressed: Cell::new(false),
        moved: Cell::new(false),
        active: Cell::new(false),
        start: Cell::new((0.0, 0.0)),
        cur: Cell::new((0.0, 0.0)),
        gen: Cell::new(0),
        mouse: Cell::new(false),
    });

    let s = st.clone();
    g.connect_drag_begin(move |g, x, y| {
        s.mouse.set(
            g.current_event_device()
                .map(|d| d.source() != gtk::gdk::InputSource::Touchscreen)
                .unwrap_or(false),
        );
        s.pressed.set(true);
        s.moved.set(false);
        s.active.set(false);
        s.start.set((x, y));
        s.cur.set((x, y));
        let gen = s.gen.get().wrapping_add(1);
        s.gen.set(gen);
        let (s2, g2) = (s.clone(), g.clone());
        let _ =
            glib::timeout_add_local_once(std::time::Duration::from_millis(HOLD_MS), move || {
                if s2.gen.get() != gen || !s2.pressed.get() || s2.moved.get() || s2.active.get() {
                    return;
                }
                let (x, y) = s2.start.get();
                if (s2.spec.start)(x, y) {
                    s2.active.set(true);
                    let _ = g2.set_state(gtk::EventSequenceState::Claimed);
                }
            });
    });

    let s = st.clone();
    g.connect_drag_update(move |g, dx, dy| {
        let (sx, sy) = s.start.get();
        s.cur.set((sx + dx, sy + dy));
        if s.active.get() {
            (s.spec.motion)(sx + dx, sy + dy);
        } else if dx.hypot(dy) > SLOP && !s.moved.get() {
            // A mouse can't flick-scroll by accident like a finger: click and
            // drag moves the thing straight away (a finger holds first).
            let upwards = dy < 0.0 && dy.abs() > dx.abs();
            if s.mouse.get() && !(s.spec.mouse_up_passes && upwards) && (s.spec.start)(sx, sy) {
                s.active.set(true);
                let _ = g.set_state(gtk::EventSequenceState::Claimed);
                (s.spec.motion)(sx + dx, sy + dy);
            } else {
                s.moved.set(true);
            }
        }
    });

    let s = st.clone();
    g.connect_drag_end(move |_, _, _| {
        s.pressed.set(false);
        if s.active.replace(false) {
            let (x, y) = s.cur.get();
            (s.spec.end)(x, y);
        }
    });

    let s = st.clone();
    g.connect_cancel(move |_, _| {
        s.pressed.set(false);
        if s.active.replace(false) {
            (s.spec.cancel)();
        }
    });

    w.add_controller(g);
}

/// With a mouse, drag a list to scroll it, like a finger does (GTK only
/// drag-scrolls with touch; a mouse otherwise needs the wheel). Touch is left
/// alone. A press held still for a moment is left alone too, so press-and-
/// hold dragging (moving icons, recents cards) still works; and at the end
/// of the list the drag is left to the menu (dragging the drawer closed).
pub fn mouse_drag_scroll(sc: &gtk::ScrolledWindow) {
    let g = gtk::GestureDrag::new();
    g.set_button(1);
    g.set_propagation_phase(gtk::PropagationPhase::Capture);
    // (horizontal, vertical) scroll at the start; when the button went down;
    // scrolling has started
    let start = Rc::new(Cell::new((0.0f64, 0.0f64)));
    let pressed_at = Rc::new(Cell::new(0i64));
    let scrolling = Rc::new(Cell::new(false));
    {
        let (sc, start, at, on) = (
            sc.clone(),
            start.clone(),
            pressed_at.clone(),
            scrolling.clone(),
        );
        g.connect_drag_begin(move |g, _, _| {
            let touch = g
                .current_event_device()
                .map(|d| d.source() == gtk::gdk::InputSource::Touchscreen)
                .unwrap_or(true);
            if touch {
                let _ = g.set_state(gtk::EventSequenceState::Denied);
                return;
            }
            start.set((sc.hadjustment().value(), sc.vadjustment().value()));
            at.set(glib::monotonic_time());
            on.set(false);
        });
    }
    {
        let (sc, start, at, on) = (
            sc.clone(),
            start.clone(),
            pressed_at.clone(),
            scrolling.clone(),
        );
        g.connect_drag_update(move |g, dx, dy| {
            if !on.get() {
                if dx.hypot(dy) < SLOP {
                    return;
                }
                // held still first: that's press-and-hold dragging, not scrolling
                if glib::monotonic_time() - at.get() > 350_000 {
                    let _ = g.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                let (h, v) = (sc.hadjustment(), sc.vadjustment());
                let can_h = h.upper() - h.page_size() > 1.0;
                let can_v = v.upper() - v.page_size() > 1.0;
                let horizontal = dx.abs() > dy.abs();
                // nothing to scroll that way, or already at that end: the
                // drag belongs to whatever is around the list
                let at_end = !horizontal && ((dy > 0.0 && v.value() <= v.lower()) || !can_v);
                if (horizontal && !can_h) || at_end {
                    let _ = g.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                on.set(true);
                let _ = g.set_state(gtk::EventSequenceState::Claimed);
            }
            let (h0, v0) = start.get();
            let (h, v) = (sc.hadjustment(), sc.vadjustment());
            if h.upper() - h.page_size() > 1.0 {
                h.set_value((h0 - dx).clamp(h.lower(), h.upper() - h.page_size()));
            }
            if v.upper() - v.page_size() > 1.0 {
                v.set_value((v0 - dy).clamp(v.lower(), v.upper() - v.page_size()));
            }
        });
    }
    {
        let on = scrolling.clone();
        g.connect_drag_end(move |_, _, _| on.set(false));
    }
    sc.add_controller(g);
}

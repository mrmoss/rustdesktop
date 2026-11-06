//! Swipe-up app drawer: search + grid of every visible .desktop app.
//! Built once and only refreshed when apps are (un)installed, so opening it
//! is instant. Long-press an app to drag it onto the home screen.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use crate::apps;
use crate::gestures::{self, LongDrag, Spec};
use crate::home;
use crate::shell::{Overlay, Shell};

/// Tiles carry "desktop-id \u{1} lowercase name" in their widget name:
/// the id for dragging, the name for search.
const SEP: char = '\u{1}';

/// The app id of the tile under (x, y) on the drawer's stage, if any.
fn app_at(stage: &gtk::Widget, x: f64, y: f64) -> Option<String> {
    let mut w = stage.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(cur) = w {
        let name = cur.widget_name();
        if let Some((id, _)) = name.split_once(SEP) {
            return Some(id.to_string());
        }
        w = cur.parent();
    }
    None
}

pub fn build(shell: &Shell) -> (gtk::Box, gtk::Box) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("overlay-bg");

    let header = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let handle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    handle.add_css_class("handle");
    handle.set_halign(gtk::Align::Center);
    let entry = gtk::SearchEntry::new();
    entry.set_placeholder_text(Some("Search apps"));
    header.append(&handle);
    header.append(&entry);
    root.append(&header);

    let flow = gtk::FlowBox::new();
    flow.set_valign(gtk::Align::Start);
    flow.set_min_children_per_line(4);
    flow.set_max_children_per_line(6);
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_row_spacing(8);
    flow.set_column_spacing(4);
    flow.set_margin_start(8);
    flow.set_margin_end(8);

    let scroll = gtk::ScrolledWindow::builder()
        .child(&flow)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    root.append(&scroll);

    let tip = gtk::Label::new(Some("Long-press an app to put it on the home screen"));
    tip.add_css_class("hint");
    tip.set_margin_top(6);
    root.append(&tip);

    let stage = shell.drawer.stage();

    // drag down (when scrolled to the top) closes the drawer with the finger
    let (s, sc) = (shell.clone(), scroll.clone());
    gestures::attach(
        &stage,
        Spec {
            pick: Box::new(move |_dx: f64, dy: f64| {
                if dy > 0.0 && sc.vadjustment().value() <= 1.0 {
                    s.target(Overlay::Drawer, -1.0, 1.0)
                } else {
                    None
                }
            }),
            tap: None,
        },
    );

    // long-press an app: the drawer fades out and you drop it on the home screen
    let dragging: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let (s1, d1, st1) = (shell.clone(), dragging.clone(), stage.clone());
    let (s2, d2) = (shell.clone(), dragging.clone());
    let (s3, d3) = (shell.clone(), dragging.clone());
    let (s4, d4) = (shell.clone(), dragging.clone());
    gestures::attach_long_drag(
        &stage,
        LongDrag {
            start: Box::new(move |x: f64, y: f64| {
                let (Some(id), Some(h)) = (app_at(&st1, x, y), s1.home_ui()) else {
                    return false;
                };
                s1.show_home();
                s1.drawer.set_ghost(true);
                home::external_start(&h, &id, x, y);
                *d1.borrow_mut() = Some(id);
                true
            }),
            motion: Box::new(move |x: f64, y: f64| {
                if let (Some(_), Some(h)) = (d2.borrow().as_ref(), s2.home_ui()) {
                    home::external_move(&h, x, y);
                }
            }),
            end: Box::new(move |x: f64, y: f64| {
                if d3.borrow_mut().take().is_some() {
                    if let Some(h) = s3.home_ui() {
                        home::external_drop(&h, x, y);
                    }
                    s3.hide_all();
                }
            }),
            cancel: Box::new(move || {
                if d4.borrow_mut().take().is_some() {
                    if let Some(h) = s4.home_ui() {
                        home::cancel(&h);
                    }
                    s4.drawer.set_ghost(false);
                }
            }),
        },
    );

    // search filtering on the name part of the widget name
    // tapping the search box brings up the keyboard (only then - not when the
    // drawer opens and GTK puts the cursor there by itself)
    // On *press*: the text field claims the touch for itself right after,
    // which cancels this gesture, so a "released" would never arrive.
    let focus = gtk::GestureClick::new();
    focus.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (s, e) = (shell.clone(), entry.clone());
    focus.connect_pressed(move |_, _, _, _| {
        e.grab_focus();
        s.sheet_keyboard(Overlay::Drawer, true);
    });
    entry.add_controller(focus);

    let e = entry.clone();
    flow.set_filter_func(move |child| {
        let q = e.text().to_lowercase();
        q.is_empty()
            || child
                .child()
                .map(|c| {
                    let n = c.widget_name();
                    n.split_once(SEP)
                        .map(|(_, name)| name.contains(q.as_str()))
                        .unwrap_or(true)
                })
                .unwrap_or(true)
    });
    let f = flow.clone();
    entry.connect_search_changed(move |_| f.invalidate_filter());

    // Enter launches the first match
    let f = flow.clone();
    entry.connect_activate(move |_| {
        let mut cur = f.first_child();
        while let Some(w) = cur {
            if w.is_child_visible() {
                let button = w
                    .downcast_ref::<gtk::FlowBoxChild>()
                    .and_then(|fc| fc.child())
                    .and_then(|c| c.downcast::<gtk::Button>().ok());
                if let Some(b) = button {
                    b.emit_clicked();
                    break;
                }
            }
            cur = w.next_sibling();
        }
    });

    let populate: Rc<dyn Fn()> = {
        let flow = flow.clone();
        let s = shell.clone();
        Rc::new(move || {
            while let Some(c) = flow.first_child() {
                flow.remove(&c);
            }
            for info in apps::all_apps() {
                let b = apps::tile(&info, 52);
                let id = info.id().map(|s| s.to_string()).unwrap_or_default();
                b.set_widget_name(&format!("{id}{SEP}{}", info.display_name().to_lowercase()));
                let s2 = s.clone();
                b.connect_clicked(move |_| apps::launch(&s2, &info));
                flow.insert(&b, -1);
            }
        })
    };
    populate();

    // refresh only when the installed apps change
    let monitor = gio::AppInfoMonitor::get();
    let p = populate.clone();
    monitor.connect_changed(move |_| p());
    std::mem::forget(monitor);

    let sc = scroll.clone();
    shell.drawer.on_open(move || {
        entry.set_text("");
        sc.vadjustment().set_value(0.0);
    });

    (root.clone(), root)
}

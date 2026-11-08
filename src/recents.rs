//! Android-style recents: a horizontal row of cards with screenshots.
//!
//! On sway a card is a *window group* (a workspace): its windows are drawn in
//! their real arrangement, each with an ✕ to close just that window.
//! * tap a card = open the group, flick it up = close every window in it
//! * hold a window and drag it: onto an edge of another window (same group or
//!   another) to dock it on that side, or onto empty space to give it a group
//!   of its own
//!
//! Elsewhere (phoc/labwc) each window is its own card.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

use crate::shell::{self, Shell};
use crate::slide::Slide;
use crate::sway::{self, Rect};
use crate::tiling::{self, Side};
use crate::wl::Toplevel;
use crate::{anim, apps, gestures};

/// A window drawn inside a group card.
struct Sub {
    id: i64,
    app_id: String,
    title: String,
    widget: gtk::Widget,
}

/// A group card's window area, for drops on the group's own edges.
struct GroupHit {
    num: i64,
    frame: gtk::Widget,
    ids: Vec<i64>,
}

#[derive(Clone, Copy, PartialEq)]
enum DropAt {
    /// dock on a side of another window
    Beside(i64, Side),
    /// dock on a side of a whole group (workspace number)
    Edge(i64, Side),
    /// a group of its own
    Alone,
}

#[derive(Default)]
struct Drag {
    subs: Vec<Sub>,
    groups: Vec<GroupHit>,
    dragged: Option<i64>,
    target: Option<DropAt>,
}

pub fn build(shell: &Shell) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("recents");

    // Android-style: older apps to the left, the one you just left at the
    // right end and centered on screen; scrolling settles on a card
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 28);
    row.set_valign(gtk::Align::Center);

    let scroll = gtk::ScrolledWindow::builder()
        .child(&row)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let empty = gtk::Label::new(None);
    empty.add_css_class("hint");
    empty.set_justify(gtk::Justification::Center);
    empty.set_vexpand(true);

    let clear = gtk::Button::with_label("Clear all");
    clear.add_css_class("pill");
    clear.set_halign(gtk::Align::Center);
    clear.set_margin_bottom(20);

    // drag layer above the cards
    let stage = gtk::Overlay::new();
    stage.set_child(Some(&scroll));
    stage.set_vexpand(true);
    let fixed = gtk::Fixed::new();
    fixed.set_can_target(false);
    stage.add_overlay(&fixed);
    let hint = gtk::Box::new(gtk::Orientation::Vertical, 0);
    hint.add_css_class("drop-hint");
    hint.set_can_target(false);
    hint.set_visible(false);
    fixed.put(&hint, 0.0, 0.0);
    let ghost = gtk::Box::new(gtk::Orientation::Vertical, 6);
    ghost.add_css_class("rdrag-ghost");
    ghost.set_can_target(false);
    ghost.set_visible(false);
    fixed.put(&ghost, 0.0, 0.0);

    root.append(&stage);
    root.append(&empty);
    root.append(&clear);

    let drag: Rc<RefCell<Drag>> = Rc::new(RefCell::new(Drag::default()));
    let rebuild_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let rebuild: Rc<dyn Fn()> = {
        let (s, row, scroll, empty, clear, drag, slot) = (
            shell.clone(),
            row.clone(),
            scroll.clone(),
            empty.clone(),
            clear.clone(),
            drag.clone(),
            rebuild_slot.clone(),
        );
        Rc::new(move || {
            while let Some(c) = row.first_child() {
                row.remove(&c);
            }
            drag.borrow_mut().subs.clear();
            drag.borrow_mut().groups.clear();
            let (cw, ch) = card_size(&s);
            // room on both sides so the first and last cards can be centered
            let (_, _, sw, _) = shell::screen_size();
            let side = ((sw - cw) / 2).max(28);
            row.set_margin_start(side);
            row.set_margin_end(side);
            let mut count = 0;
            if tiling::enabled(&s) {
                let refresh = slot.borrow().clone();
                for g in ordered_groups(&s) {
                    let (card, subs, frame) = group_card(&s, &g, cw, ch, refresh.clone());
                    row.prepend(&card); // most recent ends up on the right
                    let mut d = drag.borrow_mut();
                    d.groups.push(GroupHit {
                        num: g.num,
                        frame,
                        ids: g.root.windows().iter().map(|w| w.id).collect(),
                    });
                    d.subs.extend(subs);
                    count += 1;
                }
            } else {
                for t in s.toplevels_mru() {
                    row.prepend(&card(&s, t, cw, ch));
                    count += 1;
                }
            }
            let none = count == 0;
            scroll.set_visible(!none);
            clear.set_visible(!none);
            empty.set_visible(none);
            empty.set_text(if s.has_window_list() || tiling::enabled(&s) {
                "No recent apps"
            } else {
                "This compositor doesn't share its window list.\nRun rusttouch on sway, phoc or labwc."
            });
        })
    };
    *rebuild_slot.borrow_mut() = Some(rebuild.clone());
    rebuild();
    attach_group_drag(shell, &stage, &fixed, &hint, &ghost, &drag, rebuild.clone());

    // screen rotated (or resized) while recents shows: redraw the cards in
    // the new shape once the screen has settled
    if let Some(m) = shell::shell_monitor() {
        let (r, s) = (rebuild.clone(), shell.clone());
        m.connect_geometry_notify(move |_| {
            if s.recents.is_open() {
                let r = r.clone();
                let _ = glib::timeout_add_local_once(Duration::from_millis(400), move || r());
            }
        });
    }
    let (r, s) = (rebuild.clone(), shell.clone());
    shell.on_toplevels(move || {
        if s.recents.is_open() {
            r();
        }
    });
    let r = rebuild.clone();
    let sc = scroll.clone();
    shell.recents.on_open(move || {
        r();
        // start at the most recent card (right end), once laid out
        let sc2 = sc.clone();
        let _ = glib::idle_add_local_once(move || {
            let a = sc2.hadjustment();
            a.set_value(a.upper() - a.page_size());
        });
    });
    {
        // no gliding while a window is being dragged (the cards would slide
        // out from under the finger and the drop would land somewhere else)
        let d = drag.clone();
        attach_snap(&scroll, &row, Rc::new(move || d.borrow().dragged.is_some()));
    }

    let s = shell.clone();
    clear.connect_clicked(move |_| {
        for t in s.toplevels_mru() {
            s.close_toplevel(&t);
        }
        s.hide_all();
        s.show_home();
    });

    root
}

/// When scrolling stops, glide so the nearest card is centered.
fn attach_snap(scroll: &gtk::ScrolledWindow, row: &gtk::Box, busy: Rc<dyn Fn() -> bool>) {
    let adj = scroll.hadjustment();
    let gen = Rc::new(Cell::new(0u32));
    let snapping = Rc::new(Cell::new(false));
    let (r, g, sn, sc) = (row.clone(), gen.clone(), snapping.clone(), scroll.clone());
    adj.connect_value_changed(move |a| {
        if sn.get() {
            return;
        }
        let my = g.get().wrapping_add(1);
        g.set(my);
        let (a, r, g, sn, sc, busy) = (
            a.clone(),
            r.clone(),
            g.clone(),
            sn.clone(),
            sc.clone(),
            busy.clone(),
        );
        let _ = glib::timeout_add_local_once(Duration::from_millis(160), move || {
            if g.get() != my || busy() {
                return; // still moving, or a window is being dragged
            }
            let centre = a.value() + a.page_size() / 2.0;
            let mut best: Option<f64> = None;
            let mut child = r.first_child();
            while let Some(c) = child {
                if let Some(b) = c.compute_bounds(&r) {
                    // row coordinates + its left margin = scroll position
                    let cx = f64::from(b.x() + b.width() / 2.0) + f64::from(r.margin_start());
                    if best
                        .map(|v| (cx - centre).abs() < (v - centre).abs())
                        .unwrap_or(true)
                    {
                        best = Some(cx);
                    }
                }
                child = c.next_sibling();
            }
            let Some(cx) = best else { return };
            let to = (cx - a.page_size() / 2.0).clamp(a.lower(), a.upper() - a.page_size());
            let from = a.value();
            if (to - from).abs() < 1.0 {
                return;
            }
            sn.set(true);
            let (a2, sn2) = (a.clone(), sn.clone());
            let _ = anim::run(
                &sc,
                160,
                move |e| a2.set_value(from + (to - from) * e),
                move || sn2.set(false),
            );
        });
    });
}

/// Cards keep the shape of the app area (screen minus bars).
/// Cards have the shape of the whole screen.
fn card_size(shell: &Shell) -> (i32, i32) {
    let _ = shell;
    let (_, _, sw, sh) = shell::screen_size();
    let (sw, sh) = (f64::from(sw.max(100)), f64::from(sh.max(100)));
    let mut cw = sw * 0.46;
    let mut ch = cw * sh / sw;
    let max_h = sh * 0.55;
    if ch > max_h {
        ch = max_h;
        cw = ch * sw / sh;
    }
    (cw as i32, ch as i32)
}

/// A screenshot shrunk to w x h (device pixels for sharp text), or None.
pub fn preview(path: Option<std::path::PathBuf>, w: i32, h: i32) -> Option<gtk::Widget> {
    crate::pics::widget(path, w, h)
}

pub fn placeholder(app_id: &str, icon_px: i32, explain: bool) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.add_css_class("rcard-empty");
    let img = apps::icon_image(apps::icon_for_app_id(app_id).as_ref(), icon_px);
    img.set_vexpand(true);
    img.set_valign(if explain {
        gtk::Align::End
    } else {
        gtk::Align::Center
    });
    b.append(&img);
    if explain {
        let why =
            gtk::Label::new(Some(&shell::capture_error().unwrap_or_else(|| {
                "Preview appears once the app has been on screen".to_string()
            })));
        why.add_css_class("hint");
        why.set_wrap(true);
        why.set_justify(gtk::Justification::Center);
        why.set_vexpand(true);
        why.set_valign(gtk::Align::Start);
        why.set_margin_top(12);
        b.append(&why);
    }
    b.upcast()
}

fn title_of<'a>(app_id: &'a str, title: &'a str) -> &'a str {
    if title.is_empty() {
        app_id
    } else {
        title
    }
}

// ---------------------------------------------------------------- groups (sway)

/// Groups, most recently used first.
fn ordered_groups(shell: &Shell) -> Vec<sway::Group> {
    let mut groups = sway::groups(shell::shell_output_name().as_deref());
    let mru = shell.toplevels_mru();
    let rank = |g: &sway::Group| {
        g.root
            .windows()
            .iter()
            .filter_map(|w| {
                mru.iter()
                    .position(|t| t.app_id == w.app_id && t.title == w.title)
            })
            .min()
            .unwrap_or(usize::MAX)
    };
    groups.sort_by_key(rank);
    groups
}

fn group_card(
    shell: &Shell,
    g: &sway::Group,
    w: i32,
    h: i32,
    refresh: Option<Rc<dyn Fn()>>,
) -> (Slide, Vec<Sub>, gtk::Widget) {
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 8);
    vb.add_css_class("rcard");
    let wins = g.root.windows();

    // header: the apps' icons, then the most recent window's title
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    for win in wins.iter().take(4) {
        header.append(&apps::icon_image(
            apps::icon_for_app_id(&win.app_id).as_ref(),
            22,
        ));
    }
    let first = wins
        .iter()
        .min_by_key(|win| {
            shell
                .toplevels_mru()
                .iter()
                .position(|t| t.app_id == win.app_id && t.title == win.title)
                .unwrap_or(usize::MAX)
        })
        .cloned();
    let name = match (&first, wins.len()) {
        (Some(f), 1) => title_of(&f.app_id, &f.title).to_string(),
        (Some(f), n) => format!("{} +{}", title_of(&f.app_id, &f.title), n - 1),
        _ => String::new(),
    };
    let title = gtk::Label::new(Some(&name));
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(20);
    title.set_halign(gtk::Align::Start);
    title.set_hexpand(true);
    header.append(&title);

    // the windows in their real arrangement, scaled down
    let fixed = gtk::Fixed::new();
    fixed.set_size_request(w, h);
    fixed.add_css_class("rcard-frame");
    fixed.add_css_class("group-frame");
    fixed.set_overflow(gtk::Overflow::Hidden);
    // the card is a small copy of the whole screen: windows sit where they
    // are on it (the bars' strips stay dark)
    let (ox, oy, ow, oh) = shell::screen_size();
    let sx = f64::from(w) / f64::from(ow.max(1));
    let sy = f64::from(h) / f64::from(oh.max(1));
    let gap = if wins.len() > 1 { 1 } else { 0 };
    let mut subs = Vec::new();
    for win in &wins {
        let r = Rect {
            x: ((f64::from(win.rect.x - ox)) * sx) as i32 + gap,
            y: ((f64::from(win.rect.y - oy)) * sy) as i32 + gap,
            w: ((f64::from(win.rect.w)) * sx) as i32 - 2 * gap,
            h: ((f64::from(win.rect.h)) * sy) as i32 - 2 * gap,
        };
        let (rw, rh) = (r.w.max(24), r.h.max(24));
        let content: gtk::Widget = match preview(shell.con_preview(win.id), rw, rh) {
            Some(p) => p.upcast(),
            None => placeholder(
                &win.app_id,
                if wins.len() > 1 { 48 } else { 96 },
                wins.len() == 1,
            ),
        };
        content.set_size_request(rw, rh);

        let ov = gtk::Overlay::new();
        ov.add_css_class("group-sub");
        ov.set_overflow(gtk::Overflow::Hidden);
        ov.set_child(Some(&content));
        ov.set_size_request(rw, rh);

        // ✕: close just this window. Claims the touch on press, so the card
        // underneath doesn't also open.
        let x = gtk::Label::new(Some("✕"));
        x.add_css_class("sub-close");
        x.set_halign(gtk::Align::End);
        x.set_valign(gtk::Align::Start);
        x.set_margin_top(6);
        x.set_margin_end(6);
        let click = gtk::GestureClick::new();
        click.connect_pressed(|g, _, _, _| {
            let _ = g.set_state(gtk::EventSequenceState::Claimed);
        });
        let (id, refresh2) = (win.id, refresh.clone());
        click.connect_released(move |_, _, _, _| {
            sway::run(&format!("[con_id={id}] kill"));
            if let Some(f) = refresh2.clone() {
                let _ = glib::timeout_add_local_once(Duration::from_millis(250), move || f());
            }
        });
        x.add_controller(click);
        ov.add_overlay(&x);

        fixed.put(&ov, f64::from(r.x), f64::from(r.y));
        subs.push(Sub {
            id: win.id,
            app_id: win.app_id.clone(),
            title: win.title.clone(),
            widget: ov.upcast(),
        });
    }

    vb.append(&header);
    vb.append(&fixed);
    let slide = Slide::new(&vb);

    let ids: Vec<i64> = wins.iter().map(|w| w.id).collect();
    let focus = first.map(|f| f.id).or(ids.first().copied());
    let s = shell.clone();
    let on_tap: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(id) = focus {
            sway::run(&format!("[con_id={id}] focus"));
        }
        s.go_app();
    });
    let on_close: Rc<dyn Fn()> = Rc::new(move || {
        let cmd: Vec<String> = ids.iter().map(|id| format!("[con_id={id}] kill")).collect();
        if !cmd.is_empty() {
            sway::run(&cmd.join("; "));
        }
    });
    attach_swipe(&slide, f64::from(h), on_tap, on_close);
    (slide, subs, fixed.upcast())
}

/// Hold a window in a group card and drag it somewhere else.
fn attach_group_drag(
    shell: &Shell,
    stage: &gtk::Overlay,
    fixed: &gtk::Fixed,
    hint: &gtk::Box,
    ghost: &gtk::Box,
    drag: &Rc<RefCell<Drag>>,
    rebuild: Rc<dyn Fn()>,
) {
    let bounds = {
        let stage = stage.clone();
        move |w: &gtk::Widget| {
            w.compute_bounds(&stage).map(|b| {
                (
                    f64::from(b.x()),
                    f64::from(b.y()),
                    f64::from(b.width()),
                    f64::from(b.height()),
                )
            })
        }
    };
    let inside = |b: (f64, f64, f64, f64), x: f64, y: f64| {
        x >= b.0 && x < b.0 + b.2 && y >= b.1 && y < b.1 + b.3
    };

    let reset = {
        let (hint, ghost, drag) = (hint.clone(), ghost.clone(), drag.clone());
        move || {
            hint.set_visible(false);
            ghost.set_visible(false);
            let mut d = drag.borrow_mut();
            if let Some(id) = d.dragged.take() {
                if let Some(s) = d.subs.iter().find(|s| s.id == id) {
                    s.widget.set_opacity(1.0);
                }
            }
            d.target = None;
        }
    };

    let (s1, d1, ghost1, fixed1, b1, stage1) = (
        shell.clone(),
        drag.clone(),
        ghost.clone(),
        fixed.clone(),
        bounds.clone(),
        stage.clone(),
    );
    let (d2, ghost2, hint2, fixed2, b2) = (
        drag.clone(),
        ghost.clone(),
        hint.clone(),
        fixed.clone(),
        bounds,
    );
    let (d3, reset3) = (drag.clone(), reset.clone());
    gestures::attach_long_drag(
        stage,
        gestures::LongDrag {
            start: Box::new(move |x: f64, y: f64| {
                if !tiling::enabled(&s1) {
                    return false;
                }
                let mut d = d1.borrow_mut();
                let Some(sub) = d
                    .subs
                    .iter()
                    .find(|s| b1(&s.widget).map(|b| inside(b, x, y)).unwrap_or(false))
                else {
                    return false;
                };
                sub.widget.set_opacity(0.35);
                // stop the row where it is (the scroll that began before the
                // hold kicked in mustn't keep going while you drag)
                if let Some(sc) = stage1.child().and_downcast::<gtk::ScrolledWindow>() {
                    let a = sc.hadjustment();
                    let v = a.value();
                    sc.set_kinetic_scrolling(false);
                    sc.set_kinetic_scrolling(true);
                    a.set_value(v);
                }
                while let Some(ch) = ghost1.first_child() {
                    ghost1.remove(&ch);
                }
                ghost1.append(&apps::icon_image(
                    apps::icon_for_app_id(&sub.app_id).as_ref(),
                    48,
                ));
                let l = gtk::Label::new(Some(title_of(&sub.app_id, &sub.title)));
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                l.set_max_width_chars(14);
                ghost1.append(&l);
                fixed1.move_(&ghost1, x - 60.0, y - 50.0);
                ghost1.set_visible(true);
                let id = sub.id;
                d.dragged = Some(id);
                true
            }),
            motion: Box::new(move |x: f64, y: f64| {
                fixed2.move_(&ghost2, x - 60.0, y - 50.0);
                let mut d = d2.borrow_mut();
                let me = d.dragged;

                // near the outer edge of a group card (or just outside it):
                // the window goes beside the whole group
                let edge = d.groups.iter().find_map(|g| {
                    if g.ids.len() == 1 && Some(g.ids[0]) == me {
                        return None; // its own single-window card
                    }
                    let (bx, by, bw, bh) = b2(&g.frame)?;
                    let out = 18.0;
                    if !inside((bx - out, by - out, bw + 2.0 * out, bh + 2.0 * out), x, y) {
                        return None;
                    }
                    let band_x = (bw * 0.14).max(18.0);
                    let band_y = (bh * 0.14).max(18.0);
                    let side = if x < bx + band_x {
                        Side::Left
                    } else if x > bx + bw - band_x {
                        Side::Right
                    } else if y < by + band_y {
                        Side::Top
                    } else if y > by + bh - band_y {
                        Side::Bottom
                    } else {
                        return None;
                    };
                    // a line along that edge of this one group, inside its card
                    let t = 8.0;
                    let r = match side {
                        Side::Left => (bx, by, t, bh),
                        Side::Right => (bx + bw - t, by, t, bh),
                        Side::Top => (bx, by, bw, t),
                        Side::Bottom => (bx, by + bh - t, bw, t),
                    };
                    Some((DropAt::Edge(g.num, side), r))
                });
                if let Some((t, (hx, hy, hw, hh))) = edge {
                    hint2.add_css_class("group-edge");
                    hint2.set_size_request(hw.max(8.0) as i32, hh.max(8.0) as i32);
                    fixed2.move_(&hint2, hx, hy);
                    hint2.set_visible(true);
                    ghost2.remove_css_class("alone");
                    d.target = Some(t);
                    return;
                }
                hint2.remove_css_class("group-edge");

                let over = d.subs.iter().find(|s| {
                    Some(s.id) != me && b2(&s.widget).map(|b| inside(b, x, y)).unwrap_or(false)
                });
                let found = match over {
                    Some(s) => b2(&s.widget).map(|(bx, by, bw, bh)| {
                        let side = tiling::nearest_side(x - bx, y - by, bw, bh);
                        let (hx, hy, hw, hh) = match side {
                            Side::Left => (bx, by, bw / 2.0, bh),
                            Side::Right => (bx + bw / 2.0, by, bw / 2.0, bh),
                            Side::Top => (bx, by, bw, bh / 2.0),
                            Side::Bottom => (bx, by + bh / 2.0, bw, bh / 2.0),
                        };
                        hint2.set_size_request(hw.max(8.0) as i32, hh.max(8.0) as i32);
                        fixed2.move_(&hint2, hx, hy);
                        DropAt::Beside(s.id, side)
                    }),
                    // over its own spot or a gap inside a card: nothing
                    None if d
                        .subs
                        .iter()
                        .any(|s| b2(&s.widget).map(|b| inside(b, x, y)).unwrap_or(false)) =>
                    {
                        None
                    }
                    // empty space: its own group
                    None => Some(DropAt::Alone),
                };
                hint2.set_visible(matches!(found, Some(DropAt::Beside(..))));
                if found == Some(DropAt::Alone) {
                    ghost2.add_css_class("alone");
                } else {
                    ghost2.remove_css_class("alone");
                }
                d.target = found;
            }),
            end: Box::new(move |_x: f64, _y: f64| {
                let (dragged, target) = {
                    let d = d3.borrow();
                    (d.dragged, d.target)
                };
                reset3();
                let Some(id) = dragged else { return };
                match target {
                    Some(DropAt::Beside(t, side)) => tiling::dock(id, t, side),
                    Some(DropAt::Edge(num, side)) => tiling::dock_group(id, num, side),
                    Some(DropAt::Alone) => {
                        let k = sway::free_workspace();
                        sway::run(&format!(
                            "[con_id={id}] move container to workspace number {k}"
                        ));
                    }
                    None => return,
                }
                // stay in recents to keep arranging; redraw once sway has moved things
                let r = rebuild.clone();
                let _ = glib::timeout_add_local_once(Duration::from_millis(200), move || r());
            }),
            cancel: Box::new(move || reset()),
            mouse_up_passes: true,
        },
    );
}

// ---------------------------------------------------------------- single windows

fn card(shell: &Shell, t: Toplevel, w: i32, h: i32) -> Slide {
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 8);
    vb.add_css_class("rcard");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.append(&apps::icon_image(
        apps::icon_for_app_id(&t.app_id).as_ref(),
        22,
    ));
    let title = gtk::Label::new(Some(title_of(&t.app_id, &t.title)));
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(20);
    title.set_halign(gtk::Align::Start);
    title.set_hexpand(true);
    header.append(&title);

    let frame: gtk::Widget = match preview(shell.preview(t.key), w, h) {
        Some(p) => p.upcast(),
        None => placeholder(&t.app_id, 96, true),
    };
    frame.set_size_request(w, h);
    frame.add_css_class("rcard-frame");
    frame.set_overflow(gtk::Overflow::Hidden);

    vb.append(&header);
    vb.append(&frame);

    let slide = Slide::new(&vb);
    let (s1, t1) = (shell.clone(), t.clone());
    let (s2, t2) = (shell.clone(), t);
    attach_swipe(
        &slide,
        f64::from(h),
        Rc::new(move || s1.activate(&t1)),
        Rc::new(move || s2.close_toplevel(&t2)),
    );
    slide
}

/// Vertical drag up = close, tap = open. Horizontal drags scroll the row.
fn attach_swipe(slide: &Slide, h: f64, on_tap: Rc<dyn Fn()>, on_close: Rc<dyn Fn()>) {
    let g = gtk::GestureDrag::new();
    // 0 = undecided, 1 = swiping this card, 2 = not ours (scrolling)
    let mode = Rc::new(Cell::new(0u8));
    let samples = Rc::new(RefCell::new(Vec::<(i64, f64)>::new()));

    {
        let (mode, samples) = (mode.clone(), samples.clone());
        g.connect_drag_begin(move |_, _, _| {
            mode.set(0);
            samples.borrow_mut().clear();
        });
    }

    {
        let (mode, samples, sl) = (mode.clone(), samples.clone(), slide.clone());
        g.connect_drag_update(move |g, dx, dy| {
            if mode.get() == 0 {
                if dx.abs().max(dy.abs()) < 10.0 {
                    return;
                }
                if dy < 0.0 && dy.abs() > dx.abs() {
                    let _ = g.set_state(gtk::EventSequenceState::Claimed);
                    mode.set(1);
                } else {
                    let _ = g.set_state(gtk::EventSequenceState::Denied);
                    mode.set(2);
                    return;
                }
            }
            if mode.get() == 1 {
                let y = dy.min(0.0);
                sl.set_offset(y);
                sl.set_opacity((1.0 + y / (h * 1.3)).clamp(0.0, 1.0));
                let mut v = samples.borrow_mut();
                v.push((glib::monotonic_time(), y));
                if v.len() > 10 {
                    v.remove(0);
                }
            }
        });
    }

    let sl = slide.clone();
    g.connect_drag_end(move |_, _dx, dy| {
        if mode.get() != 1 {
            return;
        }
        let from = sl.offset();
        let vel = gestures::velocity(&samples.borrow());
        if dy < -h * 0.3 || vel < -900.0 {
            // fly away, then close
            let target = -h * 1.5;
            let (sl2, sl3, close) = (sl.clone(), sl.clone(), on_close.clone());
            let _ = anim::run(
                &sl,
                170,
                move |e| {
                    let y = from + (target - from) * e;
                    sl2.set_offset(y);
                    sl2.set_opacity((1.0 + y / (h * 1.3)).clamp(0.0, 1.0));
                },
                move || {
                    close();
                    // if an app refuses to close (unsaved work), bring the card back
                    let _ = glib::timeout_add_local_once(Duration::from_millis(1500), move || {
                        sl3.set_offset(0.0);
                        sl3.set_opacity(1.0);
                    });
                },
            );
        } else {
            let sl2 = sl.clone();
            let op = sl.opacity();
            let _ = anim::run(
                &sl,
                150,
                move |e| {
                    sl2.set_offset(from * (1.0 - e));
                    sl2.set_opacity(op + (1.0 - op) * e);
                },
                || {},
            );
        }
    });
    slide.add_controller(g);

    // Taps use a click gesture: GTK cancels it as soon as the row starts
    // scrolling sideways, so dragging through a card never opens it.
    let click = gtk::GestureClick::new();
    click.connect_released(move |_, _, _, _| on_tap());
    slide.add_controller(click);
}

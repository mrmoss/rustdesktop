//! Split screen (needs sway - phoc and labwc don't let a shell arrange windows).
//!
//! * Every app opens on its own workspace, full screen, like a tablet.
//! * Recents shows window groups (workspaces): hold a window and drag it onto
//!   another window's edge, or onto empty space for a group of its own.
//! * A handle sits on the line between split windows: drag it to resize,
//!   tap it to arrange - drag windows onto each other to swap them, or onto
//!   an edge of another window to dock there, so you can build any grid.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};

use crate::apps;
use crate::layer::{self, Anchors, KeyboardMode, Layer, Zone};
use crate::shell::{self, Event, Shell};
use crate::sway::{self, Node, Rect, Screen, Win};
use crate::wl::Toplevel;

/// Thickness of the grab area on the line between windows (logical px).
const GRAB: i32 = 28;
/// Matches `gaps inner` in session/sway/config.
const GAP: i32 = 4;
/// Smallest a window can be made by dragging.
const MIN: i32 = 140;
pub use crate::screens::MIRROR_APP;

#[derive(Clone, Debug, PartialEq)]
struct Divider {
    /// the window/split left of (or above) the line
    a: i64,
    /// true = line between side-by-side windows (vertical line)
    side_by_side: bool,
    /// centre of the line, along the moving axis (global coords)
    pos: i32,
    /// where the line starts and how long it is, along the other axis
    start: i32,
    len: i32,
    /// start of `a` and end of the next one, along the moving axis
    lo: i32,
    hi: i32,
}

#[derive(Default)]
struct State {
    dividers: Vec<gtk::ApplicationWindow>,
    layout: Vec<Divider>,
    output: Rect,
    dragging: bool,
    guide: Option<(
        gtk::ApplicationWindow,
        gtk::DrawingArea,
        Rc<Cell<(bool, f64)>>,
    )>,
    arrange: Option<gtk::ApplicationWindow>,
    last_poll: i64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn enabled(shell: &shell::ShellInner) -> bool {
    shell.layered && sway::available()
}

/// Watch sway for new windows / layout changes.
pub fn start(shell: &Shell) {
    if !enabled(shell) {
        return;
    }
    eprintln!("rusttouch: sway found - split screen available");
    let tx = shell.tx();
    sway::watch_windows(move |change, c| {
        if change == "new" && c["type"] == "con" && c["app_id"] != MIRROR_APP {
            if let Some(id) = c["id"].as_i64() {
                let _ = tx.send(Event::NewWindow(id));
            }
        }
        // titles don't move anything
        if change != "title" && change != "mark" && change != "urgent" {
            let _ = tx.send(Event::Layout);
        }
    });
}

/// A window opened next to others on a workspace (the app opened a second
/// window by itself): give it its own workspace, like everything else.
fn tile_mode() -> bool {
    crate::config::Config::load().get("window_mode") == "tile"
}

pub fn on_new_window(id: i64) {
    if tile_mode() {
        return; // desktop-style: new windows share the screen
    }
    let Some(scr) = sway::screen(shell::shell_output_name().as_deref()) else {
        return;
    };
    let wins = scr.root.windows();
    if wins.len() > 1 && wins.iter().any(|w| w.id == id) {
        let k = sway::free_workspace();
        sway::run(&format!(
            "[con_id={id}] move container to workspace number {k}; workspace number {k}"
        ));
    }
}

/// Before launching an app: switch the main screen to an empty workspace so
/// the new window gets the whole screen instead of squeezing in next to another.
pub fn prepare_launch(output: Option<&str>) {
    if tile_mode() {
        if let Some(o) = output {
            sway::run(&format!("focus output {o}"));
        }
        return;
    }
    let scr = sway::screen(output);
    if scr.map(|s| s.root.windows().is_empty()).unwrap_or(false) {
        return;
    }
    let k = sway::free_workspace();
    let focus = output
        .map(|o| format!("focus output {o}; "))
        .unwrap_or_default();
    sway::run(&format!("{focus}workspace number {k}"));
}

/// The sway window for one of our window-list entries.
pub fn find(t: &Toplevel) -> Option<i64> {
    let all = sway::all_windows();
    all.iter()
        .find(|w| w.app_id == t.app_id && w.title == t.title)
        .or_else(|| {
            let same: Vec<&Win> = all.iter().filter(|w| w.app_id == t.app_id).collect();
            (same.len() == 1).then(|| same[0])
        })
        .or_else(|| {
            all.iter()
                .find(|w| !t.title.is_empty() && w.title == t.title)
        })
        .map(|w| w.id)
}

/// Is this window on the main screen's visible workspace?
pub fn on_main_screen(t: &Toplevel) -> bool {
    let Some(scr) = sway::screen(shell::shell_output_name().as_deref()) else {
        return true;
    };
    let Some(id) = find(t) else { return true };
    scr.root.windows().iter().any(|w| w.id == id)
}

/// Windows visible on the main screen right now, with their screen areas.
pub fn visible_windows() -> Vec<Win> {
    sway::screen(shell::shell_output_name().as_deref())
        .map(|s| s.root.windows())
        .unwrap_or_default()
}

/// Nearest edge of a w x h box to the point (x, y) inside it.
pub fn nearest_side(x: f64, y: f64, w: f64, h: f64) -> Side {
    let rx = x / w.max(1.0);
    let ry = y / h.max(1.0);
    [
        (rx, Side::Left),
        (1.0 - rx, Side::Right),
        (ry, Side::Top),
        (1.0 - ry, Side::Bottom),
    ]
    .iter()
    .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
    .map(|e| e.1)
    .unwrap_or(Side::Right)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// Put window `d` on `side` of window `t`.
pub fn dock(d: i64, t: i64, side: Side) {
    if d == t {
        return;
    }
    let want_h = matches!(side, Side::Left | Side::Right);
    let parent_h = parent_layout(t).unwrap_or(true);
    let mut cmd = String::new();
    if parent_h != want_h {
        cmd += &format!(
            "[con_id={t}] split {}; ",
            if want_h { "horizontal" } else { "vertical" }
        );
    }
    cmd += &format!(
        "[con_id={t}] mark --add _rt_dock; [con_id={d}] move container to mark _rt_dock; [con_id={t}] unmark _rt_dock"
    );
    sway::run(&cmd);
    // "move to mark" lands after the target: swap to get left / top
    if matches!(side, Side::Left | Side::Top) {
        sway::run(&format!("[con_id={d}] swap container with con_id {t}"));
    }
}

/// Put window `d` on `side` of the whole group (workspace) `num` - e.g. to
/// the right of two windows stacked on top of each other.
pub fn dock_group(d: i64, num: i64, side: Side) {
    let out = shell::shell_output_name();
    let find = || {
        sway::groups(out.as_deref())
            .into_iter()
            .find(|g| g.num == num)
    };
    let Some(g) = find() else { return };
    // dragged out of this same group: take it out first
    if g.root.windows().iter().any(|w| w.id == d) {
        let k = sway::free_workspace();
        sway::run(&format!(
            "[con_id={d}] move container to workspace number {k}"
        ));
    }
    let Some(g) = find() else {
        // it was the group's only window
        return;
    };
    let want_h = matches!(side, Side::Left | Side::Right);
    let Node::Split {
        horizontal,
        children,
        ..
    } = &g.root
    else {
        return;
    };
    let before = matches!(side, Side::Left | Side::Top);
    if *horizontal == want_h && children.len() > 1 {
        // already a row (or column) the right way: join it at the start/end
        let anchor = if before {
            children.first()
        } else {
            children.last()
        };
        let Some(anchor) = anchor.map(|c| c.id()) else {
            return;
        };
        sway::run(&format!(
            "[con_id={anchor}] mark --add _rt_dock; [con_id={d}] move container to mark _rt_dock; [con_id={anchor}] unmark _rt_dock"
        ));
        if before {
            sway::run(&format!("[con_id={d}] swap container with con_id {anchor}"));
        }
        return;
    }
    // wrap everything in the group into one container, turn the group the
    // other way, then add the window next to that container
    let Some(w) = g.root.windows().first().cloned() else {
        return;
    };
    let ups = g.root.depth_of(w.id).unwrap_or(1).max(1);
    let mut cmd = format!("[con_id={}] focus", w.id);
    for _ in 0..ups {
        cmd += "; focus parent";
    }
    cmd += if want_h {
        "; split horizontal"
    } else {
        "; split vertical"
    };
    sway::run(&cmd);
    sway::run(&format!(
        "[con_id={d}] move container to workspace number {num}"
    ));
    if before {
        if let Some(Node::Split { children, .. }) = find().map(|g| g.root) {
            if let Some(first) = children.first().map(|c| c.id()).filter(|&f| f != d) {
                sway::run(&format!("[con_id={d}] swap container with con_id {first}"));
            }
        }
    }
}

/// Is the split holding `id` side by side? (visible workspaces on all monitors)
fn parent_layout(id: i64) -> Option<bool> {
    for out in crate::screens::all_outputs() {
        if let Some(s) = sway::screen(Some(&out)) {
            if let Some(Node::Split { horizontal, .. }) = s.root.parent_of(id) {
                return Some(*horizontal);
            }
        }
    }
    None
}

// ---------------------------------------------------------------- dividers

fn dividers(n: &Node, out: &mut Vec<Divider>) {
    let Node::Split {
        horizontal,
        rect,
        children,
        ..
    } = n
    else {
        return;
    };
    for pair in children.windows(2) {
        let (a, b) = (pair[0].rect(), pair[1].rect());
        out.push(if *horizontal {
            Divider {
                a: pair[0].id(),
                side_by_side: true,
                pos: (a.x + a.w + b.x) / 2,
                start: rect.y,
                len: rect.h,
                lo: a.x,
                hi: b.x + b.w,
            }
        } else {
            Divider {
                a: pair[0].id(),
                side_by_side: false,
                pos: (a.y + a.h + b.y) / 2,
                start: rect.x,
                len: rect.w,
                lo: a.y,
                hi: b.y + b.h,
            }
        });
    }
    children.iter().for_each(|c| dividers(c, out));
}

thread_local! {
    static LAYOUT_PENDING: Cell<bool> = const { Cell::new(false) };
}

/// sway sends bursts of events (every title change of a terminal or a loading
/// web page): look at the layout once per burst, not once per event, so the
/// shell never stalls mid-animation.
pub fn layout_changed(shell: &Shell) {
    if LAYOUT_PENDING.with(|p| p.replace(true)) {
        return;
    }
    let s = shell.clone();
    let _ = glib::timeout_add_local_once(Duration::from_millis(150), move || {
        LAYOUT_PENDING.with(|p| p.set(false));
        s.sync_home(sway::showing_nothing(shell::shell_output_name().as_deref()));
        refresh(&s, true);
    });
}

/// Show/hide/rebuild the handles. Cheap when nothing changed; called on sway
/// events and from the shell's event pump.
pub fn refresh(shell: &Shell, force: bool) {
    if !enabled(shell) {
        return;
    }
    // hidden the moment a sheet starts sliding in (they'd sit on top of it)
    let show =
        !shell.home_visible() && !shell.any_showing() && !shell.is_locked() && !shell.is_blanked();
    let (dragging, arranging) = STATE.with(|s| {
        let s = s.borrow();
        (s.dragging, s.arrange.is_some())
    });
    if dragging || arranging {
        return;
    }
    if !show {
        set_layout(shell, Vec::new(), Rect::default()); // no sway call: cheap
        return;
    }
    let now = glib::monotonic_time();
    let wait = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let skip = !force && now - s.last_poll < 400_000;
        if !skip {
            s.last_poll = now;
        }
        skip
    });
    if wait {
        return;
    }
    let Some(scr) = sway::screen(shell::shell_output_name().as_deref()) else {
        return;
    };
    let mut list = Vec::new();
    dividers(&scr.root, &mut list);
    set_layout(shell, list, scr.output);
}

fn set_layout(shell: &Shell, list: Vec<Divider>, output: Rect) {
    let same = STATE.with(|s| {
        let s = s.borrow();
        s.layout == list && s.output == output
    });
    if same {
        return;
    }
    let old = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.layout = list.clone();
        s.output = output;
        std::mem::take(&mut s.dividers)
    });
    for w in old {
        w.destroy();
    }
    let wins: Vec<_> = list
        .iter()
        .map(|d| divider_window(shell, d, output))
        .collect();
    STATE.with(|s| s.borrow_mut().dividers = wins);
}

fn overlay_window(shell: &Shell, title: &str, layer_: Layer, a: Anchors) -> gtk::ApplicationWindow {
    let app = gio::Application::default()
        .and_downcast::<gtk::Application>()
        .expect("gtk app");
    let w = shell::shell_window(&app, title);
    layer::setup(&w, layer_, a, Zone::Fixed(-1), KeyboardMode::None);
    layer::set_monitor(&w, shell::shell_monitor().as_ref());
    let _ = shell;
    w
}

fn divider_window(shell: &Shell, d: &Divider, out: Rect) -> gtk::ApplicationWindow {
    let a = Anchors {
        top: true,
        left: true,
        bottom: false,
        right: false,
    };
    let w = overlay_window(shell, "rusttouch split handle", Layer::Top, a);
    let (width, height, left, top) = if d.side_by_side {
        (GRAB, d.len, d.pos - out.x - GRAB / 2, d.start - out.y)
    } else {
        (d.len, GRAB, d.start - out.x, d.pos - out.y - GRAB / 2)
    };
    layer::set_margins(&w, top, 0, 0, left);
    w.set_default_size(width, height);

    let area = gtk::Box::new(gtk::Orientation::Vertical, 0);
    area.add_css_class("split-divider");
    area.set_size_request(width, height);
    let pill = gtk::Box::new(gtk::Orientation::Vertical, 0);
    pill.add_css_class("split-pill");
    if d.side_by_side {
        pill.set_size_request(6, 64);
    } else {
        pill.set_size_request(64, 6);
    }
    pill.set_halign(gtk::Align::Center);
    pill.set_valign(gtk::Align::Center);
    pill.set_vexpand(true);
    area.append(&pill);
    w.set_child(Some(&area));

    // drag = resize (live, a few times a second), tap = arrange
    let g = gtk::GestureDrag::new();
    let last = Rc::new(Cell::new(0i64));
    let moved = Rc::new(Cell::new(false));
    {
        let (s, moved) = (shell.clone(), moved.clone());
        let d = d.clone();
        g.connect_drag_begin(move |_, _, _| {
            moved.set(false);
            STATE.with(|st| st.borrow_mut().dragging = true);
            show_guide(&s, &d, f64::from(d.pos));
        });
    }
    {
        let (s, last, moved) = (shell.clone(), last.clone(), moved.clone());
        let d = d.clone();
        g.connect_drag_update(move |_, dx, dy| {
            let off = if d.side_by_side { dx } else { dy };
            if off.abs() > 8.0 {
                moved.set(true);
            }
            if !moved.get() {
                return;
            }
            let p = clamp_pos(&d, f64::from(d.pos) + off);
            show_guide(&s, &d, p);
            let now = glib::monotonic_time();
            if now - last.get() > 90_000 {
                last.set(now);
                resize(&d, p);
            }
        });
    }
    {
        let s = shell.clone();
        let d = d.clone();
        g.connect_drag_end(move |_, dx, dy| {
            hide_guide();
            STATE.with(|st| st.borrow_mut().dragging = false);
            if moved.get() {
                let off = if d.side_by_side { dx } else { dy };
                resize(&d, clamp_pos(&d, f64::from(d.pos) + off));
                let s2 = s.clone();
                let _ = glib::timeout_add_local_once(Duration::from_millis(150), move || {
                    refresh(&s2, true)
                });
            } else {
                arrange(&s);
            }
        });
    }
    area.add_controller(g);
    w.set_cursor_from_name(Some(if d.side_by_side {
        "col-resize"
    } else {
        "row-resize"
    }));
    w.present();
    w
}

fn clamp_pos(d: &Divider, p: f64) -> f64 {
    p.clamp(
        f64::from(d.lo + MIN),
        f64::from((d.hi - MIN).max(d.lo + MIN)),
    )
}

fn resize(d: &Divider, p: f64) {
    let size = (p as i32 - d.lo - GAP / 2).max(MIN);
    sway::run(&format!(
        "[con_id={}] resize set {} {size} px",
        d.a,
        if d.side_by_side { "width" } else { "height" }
    ));
}

/// A line that follows your finger while resizing (the handle itself stays
/// put until you let go, so touch coordinates stay steady).
fn show_guide(shell: &Shell, d: &Divider, p: f64) {
    let exists = STATE.with(|s| s.borrow().guide.is_some());
    if !exists {
        let w = overlay_window(shell, "rusttouch split guide", Layer::Overlay, Anchors::ALL);
        w.set_can_target(false);
        let pos = Rc::new(Cell::new((true, 0.0)));
        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_can_target(false);
        let p2 = pos.clone();
        area.set_draw_func(move |a, cr, w, h| {
            let (vertical, at) = p2.get();
            let c = a.color();
            cr.set_source_rgba(
                f64::from(c.red()),
                f64::from(c.green()),
                f64::from(c.blue()),
                0.9,
            );
            if vertical {
                cr.rectangle(at - 3.0, 0.0, 6.0, f64::from(h));
            } else {
                cr.rectangle(0.0, at - 3.0, f64::from(w), 6.0);
            }
            let _ = cr.fill();
        });
        area.add_css_class("split-guide");
        w.set_child(Some(&area));
        STATE.with(|s| s.borrow_mut().guide = Some((w, area, pos)));
    }
    STATE.with(|s| {
        let s = s.borrow();
        if let Some((w, area, pos)) = s.guide.as_ref() {
            let local = if d.side_by_side {
                p - f64::from(s.output.x)
            } else {
                p - f64::from(s.output.y)
            };
            pos.set((d.side_by_side, local));
            area.queue_draw();
            if !w.is_visible() {
                w.present();
            }
        }
    });
}

fn hide_guide() {
    STATE.with(|s| {
        if let Some((w, _, _)) = s.borrow().guide.as_ref() {
            w.set_visible(false);
        }
    });
}

// ---------------------------------------------------------------- arrange

/// Full-screen "arrange" view: each window as a card in its real place.
pub fn arrange(shell: &Shell) {
    close_arrange();
    let Some(scr) = sway::screen(shell::shell_output_name().as_deref()) else {
        return;
    };
    let w = overlay_window(shell, "rusttouch arrange", Layer::Overlay, Anchors::ALL);
    w.add_css_class("arrange");
    w.set_child(Some(&arrange_content(shell, &scr)));
    w.present();
    STATE.with(|s| s.borrow_mut().arrange = Some(w));
    set_layout(shell, Vec::new(), Rect::default()); // handles would sit on top
}

pub fn close_arrange() -> bool {
    let w = STATE.with(|s| s.borrow_mut().arrange.take());
    let had = w.is_some();
    if let Some(w) = w {
        w.destroy();
    }
    had
}

fn rearrange_soon(shell: &Shell) {
    let s = shell.clone();
    let _ = glib::timeout_add_local_once(Duration::from_millis(180), move || {
        let open = STATE.with(|st| st.borrow().arrange.is_some());
        if !open {
            return;
        }
        match sway::screen(shell::shell_output_name().as_deref()) {
            Some(scr) if scr.root.windows().len() > 1 => {
                STATE.with(|st| {
                    if let Some(w) = st.borrow().arrange.as_ref() {
                        w.set_child(Some(&arrange_content(&s, &scr)));
                    }
                });
            }
            _ => {
                close_arrange();
                refresh(&s, true);
            }
        }
    });
}

#[derive(Clone, Copy, PartialEq)]
enum DropAt {
    Swap(i64),
    Dock(i64, Side),
}

fn arrange_content(shell: &Shell, scr: &Screen) -> gtk::Widget {
    let out = scr.output;
    let wins = scr.root.windows();
    let local = move |r: Rect| Rect {
        x: r.x - out.x,
        y: r.y - out.y,
        w: r.w,
        h: r.h,
    };

    let fixed = gtk::Fixed::new();
    fixed.set_hexpand(true);
    fixed.set_vexpand(true);
    let hint = gtk::Box::new(gtk::Orientation::Vertical, 0);
    hint.add_css_class("drop-hint");
    hint.set_visible(false);
    hint.set_can_target(false);

    for win in &wins {
        let r = local(win.rect);
        let tile = gtk::Box::new(gtk::Orientation::Vertical, 8);
        tile.add_css_class("arrange-tile");
        let inset = 10;
        tile.set_size_request((r.w - 2 * inset).max(60), (r.h - 2 * inset).max(60));
        let icon = apps::icon_image(apps::icon_for_app_id(&win.app_id).as_ref(), 56);
        icon.set_vexpand(true);
        icon.set_valign(gtk::Align::End);
        tile.append(&icon);
        let title = gtk::Label::new(Some(if win.title.is_empty() {
            &win.app_id
        } else {
            &win.title
        }));
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_max_width_chars(24);
        tile.append(&title);

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        buttons.set_halign(gtk::Align::Center);
        buttons.set_vexpand(true);
        buttons.set_valign(gtk::Align::Start);
        let id = win.id;
        let add = |label: &str, cmd: Rc<dyn Fn()>| {
            let b = gtk::Button::with_label(label);
            b.add_css_class("pill");
            b.add_css_class("arrange-btn");
            b.connect_clicked(move |_| cmd());
            buttons.append(&b);
        };
        let s = shell.clone();
        add(
            "Flip",
            Rc::new(move || {
                sway::run(&format!("[con_id={id}] layout toggle split"));
                rearrange_soon(&s);
            }),
        );
        let s = shell.clone();
        add(
            "Full",
            Rc::new(move || {
                let k = sway::free_workspace();
                sway::run(&format!(
                    "[con_id={id}] move container to workspace number {k}; workspace number {k}"
                ));
                close_arrange();
                refresh(&s, true);
            }),
        );
        let s = shell.clone();
        add(
            "Close",
            Rc::new(move || {
                sway::run(&format!("[con_id={id}] kill"));
                rearrange_soon(&s);
            }),
        );
        tile.append(&buttons);
        fixed.put(&tile, f64::from(r.x + inset), f64::from(r.y + inset));

        // drag the card onto another window
        let g = gtk::GestureDrag::new();
        let target: Rc<Cell<Option<DropAt>>> = Rc::new(Cell::new(None));
        let start = (f64::from(r.x + inset), f64::from(r.y + inset));
        let claimed = Rc::new(Cell::new(false));
        {
            let claimed = claimed.clone();
            g.connect_drag_begin(move |_, _, _| claimed.set(false));
        }
        {
            let (fixed, tile, hint, target, claimed) = (
                fixed.clone(),
                tile.clone(),
                hint.clone(),
                target.clone(),
                claimed.clone(),
            );
            let wins = wins.clone();
            g.connect_drag_update(move |g, dx, dy| {
                if !claimed.get() {
                    if dx.abs().max(dy.abs()) < 12.0 {
                        return;
                    }
                    claimed.set(true);
                    let _ = g.set_state(gtk::EventSequenceState::Claimed);
                    tile.add_css_class("dragging");
                }
                fixed.move_(&tile, start.0 + dx, start.1 + dy);
                let (sx, sy) = g.start_point().unwrap_or((0.0, 0.0));
                // finger position in output coordinates
                let fx = start.0 + sx + dx + f64::from(out.x);
                let fy = start.1 + sy + dy + f64::from(out.y);
                let t = drop_at(&wins, id, fx, fy);
                target.set(t);
                match t.and_then(|t| hint_rect(&wins, t)) {
                    Some(hr) => {
                        let hr = local(hr);
                        hint.set_size_request(hr.w, hr.h);
                        fixed.move_(&hint, f64::from(hr.x), f64::from(hr.y));
                        hint.set_visible(true);
                    }
                    None => hint.set_visible(false),
                }
            });
        }
        {
            let (fixed, tile, hint, s) = (fixed.clone(), tile.clone(), hint.clone(), shell.clone());
            g.connect_drag_end(move |_, _, _| {
                hint.set_visible(false);
                tile.remove_css_class("dragging");
                if !claimed.get() {
                    return;
                }
                match target.take() {
                    Some(DropAt::Swap(t)) => {
                        sway::run(&format!("[con_id={id}] swap container with con_id {t}"));
                    }
                    Some(DropAt::Dock(t, side)) => dock(id, t, side),
                    None => fixed.move_(&tile, start.0, start.1),
                }
                rearrange_soon(&s);
            });
        }
        tile.add_controller(g);
    }
    fixed.put(&hint, 0.0, 0.0);

    let help = gtk::Label::new(Some(
        "Drag a window onto another to swap them, or onto its edge to dock beside it",
    ));
    help.add_css_class("arrange-help");
    help.set_wrap(true);
    help.set_justify(gtk::Justification::Center);
    help.set_halign(gtk::Align::Center);
    help.set_valign(gtk::Align::Start);
    help.set_margin_top(12);
    help.set_can_target(false);

    let done = gtk::Button::with_label("Done");
    done.add_css_class("pill");
    done.add_css_class("arrange-done");
    done.set_halign(gtk::Align::Center);
    done.set_valign(gtk::Align::End);
    done.set_margin_bottom(24);
    let s = shell.clone();
    done.connect_clicked(move |_| {
        close_arrange();
        refresh(&s, true);
    });

    let ov = gtk::Overlay::new();
    ov.set_child(Some(&fixed));
    ov.add_overlay(&help);
    ov.add_overlay(&done);
    ov.upcast()
}

/// What dropping at (x, y) would do: middle of a window = swap, near an edge = dock.
fn drop_at(wins: &[Win], me: i64, x: f64, y: f64) -> Option<DropAt> {
    let t = wins.iter().find(|w| w.id != me && w.rect.contains(x, y))?;
    let r = t.rect;
    let rx = (x - f64::from(r.x)) / f64::from(r.w.max(1));
    let ry = (y - f64::from(r.y)) / f64::from(r.h.max(1));
    if (0.3..0.7).contains(&rx) && (0.3..0.7).contains(&ry) {
        return Some(DropAt::Swap(t.id));
    }
    // nearest edge
    let edges = [
        (rx, Side::Left),
        (1.0 - rx, Side::Right),
        (ry, Side::Top),
        (1.0 - ry, Side::Bottom),
    ];
    let side = edges
        .iter()
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|e| e.1)?;
    Some(DropAt::Dock(t.id, side))
}

fn hint_rect(wins: &[Win], d: DropAt) -> Option<Rect> {
    let (id, side) = match d {
        DropAt::Swap(id) => (id, None),
        DropAt::Dock(id, s) => (id, Some(s)),
    };
    let r = wins.iter().find(|w| w.id == id)?.rect;
    Some(match side {
        None => r,
        Some(Side::Left) => Rect { w: r.w / 2, ..r },
        Some(Side::Right) => Rect {
            x: r.x + r.w / 2,
            w: r.w / 2,
            ..r
        },
        Some(Side::Top) => Rect { h: r.h / 2, ..r },
        Some(Side::Bottom) => Rect {
            y: r.y + r.h / 2,
            h: r.h / 2,
            ..r
        },
    })
}

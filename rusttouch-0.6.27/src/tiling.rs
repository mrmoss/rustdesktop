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
    /// ✕ buttons for windows without a title bar: (window id, its area)
    closers: Vec<(i64, Rect)>,
    output: Rect,
    dragging: bool,
    arrange: Option<gtk::ApplicationWindow>,
    last_poll: i64,
    last_sync: i64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn enabled(shell: &shell::ShellInner) -> bool {
    shell.layered && sway::available()
}

/// Watch sway for new windows / layout changes.
/// Mice and touchpads: the pointer stays on the main screen (the other
/// screens only show a copy, so it used to wander off and "wrap" onto them),
/// and the scroll direction from Settings > Mouse.
pub fn set_pointer(shell: &Shell, main: Option<&str>) {
    if !enabled(shell) {
        return;
    }
    let natural = if shell.cfg().on("mouse_natural_scroll") {
        "enabled"
    } else {
        "disabled"
    };
    let mut cmd = format!(
        "input type:pointer natural_scroll {natural}; input type:touchpad natural_scroll {natural}"
    );
    if let Some(m) =
        main.filter(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    {
        cmd += &format!(
            "; input type:pointer map_to_output {m}; input type:touchpad map_to_output {m}"
        );
    }
    sway::run(&cmd);
}

pub fn start(shell: &Shell) {
    if !enabled(shell) {
        return;
    }
    eprintln!("rusttouch: sway found - split screen available");
    set_pointer(shell, shell::shell_output_name().as_deref());
    // Apps that can draw their own title bar (most GTK/Qt apps) are asked to,
    // so they get a close button - sway's own title bars have none.
    sway::run(r#"for_window [shell="xdg_shell"] border csd"#);
    let tx = shell.tx();
    sway::watch_windows(move |change, c| {
        if change == "new" && c["type"] == "con" && c["app_id"] != MIRROR_APP {
            if let Some(id) = c["id"].as_i64() {
                let _ = tx.send(Event::NewWindow(id));
                crate::shell::wake();
            }
        }
        // titles don't move anything
        if change != "title" && change != "mark" && change != "urgent" {
            let _ = tx.send(Event::Layout);
            crate::shell::wake();
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
    // "move to mark" puts it after the target: for left / top, move the
    // target past it (two windows side by side just trade places)
    if matches!(side, Side::Left | Side::Top) {
        sway::run(&format!(
            "[con_id={t}] move {}",
            if want_h { "right" } else { "down" }
        ));
    }
}

/// Put window `d` on `side` of the whole group (workspace) `num`: it gets
/// that half of the screen (left, right, top or bottom), and the group, as it
/// was, the other half - e.g. two windows stacked on top of each other stay
/// stacked, on the right of the new one.
///
/// How sway places things decides the steps (checked against sway's source):
/// "move container to workspace" puts a window next to whatever was last
/// focused there - often *inside* a stack - and "move to mark" on a
/// container puts it inside too. Only moving something onto a single window
/// reliably makes them neighbours. So:
///  1. wrap the whole group into one container C (and turn the workspace
///     the way the new window goes);
///  2. park C on a spare workspace, so the group's workspace is empty;
///  3. move the new window into the empty group workspace (now its only
///     window);
///  4. wrap the parked group again and move it onto the new window: they
///     become neighbours, the group after the window;
///  5. for right / bottom, swap the two.
pub fn dock_group(d: i64, num: i64, side: Side) {
    let out = shell::shell_output_name();
    let group = |n: i64| {
        sway::groups(out.as_deref())
            .into_iter()
            .find(|g| g.num == n)
    };
    let Some(g) = group(num) else { return };
    // dragged out of this same group: take it out first
    if g.root.windows().iter().any(|w| w.id == d) {
        let k = sway::free_workspace();
        sway::run(&format!(
            "[con_id={d}] move container to workspace number {k}"
        ));
    }
    let want_h = matches!(side, Side::Left | Side::Right);
    let split = if want_h {
        "split horizontal"
    } else {
        "split vertical"
    };
    // wrap everything on workspace `n` into one container; its id
    let wrap = |n: i64, how: &str| -> Option<i64> {
        let g = group(n)?;
        let w = g.root.windows().first().cloned()?;
        let ups = g.root.depth_of(w.id).unwrap_or(1).max(1);
        let mut cmd = format!("[con_id={}] focus", w.id);
        for _ in 0..ups {
            cmd += "; focus parent";
        }
        cmd += &format!("; {how}");
        sway::run(&cmd);
        match group(n)?.root {
            Node::Split { children, .. } if children.len() == 1 => Some(children[0].id()),
            _ => None,
        }
    };
    // 1. the group as one container
    let Some(c) = wrap(num, split) else {
        eprintln!("rusttouch: couldn't gather window group {num} into one container");
        return;
    };
    // 2. parked on a spare workspace
    let spare = sway::free_workspace();
    sway::run(&format!(
        "[con_id={c}] move container to workspace number {spare}"
    ));
    // 3. the new window alone on the group's workspace
    sway::run(&format!(
        "[con_id={d}] move container to workspace number {num}"
    ));
    // 4. the group back, next to (after) the new window
    let Some(c2) = wrap(spare, "split horizontal") else {
        return;
    };
    sway::run(&format!(
        "[con_id={c2}] move container to workspace number {num}; [con_id={d}] focus"
    ));
    // 5. right / bottom: the window goes after the group
    if !matches!(side, Side::Left | Side::Top) {
        sway::run(&format!("[con_id={d}] swap container with con_id {c2}"));
    }
}

/// A window-group switch that follows your finger (swipe along the button
/// bar): the group you're leaving and the next one, drawn from their recents
/// screenshots, move 1:1 with the finger; let go to finish or snap back.
pub struct SlideDrag {
    win: gtk::ApplicationWindow,
    fixed: gtk::Fixed,
    old: gtk::Widget,
    new: gtk::Widget,
    /// screen width, and +1 = next group comes in from the right
    w: f64,
    dir: f64,
    num: i64,
}

impl SlideDrag {
    /// `forward` = towards the next group (finger moving left).
    pub fn begin(shell: &Shell, forward: bool) -> Option<SlideDrag> {
        let out = shell::shell_output_name();
        let (scr, mut groups) = sway::screen_and_groups(out.as_deref())?;
        groups.sort_by_key(|g| g.num);
        let cur = scr.num;
        let target = if forward {
            groups.iter().find(|g| g.num > cur).or(groups.first())
        } else {
            groups.iter().rev().find(|g| g.num < cur).or(groups.last())
        }?
        .clone();
        if target.num == cur {
            return None;
        }
        let area = scr.area;
        let (w, h) = (area.w.max(1), area.h.max(1));
        let old = group_picture(&scr.root, area, w, h, shell);
        let new = group_picture(&target.root, target.rect, w, h, shell);
        let a = Anchors {
            top: true,
            left: true,
            bottom: false,
            right: false,
        };
        let win = overlay_window(shell, "rusttouch group switch", Layer::Top, a);
        layer::set_margins(&win, area.y - scr.output.y, 0, 0, area.x - scr.output.x);
        win.set_default_size(w, h);
        win.add_css_class("switch-bg");
        let fixed = gtk::Fixed::new();
        fixed.set_size_request(w, h);
        fixed.set_overflow(gtk::Overflow::Hidden);
        let dir = if forward { 1.0 } else { -1.0 };
        let fw = f64::from(w);
        fixed.put(&old, 0.0, 0.0);
        fixed.put(&new, dir * fw, 0.0);
        win.set_child(Some(&fixed));
        win.present();
        Some(SlideDrag {
            win,
            fixed,
            old,
            new,
            w: fw,
            dir,
            num: target.num,
        })
    }

    /// Finger moved: `dx` px since the swipe started.
    pub fn set(&self, dx: f64) {
        // only towards the chosen group; pulling back stops at the start
        let travel = (-dx * self.dir).clamp(0.0, self.w);
        let p = travel / self.w;
        self.place(p);
    }

    fn place(&self, p: f64) {
        self.fixed.move_(&self.old, -self.dir * self.w * p, 0.0);
        self.fixed
            .move_(&self.new, self.dir * self.w * (1.0 - p), 0.0);
    }

    fn progress(&self, dx: f64) -> f64 {
        ((-dx * self.dir) / self.w).clamp(0.0, 1.0)
    }

    /// Finger lifted: finish when far enough or flicked, else snap back.
    /// `vel` = px/s (negative = moving left).
    pub fn end(self, dx: f64, vel: f64) {
        let p = self.progress(dx);
        let flick = -vel * self.dir; // positive = towards the target
        let commit = p > 0.3 || flick > 600.0;
        if commit {
            // the real windows switch underneath while we finish the slide
            sway::run(&format!("workspace number {}", self.num));
        }
        let to = if commit { 1.0 } else { 0.0 };
        let ms = (((to - p) as f64).abs() * 220.0).max(80.0) as u32;
        let fixed = self.fixed.clone();
        let win = self.win.clone();
        let this = Rc::new(self);
        let _ = crate::anim::run(
            &fixed,
            ms,
            move |e| this.place(p + (to - p) * e),
            move || {
                let _ = glib::timeout_add_local_once(
                    Duration::from_millis(if commit { 50 } else { 0 }),
                    move || win.destroy(),
                );
            },
        );
    }
}

/// Switch to the next/previous group without a finger (`rusttouch --do`).
pub fn slide_switch(shell: &Shell, forward: bool) {
    if let Some(d) = SlideDrag::begin(shell, forward) {
        let dx = -d.dir * d.w; // "dragged" all the way
        let _ =
            glib::timeout_add_local_once(Duration::from_millis(30), move || d.end(dx * 0.31, 0.0));
    }
}

/// Read the screenshots a swipe would need ahead of time (on a worker thread).
pub fn warm_pictures(shell: &Shell) {
    let out = shell::shell_output_name();
    let paths = sway::groups(out.as_deref())
        .iter()
        .flat_map(|g| g.root.windows())
        .filter_map(|w| shell.con_preview(w.id))
        .collect();
    crate::pics::prefetch(paths);
}

/// A group drawn from its windows' screenshots, w x h.
fn group_picture(root: &Node, area: Rect, w: i32, h: i32, shell: &Shell) -> gtk::Widget {
    let fixed = gtk::Fixed::new();
    fixed.set_size_request(w, h);
    fixed.add_css_class("switch-bg");
    let wins = root.windows();
    if wins.is_empty() {
        // the home screen
        let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
        b.add_css_class("home");
        b.set_size_request(w, h);
        fixed.put(&b, 0.0, 0.0);
        return fixed.upcast();
    }
    let sx = f64::from(w) / f64::from(area.w.max(1));
    let sy = f64::from(h) / f64::from(area.h.max(1));
    for win in wins {
        let rw = ((f64::from(win.rect.w)) * sx) as i32;
        let rh = ((f64::from(win.rect.h)) * sy) as i32;
        let content = crate::pics::widget(shell.con_preview(win.id), rw.max(1), rh.max(1))
            .unwrap_or_else(|| crate::recents::placeholder(&win.app_id, 96, false));
        content.set_size_request(rw.max(1), rh.max(1));
        fixed.put(
            &content,
            f64::from(win.rect.x - area.x) * sx,
            f64::from(win.rect.y - area.y) * sy,
        );
    }
    fixed.upcast()
}

/// Show the most recently used group other than the one showing (Android:
/// tap ■ twice). `mru` = window list, most recent first.
pub fn last_group(mru: &[Toplevel], home_showing: bool) {
    let groups = sway::groups(shell::shell_output_name().as_deref());
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
    let mut ordered: Vec<&sway::Group> = groups.iter().collect();
    ordered.sort_by_key(|g| rank(g));
    let pick = if home_showing {
        ordered.first()
    } else {
        ordered.get(1)
    };
    let Some(g) = pick else { return };
    // its most recently used window
    let wins = g.root.windows();
    let best = wins
        .iter()
        .min_by_key(|w| {
            mru.iter()
                .position(|t| t.app_id == w.app_id && t.title == w.title)
                .unwrap_or(usize::MAX)
        })
        .or(wins.first());
    if let Some(w) = best {
        sway::run(&format!("[con_id={}] focus", w.id));
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
        let alive: Vec<i64> = sway::all_windows().iter().map(|w| w.id).collect();
        s.prune_con_previews(&alive);
        // keep previews fresh after the layout changed under the user's eyes
        if !s.home_visible() && !s.any_showing() {
            let _ = glib::timeout_add_local_once(Duration::from_millis(250), {
                let s = s.clone();
                move || shell::capture_quietly(&s)
            });
        }
        refresh(&s, true);
    });
}

/// Show/hide/rebuild the handles. Cheap when nothing changed; called on sway
/// events and from the shell's event pump.
pub fn refresh(shell: &Shell, force: bool) {
    if !enabled(shell) {
        return;
    }
    // the home flag can go stale (a window shown some other way): check it
    // against sway now and then, so auto-hide bars don't stay up
    let now = glib::monotonic_time();
    let resync = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let due = now - s.last_sync > 1_000_000;
        if due {
            s.last_sync = now;
        }
        due
    });
    if resync && shell.home_visible() && !shell.any_showing() {
        shell.sync_home(sway::showing_nothing(shell::shell_output_name().as_deref()));
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
        set_layout(shell, Vec::new(), Vec::new(), Rect::default()); // no sway call: cheap
        return;
    }
    let now = glib::monotonic_time();
    let wait = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let skip = !force && now - s.last_poll < 1_500_000;
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
    // X11 apps have no title bar under sway: give them a ✕
    let closers: Vec<(i64, Rect)> = scr
        .root
        .windows()
        .iter()
        .filter(|w| w.xwayland)
        .map(|w| (w.id, w.rect))
        .collect();
    set_layout(shell, list, closers, scr.output);
}

/// Take the split-screen handles away now (going home, a menu opening) -
/// without waiting for the next check, so they don't linger on screen.
pub fn hide_handles(shell: &Shell) {
    if enabled(shell) {
        set_layout(shell, Vec::new(), Vec::new(), Rect::default());
    }
}

fn set_layout(shell: &Shell, list: Vec<Divider>, closers: Vec<(i64, Rect)>, output: Rect) {
    let same = STATE.with(|s| {
        let s = s.borrow();
        s.layout == list && s.closers == closers && s.output == output
    });
    if same {
        return;
    }
    let old = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.layout = list.clone();
        s.closers = closers.clone();
        s.output = output;
        std::mem::take(&mut s.dividers)
    });
    for w in old {
        w.destroy();
    }
    let mut wins: Vec<_> = list
        .iter()
        .map(|d| divider_window(shell, d, output))
        .collect();
    wins.extend(
        closers
            .iter()
            .map(|(id, r)| close_window(shell, *id, *r, output)),
    );
    STATE.with(|s| s.borrow_mut().dividers = wins);
}

/// A small ✕ in the top right corner of a window that has no title bar.
fn close_window(shell: &Shell, id: i64, r: Rect, out: Rect) -> gtk::ApplicationWindow {
    let a = Anchors {
        top: true,
        left: true,
        bottom: false,
        right: false,
    };
    let w = overlay_window(shell, "rusttouch close button", Layer::Top, a);
    let size = 32;
    layer::set_margins(&w, r.y - out.y + 4, 0, 0, r.x - out.x + r.w - size - 4);
    w.set_default_size(size, size);
    let b = gtk::Button::with_label("✕");
    b.add_css_class("win-close");
    b.set_size_request(size, size);
    b.connect_clicked(move |_| {
        sway::run(&format!("[con_id={id}] kill"));
    });
    w.set_child(Some(&b));
    w.present();
    w
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

    // invisible: you grab the black gap itself (no handle drawn on it)
    let area = gtk::Box::new(gtk::Orientation::Vertical, 0);
    area.add_css_class("split-divider");
    area.set_size_request(width, height);
    w.set_child(Some(&area));

    // drag = resize (live, a few times a second), tap = arrange
    let g = gtk::GestureDrag::new();
    let last = Rc::new(Cell::new(0i64));
    let moved = Rc::new(Cell::new(false));
    {
        let moved = moved.clone();
        g.connect_drag_begin(move |_, _, _| {
            moved.set(false);
            STATE.with(|st| st.borrow_mut().dragging = true);
        });
    }
    {
        let (last, moved) = (last.clone(), moved.clone());
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
            let now = glib::monotonic_time();
            if now - last.get() > 40_000 {
                last.set(now);
                resize(&d, p);
            }
        });
    }
    {
        let s = shell.clone();
        let d = d.clone();
        g.connect_drag_end(move |_, dx, dy| {
            STATE.with(|st| st.borrow_mut().dragging = false);
            if moved.get() {
                let off = if d.side_by_side { dx } else { dy };
                resize(&d, clamp_pos(&d, f64::from(d.pos) + off));
                // put the grab area where the windows now meet
                let s2 = s.clone();
                let _ = glib::timeout_add_local_once(Duration::from_millis(40), move || {
                    refresh(&s2, true);
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

/// Resize so the gap between the windows is centred on `p`. Returns where the
/// gap's centre ends up.
fn resize(d: &Divider, p: f64) {
    let size = (p as i32 - d.lo - GAP / 2).max(MIN);
    sway::run(&format!(
        "[con_id={}] resize set {} {size} px",
        d.a,
        if d.side_by_side { "width" } else { "height" }
    ));
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
    shell::sync_escape();
    set_layout(shell, Vec::new(), Vec::new(), Rect::default()); // handles would sit on top
}

pub fn close_arrange() -> bool {
    let w = STATE.with(|s| s.borrow_mut().arrange.take());
    let had = w.is_some();
    if let Some(w) = w {
        w.destroy();
        shell::sync_escape();
    }
    had
}

pub fn arrange_open() -> bool {
    STATE.with(|s| s.borrow().arrange.is_some())
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

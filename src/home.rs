//! Home screen: an Android-style grid of app icons.
//!
//! * Long-press an icon and drag: drop it on a free spot to move it, on
//!   another icon to swap them, or on "Remove" at the top to take it off.
//! * Long-press an app in the drawer: the drawer fades away and you drop the
//!   app into a spot on the home screen.
//! Stored in rusttouch.conf as `home_apps` (portrait) / `home_apps_landscape`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, graphene};

use crate::apps;
use crate::config::Config;
use crate::gestures::{self, LongDrag, Spec};
use crate::shell::{self, Overlay, Shell};

/// Roughly how much room one icon gets.
const CELL_W: i32 = 96;
const CELL_H: i32 = 112;
const ICON: i32 = 56;

pub struct HomeInner {
    shell: Shell,
    pub root: gtk::Overlay,
    remove: gtk::Label,
    grid: gtk::Grid,
    float: gtk::Fixed,
    hint: gtk::Label,
    cells: RefCell<Vec<gtk::Box>>,
    /// slot being dragged (home drag) or app id (from the drawer)
    dragging: RefCell<Option<Dragged>>,
    floating: RefCell<Option<gtk::Widget>>,
    hover: Cell<Option<usize>>,
}

pub type Home = Rc<HomeInner>;

#[derive(Clone)]
enum Dragged {
    Slot(usize),
    New(String),
}

fn app(id: &str) -> Option<gio::AppInfo> {
    gio_unix::DesktopAppInfo::new(id).map(|d| d.upcast())
}

// Layouts are saved per orientation ("home_apps" = portrait,
// "home_apps_landscape"), as `id@col:row` entries, so rotating never shuffles
// your icons: each orientation keeps its own arrangement. The first time you
// rotate, the other orientation starts from this one (same column/row where it
// fits, the rest in free spots). Adding/removing an app applies to both.

const PORTRAIT_KEY: &str = "home_apps";
const LANDSCAPE_KEY: &str = "home_apps_landscape";

fn is_landscape() -> bool {
    let (_, _, w, h) = shell::screen_size();
    w > h
}

fn keys() -> (&'static str, &'static str) {
    if is_landscape() {
        (LANDSCAPE_KEY, PORTRAIT_KEY)
    } else {
        (PORTRAIT_KEY, LANDSCAPE_KEY)
    }
}

/// Entries of one saved layout: (desktop id, Some((col, row)) or None = anywhere).
fn parse(s: &str) -> Vec<(String, Option<(usize, usize)>)> {
    let raw: Vec<&str> = s.split(',').map(str::trim).collect();
    if !s.contains('@') {
        // older format: grid row by row (4 columns), "" = free spot
        return raw
            .iter()
            .enumerate()
            .filter(|(_, id)| !id.is_empty())
            .map(|(i, id)| (id.to_string(), Some((i % 4, i / 4))))
            .collect();
    }
    raw.iter()
        .filter(|e| !e.is_empty())
        .map(|e| match e.rsplit_once('@') {
            Some((id, pos)) => {
                let p = pos
                    .split_once(':')
                    .and_then(|(c, r)| Some((c.parse().ok()?, r.parse().ok()?)));
                (id.to_string(), p)
            }
            None => (e.to_string(), None),
        })
        .collect()
}

/// The cells of the current grid, row by row ("" = free).
fn layout(cfg: &Config) -> Vec<String> {
    let (key, other) = keys();
    let mut items = parse(&cfg.get(key));
    if items.is_empty() {
        items = parse(&cfg.get(other)); // never arranged in this orientation yet
    }
    let (cols, rows) = dims();
    let mut v = vec![String::new(); cols * rows];
    let mut leftover: Vec<String> = Vec::new();
    for (id, pos) in items {
        match pos {
            Some((c, r)) if c < cols && r * cols + c < v.len() && v[r * cols + c].is_empty() => {
                v[r * cols + c] = id;
            }
            _ => leftover.push(id),
        }
    }
    for id in leftover {
        if v.contains(&id) {
            continue;
        }
        match v.iter().position(String::is_empty) {
            Some(i) => v[i] = id,
            None => {
                v.extend(std::iter::repeat(String::new()).take(cols));
                let i = v.iter().position(String::is_empty).unwrap_or(v.len() - 1);
                v[i] = id;
            }
        }
    }
    v
}

/// Save the current grid for this orientation, and carry additions /
/// removals over to the other orientation's layout.
fn save(shell: &Shell, v: Vec<String>) {
    let (cols, _) = dims();
    let (key, other) = keys();
    let here: Vec<String> = v
        .iter()
        .enumerate()
        .filter(|(_, id)| !id.is_empty())
        .map(|(i, id)| format!("{id}@{}:{}", i % cols, i / cols))
        .collect();
    shell.save(key, &here.join(","));

    let cfg = shell.cfg();
    let theirs = parse(&cfg.get(other));
    if !theirs.is_empty() {
        let ids: Vec<&String> = v.iter().filter(|id| !id.is_empty()).collect();
        let mut out: Vec<String> = theirs
            .iter()
            .filter(|(id, _)| ids.contains(&id))
            .map(|(id, pos)| match pos {
                Some((c, r)) => format!("{id}@{c}:{r}"),
                None => format!("{id}@-"),
            })
            .collect();
        for id in ids {
            if !theirs.iter().any(|(t, _)| t == id) {
                out.push(format!("{id}@-")); // new app: first free spot there
            }
        }
        shell.save(other, &out.join(","));
    }
}

/// (columns, rows) that fit the home area.
fn dims() -> (usize, usize) {
    let (_, _, w, h) = shell::screen_size();
    let (top, nav) = crate::theme::bar_sizes(&Config::load());
    let usable_h = (h - top - nav - 60).max(CELL_H * 3);
    let cols = (w / CELL_W).clamp(4, 8) as usize;
    let rows = (usable_h / CELL_H).clamp(3, 12) as usize;
    (cols, rows)
}

fn tile(h: &Home, id: &str) -> Option<gtk::Widget> {
    let info = app(id)?;
    let b = apps::tile(&info, ICON);
    b.add_css_class("home-tile");
    let s = h.shell.clone();
    b.connect_clicked(move |_| apps::launch(&s, &info));
    Some(b.upcast())
}

pub fn render(h: &Home) {
    while let Some(c) = h.grid.first_child() {
        h.grid.remove(&c);
    }
    let v = layout(&h.shell.cfg());
    let (cols, _) = dims();
    let mut cells = Vec::with_capacity(v.len());
    for (i, id) in v.iter().enumerate() {
        let cell = gtk::Box::new(gtk::Orientation::Vertical, 0);
        cell.add_css_class("home-cell");
        cell.set_hexpand(true);
        cell.set_vexpand(true);
        if !id.is_empty() {
            if let Some(t) = tile(h, id) {
                t.set_valign(gtk::Align::Center);
                t.set_vexpand(true);
                cell.append(&t);
            }
        }
        h.grid
            .attach(&cell, (i % cols) as i32, (i / cols) as i32, 1, 1);
        cells.push(cell);
    }
    *h.cells.borrow_mut() = cells;
    h.hint.set_visible(v.iter().all(|x| x.is_empty()));
}

fn bounds_contain(w: &impl IsA<gtk::Widget>, target: &gtk::Overlay, x: f64, y: f64) -> bool {
    w.is_visible()
        && w.compute_bounds(target)
            .map(|r| r.contains_point(&graphene::Point::new(x as f32, y as f32)))
            .unwrap_or(false)
}

fn cell_at(h: &Home, x: f64, y: f64) -> Option<usize> {
    h.cells
        .borrow()
        .iter()
        .position(|c| bounds_contain(c, &h.root, x, y))
}

fn set_hover(h: &Home, i: Option<usize>) {
    if h.hover.get() == i {
        return;
    }
    let cells = h.cells.borrow();
    if let Some(old) = h.hover.get().and_then(|o| cells.get(o)) {
        old.remove_css_class("drop-target");
    }
    if let Some(new) = i.and_then(|n| cells.get(n)) {
        new.add_css_class("drop-target");
    }
    h.hover.set(i);
}

fn start_float(h: &Home, id: &str, x: f64, y: f64) {
    let icon = app(id).and_then(|a| a.icon());
    let img = apps::icon_image(icon.as_ref(), ICON + 8);
    img.add_css_class("floating-icon");
    h.float
        .put(&img, x - f64::from(ICON) / 2.0, y - f64::from(ICON) / 2.0);
    *h.floating.borrow_mut() = Some(img.upcast());
}

fn move_float(h: &Home, x: f64, y: f64) {
    if let Some(w) = h.floating.borrow().as_ref() {
        h.float
            .move_(w, x - f64::from(ICON) / 2.0, y - f64::from(ICON) / 2.0);
    }
    let over_remove = bounds_contain(&h.remove, &h.root, x, y);
    if over_remove {
        h.remove.add_css_class("hot");
    } else {
        h.remove.remove_css_class("hot");
    }
    set_hover(h, if over_remove { None } else { cell_at(h, x, y) });
}

fn stop_float(h: &Home) {
    if let Some(w) = h.floating.borrow_mut().take() {
        h.float.remove(&w);
    }
    set_hover(h, None);
    h.remove.set_visible(false);
    h.remove.remove_css_class("hot");
    h.dragging.borrow_mut().take();
}

/// Drop whatever is being dragged at (x, y).
fn drop_at(h: &Home, x: f64, y: f64) {
    let dragged = h.dragging.borrow().clone();
    let over_remove = bounds_contain(&h.remove, &h.root, x, y);
    let target = cell_at(h, x, y);
    stop_float(h);
    let Some(dragged) = dragged else { return };

    let mut v = layout(&h.shell.cfg());
    let n = h.cells.borrow().len().max(v.len());
    v.resize(n, String::new());
    match dragged {
        Dragged::Slot(from) => {
            if over_remove {
                v[from].clear();
            } else if let Some(to) = target {
                v.swap(from, to); // free spot: a move; icon there: a swap
            }
        }
        Dragged::New(id) => {
            let Some(to) = target else {
                render(h);
                return;
            };
            if let Some(old) = v.iter().position(|x| *x == id) {
                v.swap(old, to); // already on the home screen: move it
            } else if v[to].is_empty() {
                v[to] = id;
            } else {
                // spot taken: use the nearest free spot after it (or append)
                match (to..v.len()).chain(0..to).find(|&i| v[i].is_empty()) {
                    Some(i) => v[i] = id,
                    None => v.push(id),
                }
            }
        }
    }
    save(&h.shell, v);
    render(h);
}

// ------------------------------------------------------------ from the drawer
// The drawer and the home screen cover exactly the same area, so drawer
// coordinates can be used here directly.

pub fn external_start(h: &Home, id: &str, x: f64, y: f64) {
    stop_float(h);
    *h.dragging.borrow_mut() = Some(Dragged::New(id.to_string()));
    start_float(h, id, x, y);
    move_float(h, x, y);
}

pub fn external_move(h: &Home, x: f64, y: f64) {
    move_float(h, x, y);
}

pub fn external_drop(h: &Home, x: f64, y: f64) {
    drop_at(h, x, y);
}

pub fn cancel(h: &Home) -> bool {
    let was = h.dragging.borrow().is_some();
    stop_float(h);
    if was {
        render(h);
    }
    was
}

pub fn build(shell: &Shell) -> Home {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("home");
    content.set_vexpand(true);

    // shown at the top, over the grid, only while an icon is dragged
    let remove = gtk::Label::new(Some("✕  Remove"));
    remove.add_css_class("remove-zone");
    remove.set_visible(false);
    remove.set_valign(gtk::Align::Start);

    let grid = gtk::Grid::new();
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);
    grid.set_vexpand(true);
    grid.set_hexpand(true);
    grid.set_margin_top(16);
    grid.set_margin_start(8);
    grid.set_margin_end(8);
    grid.set_margin_bottom(8);
    content.append(&grid);

    let root = gtk::Overlay::new();
    root.set_child(Some(&content));
    root.add_overlay(&remove);

    let hint = gtk::Label::new(Some(
        "Swipe up for your apps.\nLong-press one and drag it here.",
    ));
    hint.add_css_class("hint");
    hint.set_justify(gtk::Justification::Center);
    hint.set_valign(gtk::Align::Center);
    hint.set_can_target(false);
    root.add_overlay(&hint);

    let float = gtk::Fixed::new();
    float.set_can_target(false);
    root.add_overlay(&float);

    let h: Home = Rc::new(HomeInner {
        shell: shell.clone(),
        root: root.clone(),
        remove,
        grid,
        float,
        hint,
        cells: RefCell::new(Vec::new()),
        dragging: RefCell::new(None),
        floating: RefCell::new(None),
        hover: Cell::new(None),
    });

    // long-press an icon -> drag it around
    let (h1, h2, h3, h4) = (h.clone(), h.clone(), h.clone(), h.clone());
    gestures::attach_long_drag(
        &root,
        LongDrag {
            start: Box::new(move |x: f64, y: f64| {
                let Some(i) = cell_at(&h1, x, y) else {
                    return false;
                };
                let mut v = layout(&h1.shell.cfg());
                v.resize(h1.cells.borrow().len().max(v.len()), String::new());
                if v[i].is_empty() {
                    return false;
                }
                if let Some(c) = h1.cells.borrow().get(i) {
                    c.add_css_class("lifted");
                }
                *h1.dragging.borrow_mut() = Some(Dragged::Slot(i));
                h1.remove.set_visible(true);
                start_float(&h1, &v[i], x, y);
                true
            }),
            motion: Box::new(move |x: f64, y: f64| move_float(&h2, x, y)),
            end: Box::new(move |x: f64, y: f64| drop_at(&h3, x, y)),
            cancel: Box::new(move || {
                cancel(&h4);
            }),
        },
    );

    // swipe up = apps, swipe down = quick settings
    let s = shell.clone();
    let hd = h.clone();
    gestures::attach(
        &root,
        Spec {
            pick: Box::new(move |_dx: f64, dy: f64| {
                if hd.dragging.borrow().is_some() {
                    None
                } else if dy < 0.0 {
                    s.target(Overlay::Drawer, -1.0, 0.0)
                } else {
                    s.target(Overlay::Quick, 1.0, 0.0)
                }
            }),
            tap: None,
        },
    );

    crate::bars::dismiss_on_touch(shell, &root);
    render(&h);
    h
}

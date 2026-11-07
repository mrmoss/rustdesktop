//! The RustTouch keyboard: one on-screen keyboard for everything (apps, the
//! app drawer, the pull-down, settings) except the lock screen.
//!
//! * Drawn by RustTouch on the Overlay layer, so it's always above apps and
//!   our own menus, and it reserves its height so windows move up.
//! * Types through the Wayland virtual-keyboard protocol (real key presses,
//!   works in every app, terminals and X11 apps too); characters that aren't
//!   on a US keyboard (emoji) go through the input-method protocol.
//! * Layout follows wvkbd's: "full" and "special" layers in portrait, a wider
//!   one-screen layout in landscape. Settings > Keyboard sets colors, height,
//!   key rounding and font size.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

use crate::layer::{self, Anchors, KeyboardMode, Layer, Zone};
use crate::shell::{self, Shell};

// evdev key codes
pub const KEY_ESC: u32 = 1;
pub const KEY_BACKSPACE: u32 = 14;
pub const KEY_TAB: u32 = 15;
pub const KEY_ENTER: u32 = 28;
const KEY_HOME: u32 = 102;
const KEY_UP: u32 = 103;
const KEY_PAGEUP: u32 = 104;
const KEY_LEFT: u32 = 105;
const KEY_RIGHT: u32 = 106;
const KEY_END: u32 = 107;
const KEY_DOWN: u32 = 108;
const KEY_PAGEDOWN: u32 = 109;
const KEY_INSERT: u32 = 110;
const KEY_DELETE: u32 = 111;

// modifier masks (xkb, standard keymap)
pub const MOD_SHIFT: u32 = 1;
const MOD_CTRL: u32 = 4;
const MOD_ALT: u32 = 8;
const MOD_SUPER: u32 = 64;

type Row = &'static [(&'static str, f32)];

/// wvkbd "full" (portrait).
const FULL: &[Row] = &[
    &[
        ("Esc", 1.0),
        ("Ctr", 1.0),
        ("↑", 1.0),
        ("↓", 1.0),
        ("←", 1.0),
        ("→", 1.0),
        ("'", 1.0),
        ("-", 1.0),
        ("/", 1.0),
        ("Tab", 1.0),
    ],
    &[
        ("1", 1.0),
        ("2", 1.0),
        ("3", 1.0),
        ("4", 1.0),
        ("5", 1.0),
        ("6", 1.0),
        ("7", 1.0),
        ("8", 1.0),
        ("9", 1.0),
        ("0", 1.0),
    ],
    &[
        ("q", 1.0),
        ("w", 1.0),
        ("e", 1.0),
        ("r", 1.0),
        ("t", 1.0),
        ("y", 1.0),
        ("u", 1.0),
        ("i", 1.0),
        ("o", 1.0),
        ("p", 1.0),
    ],
    &[
        ("", 0.5),
        ("a", 1.0),
        ("s", 1.0),
        ("d", 1.0),
        ("f", 1.0),
        ("g", 1.0),
        ("h", 1.0),
        ("j", 1.0),
        ("k", 1.0),
        ("l", 1.0),
        ("", 0.5),
    ],
    &[
        ("⇧", 1.5),
        ("z", 1.0),
        ("x", 1.0),
        ("c", 1.0),
        ("v", 1.0),
        ("b", 1.0),
        ("n", 1.0),
        ("m", 1.0),
        ("⌫", 1.5),
    ],
    &[
        ("⌨→", 1.5),
        ("Cmp", 1.0),
        (",", 1.0),
        ("␣", 4.0),
        (".", 1.0),
        ("Enter", 1.5),
    ],
];
/// wvkbd "special".
const SPECIAL: &[Row] = &[
    &[
        ("Esc", 1.0),
        ("Alt", 1.0),
        ("↑", 1.0),
        ("↓", 1.0),
        ("←", 1.0),
        ("→", 1.0),
        ("⇈", 1.0),
        ("⇊", 1.0),
        ("⇤", 1.0),
        ("⇥", 1.0),
    ],
    &[
        ("1", 1.0),
        ("2", 1.0),
        ("3", 1.0),
        ("4", 1.0),
        ("5", 1.0),
        ("6", 1.0),
        ("7", 1.0),
        ("8", 1.0),
        ("9", 1.0),
        ("0", 1.0),
    ],
    &[
        ("CpL", 1.0),
        ("Sup", 1.0),
        ("`", 1.0),
        ("'", 1.0),
        ("-", 1.0),
        ("=", 1.0),
        ("[", 1.0),
        ("]", 1.0),
        ("\\", 1.0),
        ("Del", 1.0),
    ],
    &[
        ("⇧", 2.0),
        (";", 1.0),
        ("/", 1.0),
        ("<", 1.0),
        (">", 1.0),
        ("¡", 1.0),
        ("🙂", 1.0),
        ("AGr", 1.0),
        ("⌫", 1.0),
    ],
    &[
        ("Abc", 1.0),
        ("⌨→", 1.0),
        (",", 1.0),
        ("␣", 4.0),
        (".", 1.0),
        ("Enter", 2.0),
    ],
];
/// Landscape: every key on one wide page, like a laptop keyboard.
const WIDE: &[Row] = &[
    &[
        ("Esc", 1.0),
        ("1", 1.0),
        ("2", 1.0),
        ("3", 1.0),
        ("4", 1.0),
        ("5", 1.0),
        ("6", 1.0),
        ("7", 1.0),
        ("8", 1.0),
        ("9", 1.0),
        ("0", 1.0),
        ("-", 1.0),
        ("=", 1.0),
        ("⌫", 1.5),
    ],
    &[
        ("Tab", 1.5),
        ("q", 1.0),
        ("w", 1.0),
        ("e", 1.0),
        ("r", 1.0),
        ("t", 1.0),
        ("y", 1.0),
        ("u", 1.0),
        ("i", 1.0),
        ("o", 1.0),
        ("p", 1.0),
        ("[", 1.0),
        ("]", 1.0),
        ("\\", 1.0),
    ],
    &[
        ("Ctr", 1.75),
        ("a", 1.0),
        ("s", 1.0),
        ("d", 1.0),
        ("f", 1.0),
        ("g", 1.0),
        ("h", 1.0),
        ("j", 1.0),
        ("k", 1.0),
        ("l", 1.0),
        (";", 1.0),
        ("'", 1.0),
        ("Enter", 1.75),
    ],
    &[
        ("⇧", 2.25),
        ("z", 1.0),
        ("x", 1.0),
        ("c", 1.0),
        ("v", 1.0),
        ("b", 1.0),
        ("n", 1.0),
        ("m", 1.0),
        (",", 1.0),
        (".", 1.0),
        ("/", 1.0),
        ("↑", 1.0),
        ("Del", 1.25),
    ],
    &[
        ("⌨→", 1.5),
        ("Alt", 1.0),
        ("Sup", 1.0),
        ("`", 1.0),
        ("␣", 6.0),
        ("🙂", 1.0),
        ("←", 1.0),
        ("↓", 1.0),
        ("→", 1.5),
    ],
];
/// The 🙂 page.
const EMOJI: &[Row] = &[
    &[
        ("😀", 1.0),
        ("😂", 1.0),
        ("😊", 1.0),
        ("😍", 1.0),
        ("😎", 1.0),
        ("🤔", 1.0),
        ("😢", 1.0),
        ("😡", 1.0),
        ("😴", 1.0),
        ("🙃", 1.0),
    ],
    &[
        ("👍", 1.0),
        ("👎", 1.0),
        ("👏", 1.0),
        ("🙏", 1.0),
        ("💪", 1.0),
        ("👋", 1.0),
        ("🤝", 1.0),
        ("✌️", 1.0),
        ("👌", 1.0),
        ("🤞", 1.0),
    ],
    &[
        ("❤️", 1.0),
        ("🔥", 1.0),
        ("⭐", 1.0),
        ("🎉", 1.0),
        ("✅", 1.0),
        ("❌", 1.0),
        ("⚡", 1.0),
        ("☕", 1.0),
        ("🍕", 1.0),
        ("🚀", 1.0),
    ],
    &[("Abc", 2.0), ("␣", 6.0), ("⌫", 2.0)],
];

/// (evdev code, needs shift) for a character on a US keyboard.
fn us_key(c: char) -> Option<(u32, bool)> {
    const LOWER: &str = "qwertyuiop";
    const MID: &str = "asdfghjkl";
    const LOW: &str = "zxcvbnm";
    let lc = c.to_ascii_lowercase();
    let shift = c.is_ascii_uppercase();
    if let Some(i) = LOWER.find(lc) {
        return Some((16 + i as u32, shift));
    }
    if let Some(i) = MID.find(lc) {
        return Some((30 + i as u32, shift));
    }
    if let Some(i) = LOW.find(lc) {
        return Some((44 + i as u32, shift));
    }
    let plain = "1234567890-=[];'`\\,./ ";
    let codes = [
        2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 26, 27, 39, 40, 41, 43, 51, 52, 53, 57,
    ];
    if let Some(i) = plain.chars().position(|p| p == c) {
        return Some((codes[i], false));
    }
    let shifted = "!@#$%^&*()_+{}:\"~|<>?";
    let scodes = [
        2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 26, 27, 39, 40, 41, 43, 51, 52, 53,
    ];
    shifted
        .chars()
        .position(|p| p == c)
        .map(|i| (scodes[i], true))
}

/// What shift shows on a key.
fn shifted(k: &str) -> String {
    let plain = "1234567890-=[];'`\\,./";
    let up = "!@#$%^&*()_+{}:\"~|<>?";
    if k.chars().count() == 1 {
        let c = k.chars().next().unwrap_or(' ');
        if let Some(i) = plain.chars().position(|p| p == c) {
            return up.chars().nth(i).map(String::from).unwrap_or_default();
        }
    }
    k.to_uppercase()
}

fn named(k: &str) -> Option<u32> {
    Some(match k {
        "Esc" => KEY_ESC,
        "Tab" => KEY_TAB,
        "⌫" => KEY_BACKSPACE,
        "Enter" => KEY_ENTER,
        "↑" => KEY_UP,
        "↓" => KEY_DOWN,
        "←" => KEY_LEFT,
        "→" => KEY_RIGHT,
        "⇤" => KEY_HOME,
        "⇥" => KEY_END,
        "⇈" => KEY_PAGEUP,
        "⇊" => KEY_PAGEDOWN,
        "Del" => KEY_DELETE,
        "Ins" => KEY_INSERT,
        _ => return None,
    })
}

fn is_special(k: &str) -> bool {
    named(k).is_some()
        || matches!(
            k,
            "⇧" | "⌨→" | "Abc" | "Cmp" | "Ctr" | "Alt" | "Sup" | "AGr" | "CpL"
        )
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Main,
    Special,
    Emoji,
}

#[derive(Default)]
struct State {
    win: Option<gtk::ApplicationWindow>,
    landscape: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Keyboard height: Settings > Keyboard, or a comfortable default.
fn height(shell: &Shell, landscape: bool) -> i32 {
    let cfg = shell.cfg();
    let (key, default) = if landscape {
        ("keyboard_height_landscape", 260)
    } else {
        ("keyboard_height", 300)
    };
    cfg.get(key)
        .parse::<i32>()
        .unwrap_or(default)
        .clamp(120, 700)
}

/// Show or hide the keyboard.
pub fn set_visible(shell: &Shell, on: bool) {
    let (_, _, w, h) = shell::screen_size();
    let landscape = w > h;
    if !on {
        STATE.with(|s| {
            if let Some(win) = s.borrow().win.as_ref() {
                win.set_visible(false);
            }
        });
        shell.set_nav_zone(0);
        return;
    }
    let stale = STATE.with(|s| {
        let s = s.borrow();
        s.win.is_none() || s.landscape != landscape
    });
    if stale {
        let old = STATE.with(|s| s.borrow_mut().win.take());
        if let Some(o) = old {
            o.destroy();
        }
        let win = build(shell, landscape);
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.win = Some(win);
            s.landscape = landscape;
        });
    }
    let ht = height(shell, landscape);
    crate::osk::apply_style(&shell.cfg(), ht);
    STATE.with(|s| {
        if let Some(win) = s.borrow().win.as_ref() {
            win.set_default_size(-1, ht);
            if let Some(c) = win.child() {
                c.set_size_request(-1, ht);
            }
            win.present();
        }
    });
    place(shell);
}

/// Sit right above the button bar (the bar then reserves room for both, so
/// apps move up), or at the bottom edge when the bar is hidden.
pub fn place(shell: &Shell) {
    let Some(win) = STATE.with(|s| s.borrow().win.clone()) else {
        return;
    };
    if !win.is_visible() {
        return;
    }
    let landscape = STATE.with(|s| s.borrow().landscape);
    let ht = height(shell, landscape);
    let nav = shell.nav_shown();
    layer::set_margins(&win, 0, 0, nav, 0);
    if nav > 0 {
        layer::set_zone(&win, Zone::Fixed(-1));
        shell.set_nav_zone(ht);
    } else {
        layer::set_zone(&win, Zone::Fixed(ht));
        shell.set_nav_zone(0);
    }
}

/// Forget the window (settings or the screen changed): rebuilt on next show.
pub fn reset() {
    let old = STATE.with(|s| s.borrow_mut().win.take());
    if let Some(o) = old {
        o.destroy();
    }
}

fn build(shell: &Shell, landscape: bool) -> gtk::ApplicationWindow {
    // landscape: every key on one page, or the portrait pages
    let wide = shell.cfg().get("keyboard_landscape_layout") != "compact";
    let app = gtk::gio::Application::default()
        .and_downcast::<gtk::Application>()
        .expect("gtk app");
    let win = shell::shell_window(&app, "rusttouch keyboard");
    let a = Anchors {
        top: false,
        left: true,
        right: true,
        bottom: true,
    };
    // Overlay: above apps and our menus. Zone: apps move up out of its way.
    layer::setup(&win, Layer::Overlay, a, Zone::Auto, KeyboardMode::None);
    layer::set_monitor(&win, shell::shell_monitor().as_ref());

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("rt-osk");
    let page = Rc::new(Cell::new(Page::Main));
    let shift = Rc::new(Cell::new(false));
    let caps = Rc::new(Cell::new(false));
    let mods = Rc::new(Cell::new(0u32));
    let redraw: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let (r, s) = (root.clone(), shell.clone());
    let (pg, sh, cp, md, rd) = (
        page.clone(),
        shift.clone(),
        caps.clone(),
        mods.clone(),
        redraw.clone(),
    );
    let draw: Rc<dyn Fn()> = Rc::new(move || {
        while let Some(c) = r.first_child() {
            r.remove(&c);
        }
        let rows = match (pg.get(), landscape && wide) {
            (Page::Emoji, _) => EMOJI,
            (Page::Special, _) => SPECIAL,
            (Page::Main, false) => FULL,
            (Page::Main, true) => WIDE,
        };
        // quarter-key columns so widths like 1.75 fit
        let grid = gtk::Grid::new();
        grid.set_row_homogeneous(true);
        grid.set_column_homogeneous(true);
        grid.set_vexpand(true);
        for (y, row) in rows.iter().enumerate() {
            let mut x = 0;
            for &(k, width) in row.iter() {
                let span = (width * 4.0).round() as i32;
                if k.is_empty() {
                    x += span;
                    continue;
                }
                let up = sh.get() || cp.get();
                let label = match k {
                    "␣" => " ".to_string(),
                    _ if up && !is_special(k) => shifted(k),
                    _ => k.to_string(),
                };
                // a plain box, not a GtkButton: the button's own press
                // handling swallowed the "finger lifted" event, so held-key
                // repeat never stopped
                let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
                b.add_css_class("key");
                let l = gtk::Label::new(Some(&label));
                l.set_vexpand(true);
                l.set_hexpand(true);
                b.append(&l);
                if is_special(k) {
                    b.add_css_class("sp");
                }
                let lit = match k {
                    "⇧" => sh.get() || cp.get(),
                    "CpL" => cp.get(),
                    "Ctr" => md.get() & MOD_CTRL != 0,
                    "Alt" => md.get() & MOD_ALT != 0,
                    "Sup" => md.get() & MOD_SUPER != 0,
                    _ => false,
                };
                if lit {
                    b.add_css_class("on");
                }
                attach_key(&b, k, &s, &pg, &sh, &cp, &md, &rd);
                grid.attach(&b, x, y as i32, span, 1);
                x += span;
            }
        }
        r.append(&grid);
    });
    *redraw.borrow_mut() = Some(draw.clone());
    draw();
    win.set_child(Some(&root));
    win
}

#[allow(clippy::too_many_arguments)]
fn attach_key(
    b: &gtk::Box,
    k: &'static str,
    shell: &Shell,
    page: &Rc<Cell<Page>>,
    shift: &Rc<Cell<bool>>,
    caps: &Rc<Cell<bool>>,
    mods: &Rc<Cell<u32>>,
    redraw: &Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) {
    let g = gtk::GestureClick::new();
    let repeat_gen = Rc::new(Cell::new(0u32));
    let (s, pg, sh, cp, md, rd, rg) = (
        shell.clone(),
        page.clone(),
        shift.clone(),
        caps.clone(),
        mods.clone(),
        redraw.clone(),
        repeat_gen.clone(),
    );
    let key_widget = b.clone();
    g.connect_pressed(move |g, _, _, _| {
        // this key owns the touch until the finger lifts
        let _ = g.set_state(gtk::EventSequenceState::Claimed);
        key_widget.add_css_class("down");
        let again = || {
            if let Some(f) = rd.borrow().clone() {
                // redraw next turn (this button goes away)
                let _ = glib::idle_add_local_once(move || f());
            }
        };
        match k {
            "⇧" => {
                sh.set(!sh.get());
                again();
                return;
            }
            "CpL" => {
                cp.set(!cp.get());
                again();
                return;
            }
            "Ctr" | "Alt" | "Sup" => {
                let m = match k {
                    "Ctr" => MOD_CTRL,
                    "Alt" => MOD_ALT,
                    _ => MOD_SUPER,
                };
                md.set(md.get() ^ m);
                again();
                return;
            }
            "⌨→" => {
                pg.set(if pg.get() == Page::Main {
                    Page::Special
                } else {
                    Page::Main
                });
                again();
                return;
            }
            "Abc" => {
                pg.set(Page::Main);
                again();
                return;
            }
            "🙂" if pg.get() != Page::Emoji => {
                pg.set(Page::Emoji);
                again();
                return;
            }
            "Cmp" | "AGr" => return,
            _ => {}
        }
        let up = sh.get() || cp.get();
        let m = md.get();
        send(&s, k, up, m);
        // one-shot modifiers end after a key
        let mut changed = false;
        if sh.get() {
            sh.set(false);
            changed = true;
        }
        if m != 0 {
            md.set(0);
            changed = true;
        }
        if changed {
            again();
        }
        // hold to repeat (backspace, arrows, space, letters...)
        let my = rg.get().wrapping_add(1);
        rg.set(my);
        let (s2, rg2, kw) = (s.clone(), rg.clone(), key_widget.clone());
        // still held = same press, key still on screen and still pressed
        let held = move || rg2.get() == my && kw.is_mapped() && kw.has_css_class("down");
        let _ = glib::timeout_add_local_once(Duration::from_millis(450), move || {
            if !held() {
                return;
            }
            let mut left = 200; // never more than ~12 s of repeats
            let _ = glib::timeout_add_local(Duration::from_millis(60), move || {
                left -= 1;
                if !held() || left == 0 {
                    return glib::ControlFlow::Break;
                }
                send(&s2, k, up, 0);
                glib::ControlFlow::Continue
            });
        });
    });
    // any way the touch ends stops the repeat
    let stop = {
        let (rg, kw) = (repeat_gen.clone(), b.clone());
        Rc::new(move || {
            rg.set(rg.get().wrapping_add(1));
            kw.remove_css_class("down");
        })
    };
    let f = stop.clone();
    g.connect_released(move |_, _, _, _| f());
    let f = stop.clone();
    g.connect_stopped(move |_| f());
    let f = stop.clone();
    g.connect_cancel(move |_, _| f());
    let f = stop;
    g.connect_end(move |_, _| f());
    b.add_controller(g);
}

/// Type one key.
fn send(shell: &Shell, k: &str, up: bool, mods: u32) {
    let Some(wl) = shell.wl_client() else { return };
    if let Some(code) = named(k) {
        wl.vk_key(code, mods);
        return;
    }
    let text = match k {
        "␣" => " ".to_string(),
        _ if up => shifted(k),
        _ => k.to_string(),
    };
    let mut chars = text.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if let Some((code, shift)) = us_key(c) {
            wl.vk_key(code, mods | if shift { MOD_SHIFT } else { 0 });
            return;
        }
    }
    // emoji, accents: as text
    wl.commit_text(&text);
}

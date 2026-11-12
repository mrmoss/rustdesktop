#![cfg_attr(not(feature = "layer"), allow(dead_code, unused_imports))]
//! The on-screen keyboard for apps: shows at the bottom of the screen when a
//! text box in an app (a web page's sign-in) gets the focus. While it shows,
//! the remote's arrows move around its keys and OK types one; Back puts it
//! away. Typing on a real keyboard puts it away too (the keys go on to the
//! app). See im.rs for how it reaches the app.

use crate::im::{self, Client};
use crate::Tv;
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

// evdev key codes
const KEY_ESC: u32 = 1;
const KEY_BACKSPACE: u32 = 14;
const KEY_ENTER: u32 = 28;
const KEY_KPENTER: u32 = 96;
const KEY_UP: u32 = 103;
const KEY_LEFT: u32 = 105;
const KEY_RIGHT: u32 = 106;
const KEY_DOWN: u32 = 108;
const KEY_BACK: u32 = 158;
const KEY_OK: u32 = 352;
const KEY_SELECT: u32 = 353;
/// Shift, Ctrl, Alt, AltGr, the Windows keys: passed on, nothing else
const MODIFIERS: &[u32] = &[29, 42, 54, 56, 97, 100, 125, 126];

#[derive(Clone, Copy, PartialEq)]
enum Key {
    Char(char),
    Shift,
    Text(&'static str),
    Backspace,
    Enter,
    Hide,
}

/// The rows: (key, how many columns wide).
fn layout() -> Vec<Vec<(Key, i32)>> {
    let row = |s: &str| s.chars().map(|c| (Key::Char(c), 1)).collect::<Vec<_>>();
    vec![
        row("1234567890"),
        row("qwertyuiop"),
        row("asdfghjkl:"),
        row("zxcvbnm./-"),
        vec![
            (Key::Shift, 1),
            (Key::Char('@'), 1),
            (Key::Text(" "), 3),
            (Key::Text(".com"), 1),
            (Key::Backspace, 1),
            (Key::Enter, 2),
            (Key::Hide, 1),
        ],
    ]
}

struct Kbd {
    win: gtk::Window,
    preview: gtk::Label,
    /// per row: (button, key, first column, width)
    keys: Vec<Vec<(gtk::Button, Key, i32, i32)>>,
    /// selected (row, index in row)
    sel: Cell<(usize, usize)>,
    /// Shift: 0 off, 1 next letter, 2 caps lock
    shift: Cell<u8>,
    client: Client,
    /// a text box has the focus
    text_on: Cell<bool>,
    shown: Cell<bool>,
    /// a real keyboard's key was passed on: put the keyboard away when it's
    /// let go
    hide_after: Cell<Option<u32>>,
}

thread_local! {
    static KBD: RefCell<Option<Rc<Kbd>>> = const { RefCell::new(None) };
}

fn label(k: Key, up: bool, shift: u8) -> String {
    match k {
        Key::Char(c) if up => c.to_uppercase().to_string(),
        Key::Char(c) => c.to_string(),
        Key::Shift => if shift == 2 { "⇪" } else { "⇧" }.into(),
        Key::Text(" ") => "Space".into(),
        Key::Text(t) => t.into(),
        Key::Backspace => "⌫".into(),
        Key::Enter => "Enter".into(),
        Key::Hide => "Hide".into(),
    }
}

impl Kbd {
    fn redraw(&self) {
        let st = self.shift.get();
        let (sr, si) = self.sel.get();
        for (r, row) in self.keys.iter().enumerate() {
            for (i, (b, k, _, _)) in row.iter().enumerate() {
                b.set_label(&label(*k, st > 0, st));
                if (r, i) == (sr, si) {
                    b.add_css_class("sel");
                } else {
                    b.remove_css_class("sel");
                }
                if *k == Key::Shift && st > 0 {
                    b.add_css_class("on");
                } else if *k == Key::Shift {
                    b.remove_css_class("on");
                }
            }
        }
    }

    fn show(&self, on: bool) {
        if self.shown.replace(on) == on {
            return;
        }
        self.win.set_visible(on);
        self.client.hold_keyboard(on);
        self.hide_after.set(None);
        if on {
            self.shift.set(0);
            self.sel.set((1, 0));
            self.redraw();
        }
    }

    /// Move the selection: across a row, or up/down to the key over the
    /// same column.
    fn step(&self, dr: i32, dc: i32) {
        let (r, i) = self.sel.get();
        if dc != 0 {
            let n = self.keys[r].len() as i32;
            let i = (i as i32 + dc).rem_euclid(n) as usize;
            self.sel.set((r, i));
        } else {
            let rows = self.keys.len() as i32;
            let nr = (r as i32 + dr).clamp(0, rows - 1) as usize;
            if nr != r {
                let (_, _, col, w) = &self.keys[r][i];
                let mid = col + w / 2;
                let ni = self.keys[nr]
                    .iter()
                    .position(|(_, _, c, w)| mid >= *c && mid < c + w)
                    .unwrap_or(self.keys[nr].len() - 1);
                self.sel.set((nr, ni));
            }
        }
        self.redraw();
    }

    fn press(&self, k: Key) {
        match k {
            Key::Char(c) => {
                let up = self.shift.get() > 0;
                let s = if up {
                    c.to_uppercase().to_string()
                } else {
                    c.to_string()
                };
                self.client.type_text(&s);
                if self.shift.get() == 1 && c.is_alphabetic() {
                    self.shift.set(0);
                }
            }
            Key::Shift => self.shift.set(match self.shift.get() {
                0 => 1,
                1 => 2,
                _ => 0,
            }),
            Key::Text(t) => self.client.type_text(t),
            Key::Backspace => self.client.backspace(),
            Key::Enter => self.client.tap(KEY_ENTER),
            Key::Hide => self.show(false),
        }
        self.redraw();
    }

    /// A key while the keyboard is held.
    fn key(&self, code: u32, pressed: bool, time: u32) {
        // a real keyboard's key, passed on: put this keyboard away once it's
        // let go
        if let Some(c) = self.hide_after.get() {
            self.client.forward(code, pressed, time);
            if !pressed && c == code {
                self.show(false);
            }
            return;
        }
        if MODIFIERS.contains(&code) {
            self.client.forward(code, pressed, time);
            return;
        }
        if !pressed {
            return;
        }
        match code {
            KEY_UP => self.step(-1, 0),
            KEY_DOWN => self.step(1, 0),
            KEY_LEFT => self.step(0, -1),
            KEY_RIGHT => self.step(0, 1),
            KEY_ENTER | KEY_KPENTER | KEY_OK | KEY_SELECT => {
                let (r, i) = self.sel.get();
                let k = self.keys[r][i].1;
                self.press(k);
            }
            KEY_ESC | KEY_BACK => self.show(false),
            KEY_BACKSPACE => self.client.backspace(),
            _ => {
                // typing on a real keyboard
                self.client.forward(code, true, time);
                self.hide_after.set(Some(code));
            }
        }
    }
}

/// Start it (with the `layer` feature, over every app). False when it
/// can't: no input-method support, or RustTV built without layer-shell.
pub fn start(tv: &Tv) -> bool {
    let _ = tv;
    #[cfg(feature = "layer")]
    {
        use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
        if !gtk4_layer_shell::is_supported() {
            return false;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let Some(client) = im::start(tx) else {
            eprintln!("rusttv: no input-method support here - no on-screen keyboard for apps");
            return false;
        };
        let win = gtk::Window::builder()
            .application(&tv.app)
            .title("RustTV keyboard")
            .build();
        win.add_css_class("app-kbd");
        win.init_layer_shell();
        win.set_namespace(Some("rusttv-keyboard"));
        win.set_layer(Layer::Overlay);
        win.set_anchor(Edge::Bottom, true);
        win.set_anchor(Edge::Left, true);
        win.set_anchor(Edge::Right, true);
        // (never takes the focus: the text box keeps it)
        win.set_keyboard_mode(KeyboardMode::None);
        win.set_exclusive_zone(-1);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.add_css_class("app-kbd-box");
        root.set_halign(gtk::Align::Center);
        let preview = gtk::Label::new(None);
        preview.add_css_class("typed");
        preview.set_xalign(0.0);
        preview.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        preview.set_hexpand(true);
        root.append(&preview);
        let grid = gtk::Grid::new();
        grid.set_row_spacing(8);
        grid.set_column_spacing(8);
        grid.set_halign(gtk::Align::Center);
        let mut keys = Vec::new();
        for (r, row) in layout().into_iter().enumerate() {
            let mut col = 0;
            let mut out = Vec::new();
            for (k, w) in row {
                let b = gtk::Button::with_label(&label(k, false, 0));
                b.add_css_class("key");
                b.set_focusable(false);
                if w > 1 || !matches!(k, Key::Char(_)) {
                    b.add_css_class("wide");
                }
                if k == Key::Enter {
                    b.add_css_class("done");
                }
                grid.attach(&b, col, r as i32, w, 1);
                out.push((b, k, col, w));
                col += w;
            }
            keys.push(out);
        }
        root.append(&grid);
        root.append(&crate::ui::hints(&[
            ("OK", "Type"),
            ("Back", "Hide"),
            ("A keyboard", "Types straight in"),
        ]));
        win.set_child(Some(&root));

        let k = Rc::new(Kbd {
            win,
            preview,
            keys,
            sel: Cell::new((1, 0)),
            shift: Cell::new(0),
            client,
            text_on: Cell::new(false),
            shown: Cell::new(false),
            hide_after: Cell::new(None),
        });
        // a mouse can click the keys too
        for row in &k.keys {
            for (b, key, _, _) in row {
                let (k2, key) = (Rc::downgrade(&k), *key);
                b.connect_clicked(move |_| {
                    if let Some(k) = k2.upgrade() {
                        k.press(key);
                    }
                });
            }
        }
        KBD.with(|x| *x.borrow_mut() = Some(k.clone()));
        // the input method's news
        let weak = Rc::downgrade(&k);
        glib::timeout_add_local(Duration::from_millis(30), move || {
            let Some(k) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    im::Event::Text(on) => {
                        k.text_on.set(on);
                        let want =
                            on && crate::with_tv_ret(|t| t.cfg().app_keyboard).unwrap_or(true);
                        k.show(want);
                    }
                    im::Event::Surrounding(text) => {
                        let tail: String = text
                            .chars()
                            .rev()
                            .take(80)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        k.preview.set_text(&format!("{tail}▏"));
                    }
                    im::Event::Key(code, pressed, time) => k.key(code, pressed, time),
                }
            }
            glib::ControlFlow::Continue
        });
        return true;
    }
    #[allow(unreachable_code)]
    false
}

/// Press a key in the app on screen (an app's remote button). False when
/// RustTV can't (no input method here).
pub fn tap(code: u32) -> bool {
    KBD.with(|x| {
        x.borrow().as_ref().is_some_and(|k| {
            k.client.tap(code);
            true
        })
    })
}

/// Show it again (a remote button: the text box still has the focus).
pub fn show_again() -> bool {
    KBD.with(|x| {
        x.borrow().as_ref().is_some_and(|k| {
            if k.text_on.get() {
                k.show(true);
                true
            } else {
                false
            }
        })
    })
}

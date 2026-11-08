//! A keyboard drawn by RustTouch inside the app drawer, looking like the
//! system on-screen keyboard (same colors, height and key shape from
//! Settings > Keyboard). On sway the keyboard program can't be drawn above
//! the drawer, so the drawer carries its own; it floats over the bottom of the
//! drawer without resizing it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

use crate::config::{self, Config};

/// wvkbd's default layout ("full"), key: (label, width in key units).
/// "" = empty space. Each row is 10 units wide, like wvkbd.
const FULL: &[&[(&str, f32)]] = &[
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
/// wvkbd's "special" layer (symbols, emoji).
const SPECIAL: &[&[(&str, f32)]] = &[
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

/// What shift gives on the US keyboard.
fn shifted(k: &str) -> String {
    let m = [
        ("1", "!"),
        ("2", "@"),
        ("3", "#"),
        ("4", "$"),
        ("5", "%"),
        ("6", "^"),
        ("7", "&"),
        ("8", "*"),
        ("9", "("),
        ("0", ")"),
        ("'", "\""),
        ("-", "_"),
        ("/", "?"),
        (",", "<"),
        (".", ">"),
        (";", ":"),
        ("`", "~"),
        ("=", "+"),
        ("[", "{"),
        ("]", "}"),
        ("\\", "|"),
    ];
    m.iter()
        .find(|e| e.0 == k)
        .map(|e| e.1.to_string())
        .unwrap_or_else(|| k.to_uppercase())
}

fn is_special(k: &str) -> bool {
    matches!(
        k,
        "Esc"
            | "Ctr"
            | "Alt"
            | "Tab"
            | "⇧"
            | "⌫"
            | "⌨→"
            | "Cmp"
            | "Enter"
            | "Abc"
            | "CpL"
            | "Sup"
            | "Del"
            | "AGr"
            | "↑"
            | "↓"
            | "←"
            | "→"
            | "⇈"
            | "⇊"
            | "⇤"
            | "⇥"
    )
}

thread_local! {
    static CSS: gtk::CssProvider = {
        let p = gtk::CssProvider::new();
        if let Some(d) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &d,
                &p,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
            );
        }
        p
    };
}

/// Same colors as the keyboard program gets (or wvkbd's own look).
pub fn apply_style(cfg: &Config, height: i32) {
    let (bg, key, special, press, text) = if cfg.get("keyboard_style") == "wvkbd" {
        (
            "202020".to_string(),
            "3a3a3a".to_string(),
            "2a2a2a".to_string(),
            "4f4f4f".to_string(),
            "ffffff".to_string(),
        )
    } else {
        config::keyboard_colors(cfg)
    };
    let radius: i32 = cfg.get("keyboard_rounding").parse().unwrap_or(0); // wvkbd: square
    let font: i32 = cfg.get("keyboard_font_size").parse().unwrap_or(18);
    let css = format!(
        ".rt-osk {{ background: #{bg}; min-height: {height}px; padding: 3px; }}\n\
         .rt-osk button, .rt-osk .key {{ background: #{key}; color: #{text}; border-radius: {radius}px; \
           font-size: {font}px; margin: 1px; padding: 0; border: none; box-shadow: none; min-height: 0; min-width: 0; }}\n\
         .rt-osk button.sp, .rt-osk .key.sp {{ background: #{special}; }}\n\
         .rt-osk button:active, .rt-osk button.on, .rt-osk .key.down, .rt-osk .key.on {{ background: #{press}; }}\n"
    );
    CSS.with(|p| p.load_from_data(&css));
}

/// Type `k` into whatever text field has focus in `w`'s window.
fn type_into(w: &gtk::Widget, k: &str) {
    let Some(target) = w.root().and_then(|r| r.focus()) else {
        return;
    };
    let Ok(ed) = target.clone().dynamic_cast::<gtk::Editable>() else {
        return;
    };
    match k {
        "⌫" => {
            if let Some((a, b)) = ed.selection_bounds() {
                ed.delete_text(a, b);
            } else {
                let p = ed.position();
                if p > 0 {
                    ed.delete_text(p - 1, p);
                }
            }
        }
        "Enter" => {
            let _ = target.activate();
        }
        "Del" => {
            let p = ed.position();
            ed.delete_text(p, p + 1);
        }
        "←" => ed.set_position((ed.position() - 1).max(0)),
        "→" => ed.set_position(ed.position() + 1),
        "⇤" | "⇈" => ed.set_position(0),
        "⇥" | "⇊" => ed.set_position(-1),
        "Tab" => {
            let mut pos = ed.position();
            ed.insert_text("\t", &mut pos);
            ed.set_position(pos);
        }
        _ => {
            if let Some((a, b)) = ed.selection_bounds() {
                ed.delete_text(a, b);
            }
            let mut pos = ed.position();
            ed.insert_text(k, &mut pos);
            ed.set_position(pos);
        }
    }
}

/// The keyboard widget (hidden until shown). `on_enter` runs after Enter or
/// Esc (the keyboard goes away).
pub fn keyboard(height: i32, on_enter: Rc<dyn Fn()>) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("rt-osk");
    root.set_valign(gtk::Align::End);
    root.set_size_request(-1, height);
    root.set_visible(false);
    let special = Rc::new(Cell::new(false));
    let shift = Rc::new(Cell::new(false));
    let build: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let r = root.clone();
    let (sp, sh, b2) = (special.clone(), shift.clone(), build.clone());
    let enter = on_enter.clone();
    let rebuild: Rc<dyn Fn()> = Rc::new(move || {
        while let Some(c) = r.first_child() {
            r.remove(&c);
        }
        // a grid of half-key columns: 10 keys = 20 columns, like wvkbd
        let grid = gtk::Grid::new();
        grid.set_row_homogeneous(true);
        grid.set_column_homogeneous(true);
        grid.set_vexpand(true);
        let rows = if sp.get() { SPECIAL } else { FULL };
        for (y, row) in rows.iter().enumerate() {
            let mut x = 0;
            for &(k, width) in row.iter() {
                let span = (width * 2.0).round() as i32;
                if k.is_empty() {
                    x += span;
                    continue;
                }
                let label: String = match k {
                    "␣" => " ".into(),
                    _ if sh.get() && !is_special(k) => shifted(k),
                    _ => k.into(),
                };
                let b = gtk::Button::with_label(&label);
                b.set_focusable(false); // keep the cursor in the text field
                if is_special(k) {
                    b.add_css_class("sp");
                }
                if k == "⇧" && sh.get() {
                    b.add_css_class("on");
                }
                let (sp, sh, b3, enter) = (sp.clone(), sh.clone(), b2.clone(), enter.clone());
                let key = k.to_string();
                b.connect_clicked(move |btn| {
                    let w: gtk::Widget = btn.clone().upcast();
                    match key.as_str() {
                        "⇧" => sh.set(!sh.get()),
                        "⌨→" | "Abc" => {
                            sp.set(!sp.get());
                            sh.set(false);
                        }
                        "Esc" => {
                            enter();
                            return;
                        }
                        "Enter" => {
                            type_into(&w, "Enter");
                            enter();
                            return;
                        }
                        "␣" => {
                            type_into(&w, " ");
                            return;
                        }
                        // modifiers and keys that mean nothing in a search box
                        "Ctr" | "Alt" | "Cmp" | "CpL" | "Sup" | "AGr" | "↑" | "↓" => return,
                        k if is_special(k) => {
                            type_into(&w, k);
                            return;
                        }
                        k => {
                            let t = if sh.get() { shifted(k) } else { k.to_string() };
                            type_into(&w, &t);
                            if !sh.get() {
                                return;
                            }
                            sh.set(false); // shift lasts one key
                        }
                    }
                    if let Some(f) = b3.borrow().clone() {
                        // redraw on the next turn (this button goes away)
                        let _ = gtk::glib::idle_add_local_once(move || f());
                    }
                });
                grid.attach(&b, x, y as i32, span, 1);
                x += span;
            }
        }
        r.append(&grid);
    });
    *build.borrow_mut() = Some(rebuild.clone());
    rebuild();
    root
}

//! Typing with a remote: a grid of keys you move through with the arrows
//! (OK types the key). A real keyboard types straight in too.

use crate::Tv;
use gtk::gdk;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const ROWS: &[&str] = &["1234567890", "qwertyuiop", "asdfghjkl:", "zxcvbnm./-"];

/// Ask for some text. `done` gets it (not called if you go Back).
pub fn ask(tv: &Tv, title: &str, start: &str, done: impl Fn(&Tv, String) + 'static) {
    ask_with(tv, title, start, false, done);
}

/// Ask for a password: shown as dots (the eye key shows it).
pub fn ask_secret(tv: &Tv, title: &str, done: impl Fn(&Tv, String) + 'static) {
    ask_with(tv, title, "", true, done);
}

/// One thing a search can find: its name, a line under it, what picking it
/// does.
pub struct Found {
    pub label: String,
    pub sub: String,
    pub pick: Rc<dyn Fn(&Tv)>,
}

/// What a search page adds to the keyboard: the results beside it,
/// told when the text changes; Done goes to the first result.
struct Side {
    widget: gtk::Widget,
    changed: Rc<dyn Fn(&str)>,
    done: Rc<dyn Fn()>,
}

/// Search `items`: the keyboard on the left, what matches on the right
/// (narrowing with every letter). The arrows go from the keys to the
/// results; OK on one picks it (the search closes first).
pub fn search(tv: &Tv, title: &str, items: Vec<Found>) {
    let list = crate::ui::list();
    list.set_halign(gtk::Align::Fill);
    let mut rows = Vec::new();
    for it in items {
        let (t, pick) = (tv.clone(), it.pick.clone());
        let (b, _) = crate::ui::row(&it.label, &it.sub, move || {
            t.back();
            pick(&t);
        });
        list.append(&b);
        let text = format!("{} {}", it.label, it.sub).to_lowercase();
        rows.push((b, text));
    }
    let none = gtk::Label::new(Some("Nothing matches"));
    none.add_css_class("note");
    none.set_visible(false);
    list.append(&none);
    let sc = gtk::ScrolledWindow::new();
    sc.set_hscrollbar_policy(gtk::PolicyType::Never);
    sc.set_hexpand(true);
    sc.set_vexpand(true);
    sc.set_child(Some(&list));
    let rows = Rc::new(rows);
    let changed: Rc<dyn Fn(&str)> = {
        let rows = rows.clone();
        Rc::new(move |q: &str| {
            let words: Vec<String> = q
                .to_lowercase()
                .split_whitespace()
                .map(String::from)
                .collect();
            let mut any = false;
            for (b, text) in rows.iter() {
                let on = words.iter().all(|w| text.contains(w.as_str()));
                b.set_visible(on);
                any |= on;
            }
            none.set_visible(!any);
        })
    };
    let done: Rc<dyn Fn()> = Rc::new(move || {
        if let Some((b, _)) = rows.iter().find(|(b, _)| b.is_visible()) {
            b.grab_focus();
        }
    });
    build(
        tv,
        title,
        "",
        false,
        Rc::new(|_: &Tv, _: String| {}),
        Some(Side {
            widget: sc.upcast(),
            changed,
            done,
        }),
    );
}

fn ask_with(tv: &Tv, title: &str, start: &str, secret: bool, done: impl Fn(&Tv, String) + 'static) {
    build(tv, title, start, secret, Rc::new(done), None);
}

fn build(
    tv: &Tv,
    title: &str,
    start: &str,
    secret: bool,
    done: Rc<dyn Fn(&Tv, String)>,
    side: Option<Side>,
) {
    let changed = side.as_ref().map(|s| s.changed.clone());
    let hidden = Rc::new(Cell::new(secret));
    let text = Rc::new(RefCell::new(start.to_string()));
    // Shift: 0 off, 1 the next letter only, 2 caps lock (pressed twice in a row)
    let shift = Rc::new(Cell::new(0u8));
    // (redraws the letters and the Shift key; set once the keys exist)
    let redraw: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let root = gtk::Box::new(gtk::Orientation::Vertical, 16);
    root.add_css_class("page");
    let t = gtk::Label::new(Some(title));
    t.add_css_class("page-title");
    t.set_halign(gtk::Align::Start);
    root.append(&t);
    let shown = gtk::Label::new(None);
    shown.add_css_class("typed");
    shown.set_halign(gtk::Align::Fill);
    shown.set_xalign(0.0);
    shown.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    root.append(&shown);

    let show = {
        let (shown, text, hidden) = (shown.clone(), text.clone(), hidden.clone());
        Rc::new(move || {
            let t = text.borrow();
            let t = if hidden.get() {
                "●".repeat(t.chars().count())
            } else {
                t.clone()
            };
            shown.set_text(&format!("{t}▏"));
            drop(t);
            // (a search: narrow the results)
            if let Some(c) = &changed {
                c(&text.borrow());
            }
        })
    };
    show();

    let grid = gtk::Grid::new();
    grid.add_css_class("keys");
    grid.set_row_spacing(8);
    grid.set_column_spacing(8);
    grid.set_halign(gtk::Align::Center);
    grid.set_vexpand(side.is_none());
    grid.set_valign(if side.is_some() {
        gtk::Align::Start
    } else {
        gtk::Align::Center
    });
    let letters: Rc<RefCell<Vec<(gtk::Button, char)>>> = Rc::new(RefCell::new(Vec::new()));
    let label_for = |c: char, up: bool| {
        if up {
            c.to_uppercase().to_string()
        } else {
            c.to_string()
        }
    };
    for (r, keys) in ROWS.iter().enumerate() {
        for (c, ch) in keys.chars().enumerate() {
            let b = gtk::Button::with_label(&label_for(ch, false));
            b.add_css_class("key");
            let (text, show, shift, redraw) =
                (text.clone(), show.clone(), shift.clone(), redraw.clone());
            b.connect_clicked(move |_| {
                let up = shift.get() > 0;
                text.borrow_mut().push_str(&label_for(ch, up));
                show();
                // (one capital, then back to small letters)
                if shift.get() == 1 && ch.is_alphabetic() {
                    shift.set(0);
                    let r = redraw.borrow().clone();
                    if let Some(r) = r {
                        r();
                    }
                }
            });
            grid.attach(&b, c as i32, r as i32, 1, 1);
            letters.borrow_mut().push((b, ch));
        }
    }
    // the bottom row: wider keys
    let bottom = ROWS.len() as i32;
    let special = |label: &str, col: i32, width: i32| {
        let b = gtk::Button::with_label(label);
        b.add_css_class("key");
        b.add_css_class("wide");
        grid.attach(&b, col, bottom, width, 1);
        b
    };
    let shift_b = special("⇧", 0, 1);
    let at_b = special("@", 1, 1);
    let space_b = special("Space", 2, 3);
    let com_b = special(".com", 5, 1);
    let del_b = special("⌫", 6, 2);
    let done_b = special("Done", 8, 2);
    done_b.add_css_class("done");
    if secret {
        // (show or hide the password)
        let eye = gtk::Button::with_label("👁");
        eye.add_css_class("key");
        eye.add_css_class("wide");
        grid.attach(&eye, 10, bottom, 1, 1);
        let (h, show) = (hidden.clone(), show.clone());
        eye.connect_clicked(move |_| {
            h.set(!h.get());
            show();
        });
    }
    {
        let (shift, letters) = (shift.clone(), letters.clone());
        let draw: Rc<dyn Fn()> = {
            let (shift, letters, b) = (shift.clone(), letters.clone(), shift_b.clone());
            Rc::new(move || {
                let st = shift.get();
                b.set_label(if st == 2 { "⇪" } else { "⇧" });
                if st > 0 {
                    b.add_css_class("on");
                } else {
                    b.remove_css_class("on");
                }
                for (k, ch) in letters.borrow().iter() {
                    k.set_label(&label_for(*ch, st > 0));
                }
            })
        };
        *redraw.borrow_mut() = Some(draw.clone());
        // once: the next letter; again right away: caps lock; again: off
        shift_b.connect_clicked(move |_| {
            shift.set(match shift.get() {
                0 => 1,
                1 => 2,
                _ => 0,
            });
            draw();
        });
    }
    let add = |b: &gtk::Button, s: &'static str| {
        let (text, show) = (text.clone(), show.clone());
        b.connect_clicked(move |_| {
            text.borrow_mut().push_str(s);
            show();
        });
    };
    add(&at_b, "@");
    add(&space_b, " ");
    add(&com_b, ".com");
    let delete = {
        let (text, show) = (text.clone(), show.clone());
        Rc::new(move || {
            text.borrow_mut().pop();
            show();
        })
    };
    {
        let d = delete.clone();
        del_b.connect_clicked(move |_| d());
    }
    match side.as_ref() {
        // a search: Done goes to the first result
        Some(sd) => {
            let d = sd.done.clone();
            done_b.connect_clicked(move |_| d());
        }
        None => {
            let (tv, text, done) = (tv.clone(), text.clone(), done.clone());
            done_b.connect_clicked(move |_| {
                let s = text.borrow().trim().to_string();
                // (a password: gone from the keyboard once it's been given)
                if secret {
                    crate::autologin::wipe(&mut text.borrow_mut());
                }
                tv.back();
                done(&tv, s);
            });
        }
    }
    match side {
        Some(sd) => {
            let h = gtk::Box::new(gtk::Orientation::Horizontal, 48);
            h.set_vexpand(true);
            h.append(&grid);
            h.append(&sd.widget);
            root.append(&h);
            root.append(&crate::ui::hints(&[
                ("OK", "Type, or pick a result"),
                ("Right", "To the results"),
                ("Back", "Back"),
            ]));
        }
        None => {
            root.append(&grid);
            root.append(&crate::ui::hints(&[("OK", "Type"), ("Back", "Cancel")]));
        }
    }

    // a real keyboard: letters type, Backspace deletes, Enter is Done
    // (the remote's OK is Enter too - it presses the key that has the focus)
    let keys: Rc<dyn Fn(gdk::Key, u32) -> bool> = {
        let (text, show, delete) = (text.clone(), show.clone(), delete.clone());
        Rc::new(move |key: gdk::Key, _code: u32| {
            if key == gdk::Key::BackSpace {
                delete();
                return true;
            }
            match key.to_unicode() {
                Some(c) if !c.is_control() => {
                    text.borrow_mut().push(c);
                    show();
                    true
                }
                _ => false,
            }
        })
    };
    // (a password left with Back: gone from memory too)
    if secret {
        let text = text.clone();
        root.connect_unmap(move |_| crate::autologin::wipe(&mut text.borrow_mut()));
    }
    tv.push(&root, Some(keys));
}

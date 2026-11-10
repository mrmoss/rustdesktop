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
    let text = Rc::new(RefCell::new(start.to_string()));
    let shift = Rc::new(Cell::new(false));

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
        let (shown, text) = (shown.clone(), text.clone());
        Rc::new(move || shown.set_text(&format!("{}▏", text.borrow())))
    };
    show();

    let grid = gtk::Grid::new();
    grid.add_css_class("keys");
    grid.set_row_spacing(8);
    grid.set_column_spacing(8);
    grid.set_halign(gtk::Align::Center);
    grid.set_vexpand(true);
    grid.set_valign(gtk::Align::Center);
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
            let (text, show, shift) = (text.clone(), show.clone(), shift.clone());
            b.connect_clicked(move |_| {
                let up = shift.get();
                text.borrow_mut().push_str(&label_for(ch, up));
                show();
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
    {
        let (shift, letters) = (shift.clone(), letters.clone());
        shift_b.connect_clicked(move |b| {
            let up = !shift.get();
            shift.set(up);
            if up {
                b.add_css_class("on");
            } else {
                b.remove_css_class("on");
            }
            for (k, ch) in letters.borrow().iter() {
                k.set_label(&label_for(*ch, up));
            }
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
    let done = Rc::new(done);
    {
        let (tv, text, done) = (tv.clone(), text.clone(), done.clone());
        done_b.connect_clicked(move |_| {
            let s = text.borrow().trim().to_string();
            tv.back();
            done(&tv, s);
        });
    }
    root.append(&grid);
    root.append(&crate::ui::hints(&[("OK", "Type"), ("Back", "Cancel")]));

    // a real keyboard: letters type, Backspace deletes, Enter is Done
    // (the remote's OK is Enter too - it presses the key that has the focus)
    let keys: Rc<dyn Fn(gdk::Key) -> bool> = {
        let (text, show, delete) = (text.clone(), show.clone(), delete.clone());
        Rc::new(move |key: gdk::Key| {
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
    tv.push(&root, Some(keys));
}

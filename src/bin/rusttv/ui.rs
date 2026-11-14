//! The building blocks of RustTV's pages: a title and a list of big rows you
//! move through with the arrows, a list to pick one thing from, a yes/no
//! question.

use crate::config;
use crate::Tv;
use gtk::prelude::*;
use std::rc::Rc;

/// Black or white text, whichever reads better on `bg` ("#rrggbb").
pub fn text_on(bg: &str) -> &'static str {
    let h =
        |i: usize| u8::from_str_radix(bg.get(i..i + 2).unwrap_or("80"), 16).unwrap_or(128) as f64;
    let lum = 0.299 * h(1) + 0.587 * h(3) + 0.114 * h(5);
    if lum > 160.0 {
        "#111111"
    } else {
        "#ffffff"
    }
}

/// `c` ("#rrggbb") mixed into RustTV's dark background: `amount` of the
/// colour (0..1).
pub fn shade(c: &str, amount: f64) -> String {
    let h =
        |i: usize| u8::from_str_radix(c.get(i..i + 2).unwrap_or("80"), 16).unwrap_or(128) as f64;
    let base = [0x16 as f64, 0x1a as f64, 0x24 as f64];
    let mix = |v: f64, b: f64| (v * amount + b * (1.0 - amount)).round().clamp(0.0, 255.0) as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        mix(h(1), base[0]),
        mix(h(3), base[1]),
        mix(h(5), base[2])
    )
}

pub fn browser_name(id: &str) -> &str {
    config::BROWSERS
        .iter()
        .find(|b| b.0 == id)
        .map(|b| b.1)
        .unwrap_or(id)
}

/// A full-screen page: its title at the top, `body` under it (scrolls).
pub fn page(title: &str, body: &impl IsA<gtk::Widget>) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("page");
    let t = gtk::Label::new(Some(title));
    t.add_css_class("page-title");
    t.set_halign(gtk::Align::Start);
    // (a long one goes on to more lines, rather than off the screen)
    t.set_wrap(true);
    t.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    t.set_xalign(0.0);
    root.append(&t);
    let sc = gtk::ScrolledWindow::new();
    sc.set_hscrollbar_policy(gtk::PolicyType::Never);
    sc.set_vexpand(true);
    sc.set_child(Some(body));
    root.append(&sc);
    root.append(&hints(&[("OK", "Select"), ("Back", "Back")]));
    root
}

/// The keys line at the bottom of a screen: [("OK", "Open"), ...] ->
/// "OK Open   ·   Back Back" (the keys in bold).
pub fn hints(items: &[(&str, &str)]) -> gtk::Label {
    let esc = |s: &str| gtk::glib::markup_escape_text(s).to_string();
    let text = items
        .iter()
        .map(|(k, what)| format!("<b>{}</b>  {}", esc(k), esc(what)))
        .collect::<Vec<_>>()
        .join("     ·     ");
    let l = gtk::Label::new(None);
    l.set_markup(&text);
    l.add_css_class("hint");
    l.set_halign(gtk::Align::Start);
    l
}

/// The list rows go in.
pub fn list() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
    b.add_css_class("list");
    b.set_halign(gtk::Align::Center);
    b
}

/// A row: `text` on the left, `value` (what it's set to) on the right.
/// Returns the row and the value's label (to change it later).
pub fn row(text: &str, value: &str, on_ok: impl Fn() + 'static) -> (gtk::Button, gtk::Label) {
    let b = gtk::Button::new();
    b.add_css_class("row");
    let h = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    let l = gtk::Label::new(Some(text));
    l.set_halign(gtk::Align::Start);
    l.set_hexpand(true);
    l.set_xalign(0.0);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let v = gtk::Label::new(Some(value));
    v.add_css_class("value");
    v.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    v.set_max_width_chars(36);
    h.append(&l);
    h.append(&v);
    b.set_child(Some(&h));
    b.connect_clicked(move |_| on_ok());
    (b, v)
}

/// A section heading inside a list.
pub fn heading(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("heading");
    l.set_halign(gtk::Align::Start);
    l
}

/// Pick one of `options` ((value, label)); `current` gets a tick. Picking
/// calls `pick` and goes back.
pub fn choose(
    tv: &Tv,
    title: &str,
    options: Vec<(String, String)>,
    current: &str,
    pick: impl Fn(&Tv, String) + 'static,
) {
    let body = list();
    let pick = Rc::new(pick);
    let mut first: Option<gtk::Button> = None;
    // a long list: a Search row at the top (the keyboard, and what matches)
    if options.len() > 12 {
        let items: Vec<(String, String)> = options.clone();
        let (t, p, title2) = (tv.clone(), pick.clone(), title.to_string());
        let (b, _) = row("Search", "", move || {
            let found = items
                .iter()
                .map(|(v, l)| {
                    let (p, v) = (p.clone(), v.clone());
                    crate::keyboard::Found {
                        label: l.clone(),
                        sub: String::new(),
                        pick: Rc::new(move |t: &Tv| {
                            // (off the list too)
                            t.back();
                            p(t, v.clone());
                        }),
                    }
                })
                .collect();
            crate::keyboard::search(&t, &format!("Search: {title2}"), found);
        });
        b.add_css_class("search-row");
        body.append(&b);
    }
    for (value, label) in options {
        let mark = if value == current { "✓" } else { "" };
        let (t, p, v) = (tv.clone(), pick.clone(), value.clone());
        let (b, _) = row(&label, mark, move || {
            t.back();
            p(&t, v.clone());
        });
        // (the current choice has the focus to start with, or else the first)
        if value == current || first.is_none() {
            first = Some(b.clone());
        }
        body.append(&b);
    }
    let pg = page(title, &body);
    tv.push(&pg, None);
    // start on the current choice
    if let Some(f) = first {
        let _ = gtk::glib::idle_add_local_once(move || {
            f.grab_focus();
        });
    }
}

/// Ask before doing something: `yes` runs if OK'd.
pub fn confirm(tv: &Tv, question: &str, yes_label: &str, yes: impl Fn(&Tv) + 'static) {
    let yes = Rc::new(yes);
    let y = yes.clone();
    choose(
        tv,
        question,
        vec![
            ("yes".into(), yes_label.into()),
            ("no".into(), "Cancel".into()),
        ],
        "no",
        move |t, v| {
            if v == "yes" {
                y(t);
            }
        },
    );
}

/// A list of actions (a card's options). Each closes the list first.
pub fn actions(tv: &Tv, title: &str, items: Vec<(String, Rc<dyn Fn(&Tv)>)>) {
    let body = list();
    for (label, f) in items {
        let t = tv.clone();
        let (b, _) = row(&label, "", move || {
            t.back();
            f(&t);
        });
        body.append(&b);
    }
    let pg = page(title, &body);
    tv.push(&pg, None);
}

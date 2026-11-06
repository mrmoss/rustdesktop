//! Lock screen: clock -> swipe up -> password with a built-in full keyboard
//! (letters, caps, numbers, symbols), so you can always type your real
//! password by touch. A physical keyboard works too. Checked with PAM.
//!
//! It's an Overlay-layer surface that covers everything and grabs the
//! keyboard. Note: if rusttouch crashes, the screen unlocks; the proper fix is
//! ext-session-lock-v1 (see README).

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::shell::Shell;
use crate::{clock, pam};

pub struct LockInner {
    window: Option<gtk::ApplicationWindow>,
    root: gtk::Box,
    stack: gtk::Stack,
    entry: gtk::PasswordEntry,
    status: gtk::Label,
    message: gtk::Label,
    busy: Cell<bool>,
    locked: Cell<bool>,
}

pub type Lock = Rc<LockInner>;

enum Key {
    Char(String),
    Back,
    Enter,
}

pub fn build(shell: &Shell, window: Option<gtk::ApplicationWindow>) -> Lock {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("lock");
    root.set_hexpand(true);
    root.set_vexpand(true);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::SlideUpDown);
    stack.set_transition_duration(220);
    stack.set_vexpand(true);

    // ---- page 1: clock
    let cp = gtk::Box::new(gtk::Orientation::Vertical, 0);
    cp.set_margin_top(80);
    let time = gtk::Label::new(None);
    time.add_css_class("lock-clock");
    clock::bind(&time, clock::Kind::Time);
    let date = gtk::Label::new(None);
    date.add_css_class("home-date");
    clock::bind(&date, clock::Kind::Date);
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    let hint = gtk::Label::new(Some("︿\nSwipe up to unlock"));
    hint.set_justify(gtk::Justification::Center);
    hint.add_css_class("hint");
    hint.set_margin_bottom(40);
    // your message (Settings → Appearance → Lock screen), e.g. contact info
    let message = gtk::Label::new(None);
    message.add_css_class("lock-message");
    message.set_wrap(true);
    message.set_justify(gtk::Justification::Center);
    message.set_margin_top(18);
    message.set_margin_start(24);
    message.set_margin_end(24);
    cp.append(&time);
    cp.append(&date);
    cp.append(&message);
    cp.append(&spacer);
    cp.append(&hint);
    stack.add_named(&cp, Some("clock"));

    // ---- page 2: password
    let pp = gtk::Box::new(gtk::Orientation::Vertical, 10);
    pp.set_margin_top(40);
    pp.set_margin_start(12);
    pp.set_margin_end(12);
    pp.set_margin_bottom(12);
    let small = gtk::Label::new(None);
    small.add_css_class("qs-time");
    clock::bind(&small, clock::Kind::Time);
    let title = gtk::Label::new(Some("Enter password"));
    title.add_css_class("qs-title");
    let entry = gtk::PasswordEntry::new();
    entry.set_show_peek_icon(true);
    entry.add_css_class("lock-entry");
    entry.set_margin_start(24);
    entry.set_margin_end(24);
    let status = gtk::Label::new(None);
    status.add_css_class("lock-status");
    let spacer2 = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer2.set_vexpand(true);
    pp.append(&small);
    pp.append(&title);
    pp.append(&entry);
    pp.append(&status);
    pp.append(&spacer2);
    stack.add_named(&pp, Some("pass"));

    root.append(&stack);

    let lock = Rc::new(LockInner {
        window,
        root: root.clone(),
        stack: stack.clone(),
        entry: entry.clone(),
        status,
        message: message.clone(),
        busy: Cell::new(false),
        locked: Cell::new(false),
    });

    let l = lock.clone();
    pp.append(&keyboard(Rc::new(move |k: Key| key(&l, k))));

    // swipe up or tap on the clock page -> password page
    let g = gtk::GestureDrag::new();
    let l = lock.clone();
    g.connect_drag_end(move |_, dx, dy| {
        if dy < -40.0 || (dx.abs() < 10.0 && dy.abs() < 10.0) {
            l.ask();
        }
    });
    cp.add_controller(g);

    // physical keyboard: Enter submits, Escape goes back to the clock,
    // typing on the clock page jumps to the password page
    let l = lock.clone();
    entry.connect_activate(move |_| submit(&l));
    let k = gtk::EventControllerKey::new();
    let l = lock.clone();
    k.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gdk::Key::Escape {
            l.stack.set_visible_child_name("clock");
            return glib::Propagation::Stop;
        }
        if l.stack.visible_child_name().as_deref() == Some("clock") {
            l.ask();
        }
        glib::Propagation::Proceed
    });
    root.add_controller(k);

    lock.set_message(&shell.cfg().get("lock_message"));
    match &lock.window {
        Some(w) => w.set_child(Some(&root)),
        None => root.set_visible(false),
    }
    lock
}

impl LockInner {
    pub fn embedded(&self) -> Option<gtk::Widget> {
        if self.window.is_none() {
            Some(self.root.clone().upcast())
        } else {
            None
        }
    }

    pub fn set_message(&self, text: &str) {
        self.message.set_text(text.trim());
        self.message.set_visible(!text.trim().is_empty());
    }

    pub fn is_locked(&self) -> bool {
        self.locked.get()
    }

    pub fn show(&self) {
        self.locked.set(true);
        self.entry.set_text("");
        self.status.set_text("");
        self.stack.set_visible_child_name("clock");
        match &self.window {
            Some(w) => w.present(),
            None => self.root.set_visible(true),
        }
    }

    fn hide(&self) {
        self.locked.set(false);
        self.entry.set_text("");
        match &self.window {
            Some(w) => w.set_visible(false),
            None => self.root.set_visible(false),
        }
    }

    fn ask(&self) {
        self.stack.set_visible_child_name("pass");
        self.entry.grab_focus();
    }
}

fn key(lock: &Lock, k: Key) {
    match k {
        Key::Char(c) => {
            let mut t = lock.entry.text().to_string();
            t.push_str(&c);
            lock.entry.set_text(&t);
            lock.entry.set_position(-1);
        }
        Key::Back => {
            let mut t = lock.entry.text().to_string();
            t.pop();
            lock.entry.set_text(&t);
            lock.entry.set_position(-1);
        }
        Key::Enter => submit(lock),
    }
}

fn current_user() -> String {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| glib::user_name().to_string_lossy().into_owned())
}

fn submit(lock: &Lock) {
    if lock.busy.replace(true) {
        return;
    }
    let pw = lock.entry.text().to_string();
    if pw.is_empty() {
        lock.busy.set(false);
        return;
    }
    lock.status.set_text("Checking…");
    let user = current_user();
    let l = lock.clone();
    glib::spawn_future_local(async move {
        let ok = gio::spawn_blocking(move || pam::authenticate(&user, &pw))
            .await
            .unwrap_or(false);
        l.busy.set(false);
        if ok {
            l.status.set_text("");
            l.hide();
        } else {
            l.status.set_text("Wrong password");
            l.entry.set_text("");
        }
    });
}

// ------------------------------------------------------------- keyboard

const LOWER: &[&str] = &[
    "q w e r t y u i o p",
    "a s d f g h j k l",
    "SHIFT z x c v b n m BACK",
    "SYM , SPACE . ENTER",
];
const UPPER: &[&str] = &[
    "Q W E R T Y U I O P",
    "A S D F G H J K L",
    "SHIFT Z X C V B N M BACK",
    "SYM , SPACE . ENTER",
];
const SYMS: &[&str] = &[
    "1 2 3 4 5 6 7 8 9 0",
    "@ # $ % & * - + ( )",
    "_ = / \\ | < > [ ] ^",
    "~ ! \" ' : ; ? ` BACK",
    "ABC { SPACE } ENTER",
];

fn keyboard(on_key: Rc<dyn Fn(Key)>) -> gtk::Widget {
    let stack = gtk::Stack::new();
    stack.add_css_class("osk");
    for (name, rows) in [("lower", LOWER), ("upper", UPPER), ("syms", SYMS)] {
        let grid = gtk::Box::new(gtk::Orientation::Vertical, 6);
        for row in rows.iter() {
            let r = gtk::Box::new(gtk::Orientation::Horizontal, 5);
            for tok in row.split(' ') {
                let label = match tok {
                    "SHIFT" => "⇧",
                    "BACK" => "⌫",
                    "SYM" => "?123",
                    "ABC" => "abc",
                    "SPACE" => " ",
                    "ENTER" => "⏎",
                    t => t,
                };
                let b = gtk::Button::with_label(label);
                b.add_css_class("key");
                b.set_hexpand(true);
                b.set_focusable(false); // keep focus in the password entry
                if tok == "SPACE" {
                    b.set_width_request(150);
                }
                if matches!(tok, "SHIFT" | "BACK" | "SYM" | "ABC" | "ENTER") {
                    b.add_css_class("key-special");
                }
                let (on_key, st, tok) = (on_key.clone(), stack.clone(), tok.to_string());
                b.connect_clicked(move |_| match tok.as_str() {
                    "SHIFT" => {
                        st.set_visible_child_name(if name == "upper" { "lower" } else { "upper" })
                    }
                    "SYM" => st.set_visible_child_name("syms"),
                    "ABC" => st.set_visible_child_name("lower"),
                    "BACK" => on_key(Key::Back),
                    "ENTER" => on_key(Key::Enter),
                    "SPACE" => on_key(Key::Char(" ".into())),
                    c => {
                        on_key(Key::Char(c.to_string()));
                        if name == "upper" {
                            st.set_visible_child_name("lower"); // one-shot shift
                        }
                    }
                });
                r.append(&b);
            }
            grid.append(&r);
        }
        stack.add_named(&grid, Some(name));
    }
    stack.set_visible_child_name("lower");
    stack.upcast()
}

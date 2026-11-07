//! Lock screen: clock -> swipe up -> password with a built-in full keyboard
//! (letters, caps, numbers, symbols), so you can always type your real
//! password by touch. A physical keyboard works too. Checked with PAM.
//!
//! The screen is locked with the Wayland session-lock protocol
//! (ext-session-lock-v1), so the compositor itself keeps it locked: if
//! RustTouch crashes or is killed, the screen stays covered (rusttouch-shell
//! starts RustTouch again, and it locks again by itself), and nothing else can
//! draw over the lock screen. A compositor without that protocol gets no
//! lock at all (it fails closed: "Can't lock" instead of a cover a crash
//! would remove), unless `insecure_lock = on` in rusttouch.conf asks for the
//! old Overlay-layer cover anyway.
//!
//! Guessing is slowed down: after 5 wrong passwords in a row the lock screen
//! waits 30s before the next try, doubling with every further wrong one (up
//! to 10 minutes). The count is kept in ~/.local/state/rusttouch, so it
//! survives a restart of RustTouch or of the computer.

use std::cell::Cell;
#[cfg(feature = "layer")]
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::shell::Shell;
use crate::{clock, pam};

pub struct LockInner {
    /// the plain (Overlay layer) lock surface: only used when the compositor
    /// can't do a real session lock
    window: Option<gtk::ApplicationWindow>,
    /// the real session lock (None: windowed mode, or not supported)
    #[cfg(feature = "layer")]
    session: Option<gtk4_session_lock::Instance>,
    /// this lock's surfaces, one per monitor (made new for every lock)
    #[cfg(feature = "layer")]
    lock_windows: RefCell<Vec<gtk::ApplicationWindow>>,
    root: gtk::Box,
    stack: gtk::Stack,
    entry: gtk::PasswordEntry,
    status: gtk::Label,
    message: gtk::Label,
    busy: Cell<bool>,
    locked: Cell<bool>,
    /// the plain cover may be used when there's no session lock
    allow_plain: bool,
    /// the countdown after too many wrong passwords is running
    counting: Rc<Cell<bool>>,
    /// run once the screen is really locked (the compositor said so)
    when_locked: std::cell::RefCell<Vec<Box<dyn FnOnce()>>>,
}

pub type Lock = Rc<LockInner>;

pub enum Key {
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
    // no "show password" eye: anyone holding the device could reveal what
    // the owner left half typed
    entry.set_show_peek_icon(false);
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

    // a real session lock when the compositor offers one (sway, labwc, phoc...)
    #[cfg(feature = "layer")]
    let session = (window.is_some() && gtk4_session_lock::is_supported())
        .then(gtk4_session_lock::Instance::new);
    let allow_plain = shell.cfg().on("insecure_lock");
    #[cfg(feature = "layer")]
    eprintln!(
        "rusttouch: lock screen: {}",
        if session.is_some() {
            "secure (session lock: stays locked even if RustTouch crashes)"
        } else if window.is_some() && allow_plain {
            "WARNING: plain cover (insecure_lock = on) - this compositor has no session lock, a crash unlocks the screen"
        } else if window.is_some() {
            "NOT AVAILABLE - this compositor has no session lock (use sway), so RustTouch won't pretend to lock"
        } else {
            "in the window"
        }
    );
    let lock = Rc::new(LockInner {
        window,
        #[cfg(feature = "layer")]
        session,
        #[cfg(feature = "layer")]
        lock_windows: RefCell::new(Vec::new()),
        root: root.clone(),
        stack: stack.clone(),
        entry: entry.clone(),
        status,
        message: message.clone(),
        busy: Cell::new(false),
        locked: Cell::new(false),
        allow_plain,
        counting: Rc::new(Cell::new(false)),
        when_locked: std::cell::RefCell::new(Vec::new()),
    });

    #[cfg(feature = "layer")]
    if let Some(s) = &lock.session {
        // the compositor refused (another locker holds the screen?): the
        // plain lock screen is better than none
        let l = Rc::downgrade(&lock);
        s.connect_failed(move |_| {
            let Some(l) = l.upgrade() else { return };
            l.lock_windows.borrow_mut().clear();
            if l.locked.get() && l.allow_plain {
                eprintln!("rusttouch: the compositor refused the session lock - using the plain cover (insecure_lock = on)");
                l.show_plain();
            } else if l.locked.get() {
                // (another locker may hold the screen; either way, no pretending)
                eprintln!("rusttouch: the compositor refused the session lock - not locked by RustTouch");
                l.locked.set(false);
                set_marker(false);
            }
            l.run_when_locked();
        });
        // the compositor confirms the screen is covered: now it may sleep
        let l = Rc::downgrade(&lock);
        s.connect_locked(move |_| {
            if let Some(l) = l.upgrade() {
                l.run_when_locked();
            }
        });
        // the lock ended without us unlocking it (the compositor did): if we
        // still think it's locked, lock again
        let l = Rc::downgrade(&lock);
        s.connect_unlocked(move |_| {
            let Some(l) = l.upgrade() else { return };
            l.lock_windows.borrow_mut().clear();
            if l.locked.get() {
                eprintln!("rusttouch: the compositor ended the session lock - locking again");
                let l = l.clone();
                let _ = glib::idle_add_local_once(move || {
                    if l.locked.get() {
                        let _ = l.show();
                    }
                });
            }
        });
    }

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
            // (a half-typed password doesn't stay behind)
            l.entry.set_text("");
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

    /// Lock the screen. False: it can't be locked securely here (no session
    /// lock, and `insecure_lock` is off) - nothing was done.
    pub fn show(&self) -> bool {
        #[cfg(feature = "layer")]
        let secure = self.session.is_some();
        #[cfg(not(feature = "layer"))]
        let secure = false;
        // (the windowed phone emulator is just a preview: its lock page is fine)
        let plain_ok = self.window.is_none() || self.allow_plain;
        if !secure && !plain_ok {
            eprintln!(
                "rusttouch: can't lock: this window manager has no session lock (ext-session-lock-v1)"
            );
            return false;
        }
        self.locked.set(true);
        set_marker(true);
        self.entry.set_text("");
        self.status.set_text("");
        self.stack.set_visible_child_name("clock");
        self.show_lockout();
        #[cfg(feature = "layer")]
        if let Some(s) = &self.session {
            if self.lock_session(s) {
                return true;
            }
            if !plain_ok {
                eprintln!("rusttouch: couldn't start the session lock - not locked");
                self.locked.set(false);
                set_marker(false);
                return false;
            }
            eprintln!("rusttouch: couldn't start the session lock - using the plain cover (insecure_lock = on)");
        }
        self.show_plain();
        true
    }

    /// Run `f` once the screen is really covered: when the compositor
    /// confirms the session lock - or a moment after the plain cover went
    /// up, or right away if nothing is being locked. Waits 3s at most.
    pub fn when_locked(self: &Rc<Self>, f: impl FnOnce() + 'static) {
        #[cfg(feature = "layer")]
        let waiting = self.locked.get()
            && self
                .session
                .as_ref()
                .map(|s| !s.is_locked())
                .unwrap_or(false)
            && !self.lock_windows.borrow().is_empty();
        #[cfg(not(feature = "layer"))]
        let waiting = false;
        if !waiting {
            // the plain cover (or nothing): give it a few frames
            let _ = glib::timeout_add_local_once(std::time::Duration::from_millis(400), f);
            return;
        }
        self.when_locked.borrow_mut().push(Box::new(f));
        let l = Rc::downgrade(self);
        let _ = glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            if let Some(l) = l.upgrade() {
                l.run_when_locked();
            }
        });
    }

    fn run_when_locked(&self) {
        let pending: Vec<Box<dyn FnOnce()>> = self.when_locked.borrow_mut().drain(..).collect();
        for f in pending {
            f();
        }
    }

    /// The plain lock surface (Overlay layer), or the lock page in the window.
    fn show_plain(&self) {
        match &self.window {
            Some(w) => {
                self.take_root();
                w.set_child(Some(&self.root));
                w.present();
            }
            None => self.root.set_visible(true),
        }
    }

    /// Lock with ext-session-lock-v1: a new lock surface on every monitor,
    /// the lock screen itself on the main one.
    #[cfg(feature = "layer")]
    fn lock_session(&self, s: &gtk4_session_lock::Instance) -> bool {
        if s.is_locked() {
            return true;
        }
        let Some(app) = gio::Application::default().and_downcast::<gtk::Application>() else {
            return false;
        };
        let Some(display) = gdk::Display::default() else {
            return false;
        };
        if !s.lock() {
            return false;
        }
        let monitors: Vec<gdk::Monitor> = {
            let list = display.monitors();
            (0..list.n_items())
                .filter_map(|i| list.item(i).and_downcast::<gdk::Monitor>())
                .collect()
        };
        let main = crate::shell::shell_monitor()
            .and_then(|m| monitors.iter().position(|x| *x == m))
            .unwrap_or(0);
        self.take_root();
        let mut wins = Vec::new();
        for (i, m) in monitors.iter().enumerate() {
            // (the library destroys these when the lock ends)
            let w = gtk::ApplicationWindow::new(&app);
            w.set_title(Some("rusttouch lock"));
            if i == main {
                w.set_child(Some(&self.root));
            } else {
                // other screens: just covered
                let blank = gtk::Box::new(gtk::Orientation::Vertical, 0);
                blank.add_css_class("lock");
                w.set_child(Some(&blank));
            }
            s.assign_window_to_monitor(&w, m);
            w.present();
            wins.push(w);
        }
        *self.lock_windows.borrow_mut() = wins;
        true
    }

    /// Take the lock page out of whatever window holds it.
    fn take_root(&self) {
        if let Some(p) = self.root.parent() {
            match p.downcast_ref::<gtk::Window>() {
                Some(w) => w.set_child(None::<&gtk::Widget>),
                None => self.root.unparent(),
            }
        }
    }

    fn hide(&self) {
        self.locked.set(false);
        set_marker(false);
        self.entry.set_text("");
        #[cfg(feature = "layer")]
        if let Some(s) = &self.session {
            if s.is_locked() {
                // keep the lock page: the lock's windows are destroyed now
                self.take_root();
                s.unlock();
                self.lock_windows.borrow_mut().clear();
            }
        }
        match &self.window {
            Some(w) => w.set_visible(false),
            None => self.root.set_visible(false),
        }
    }

    /// While a wait after too many wrong passwords is running: say how long
    /// is left, counting down.
    fn show_lockout(&self) {
        if wait_left() == 0 {
            return;
        }
        self.status.set_text(&wait_text(wait_left()));
        if self.counting.replace(true) {
            return;
        }
        let (status, counting) = (self.status.clone(), self.counting.clone());
        let _ = glib::timeout_add_seconds_local(1, move || {
            let left = wait_left();
            if left == 0 {
                status.set_text("");
                counting.set(false);
                return glib::ControlFlow::Break;
            }
            status.set_text(&wait_text(left));
            glib::ControlFlow::Continue
        });
    }

    fn ask(&self) {
        self.stack.set_visible_child_name("pass");
        self.entry.grab_focus();
    }
}

fn key(lock: &Lock, k: Key) {
    match k {
        // typed straight into the field, so no extra copies of the
        // password are left around in memory
        Key::Char(c) => {
            let mut end = lock.entry.text().chars().count() as i32;
            lock.entry.insert_text(&c, &mut end);
            lock.entry.set_position(-1);
        }
        Key::Back => {
            let end = lock.entry.text().chars().count() as i32;
            if end > 0 {
                lock.entry.delete_text(end - 1, end);
            }
            lock.entry.set_position(-1);
        }
        Key::Enter => submit(lock),
    }
}

/// A file that says "the screen is locked", in RustTouch's private runtime
/// folder. If RustTouch crashes while locked, the compositor keeps the screen
/// covered, rusttouch-shell starts RustTouch again, and this file tells it to
/// lock again straight away (so you get the unlock screen back).
fn set_marker(on: bool) {
    let p = crate::sys::runtime_dir().join("locked");
    if on {
        let _ = std::fs::write(&p, b"");
    } else {
        let _ = std::fs::remove_file(&p);
    }
}

/// Was the screen locked when RustTouch last stopped?
pub fn was_locked() -> bool {
    crate::sys::runtime_dir().join("locked").exists()
}

#[repr(C)]
struct Passwd {
    pw_name: *const std::os::raw::c_char,
    // (the other fields aren't read)
}

extern "C" {
    fn getpwuid(uid: u32) -> *const Passwd;
}

/// The account that is logged in, from the system's user list (getpwuid) -
/// never from $USER, which anything that sets the session's environment
/// (like ~/.config/rusttouch/env) could change.
fn current_user() -> Option<String> {
    unsafe {
        let p = getpwuid(crate::sys::my_uid());
        if p.is_null() || (*p).pw_name.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr((*p).pw_name)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

// ----------------------------------------------- slowing down guessing

/// Wrong passwords in a row before the lock screen makes you wait.
const FREE_TRIES: u32 = 5;

/// ~/.local/state/rusttouch/lock-tries: "<wrong tries in a row> <wait until,
/// in seconds since 1970>". Kept outside the runtime folder so restarting
/// RustTouch or the computer doesn't reset it.
fn tries_file() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        })?;
    Some(base.join("rusttouch").join("lock-tries"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_tries() -> (u32, u64) {
    let Some(s) = tries_file().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return (0, 0);
    };
    let mut it = s.split_whitespace().map(|x| x.parse::<u64>().unwrap_or(0));
    let fails = it.next().unwrap_or(0).min(u64::from(u32::MAX)) as u32;
    (fails, it.next().unwrap_or(0))
}

fn write_tries(fails: u32, until: u64) {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let Some(p) = tries_file() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&p)
    {
        let _ = writeln!(f, "{fails} {until}");
    }
}

/// The longest wait (10 minutes).
const MAX_WAIT: u64 = 600;

thread_local! {
    /// The same count kept in memory, with the wait timed by the system's
    /// steady clock: it still counts if the file can't be written (full
    /// disk, read-only home), and moving the clock forward doesn't skip it.
    static MEM_FAILS: Cell<u32> = const { Cell::new(0) };
    static MEM_UNTIL: Cell<Option<std::time::Instant>> = const { Cell::new(None) };
}

/// Seconds left before the next try is allowed (0 = now): the longer of the
/// saved wait and the one in memory, never more than 10 minutes (if the clock
/// went backwards, the saved one would otherwise last far too long).
fn wait_left() -> u64 {
    let saved = read_tries().1.saturating_sub(now_secs()).min(MAX_WAIT);
    let mem = MEM_UNTIL
        .with(|u| u.get())
        .map(|t| {
            t.saturating_duration_since(std::time::Instant::now())
                .as_secs()
        })
        .unwrap_or(0);
    saved.max(mem).min(MAX_WAIT)
}

/// After a wrong password: from the 5th one in a row, wait 30s, then twice
/// as long after each further one (60s, 120s...), at most 10 minutes.
fn note_wrong() {
    let fails = read_tries()
        .0
        .max(MEM_FAILS.with(|f| f.get()))
        .saturating_add(1);
    MEM_FAILS.with(|f| f.set(fails));
    let wait = if fails >= FREE_TRIES {
        30u64
            .saturating_mul(1u64 << (fails - FREE_TRIES).min(5))
            .min(MAX_WAIT)
    } else {
        0
    };
    let until =
        (wait > 0).then(|| std::time::Instant::now() + std::time::Duration::from_secs(wait));
    MEM_UNTIL.with(|u| u.set(until));
    write_tries(fails, if wait > 0 { now_secs() + wait } else { 0 });
}

/// The right password: the count starts over.
fn reset_tries() {
    MEM_FAILS.with(|f| f.set(0));
    MEM_UNTIL.with(|u| u.set(None));
    write_tries(0, 0);
}

fn wait_text(secs: u64) -> String {
    if secs >= 60 {
        format!(
            "Too many tries. Try again in {}:{:02}",
            secs / 60,
            secs % 60
        )
    } else {
        format!("Too many tries. Try again in {secs}s")
    }
}

fn submit(lock: &Lock) {
    if lock.busy.replace(true) {
        return;
    }
    let pw = lock.entry.text().to_string();
    // (the field's own copy goes; ours is wiped once it's been checked)
    lock.entry.set_text("");
    // still waiting after too many wrong tries: not even checked
    if wait_left() > 0 {
        pam::wipe(pw);
        lock.busy.set(false);
        lock.show_lockout();
        return;
    }
    if pw.is_empty() {
        lock.busy.set(false);
        return;
    }
    let Some(user) = current_user() else {
        pam::wipe(pw);
        lock.status.set_text("Can't find your account");
        lock.busy.set(false);
        return;
    };
    lock.status.set_text("Checking…");
    let l = lock.clone();
    glib::spawn_future_local(async move {
        let ok = gio::spawn_blocking(move || pam::authenticate(&user, pw))
            .await
            .unwrap_or(false);
        l.busy.set(false);
        if ok {
            reset_tries();
            l.status.set_text("");
            l.hide();
        } else {
            note_wrong();
            l.entry.set_text("");
            if wait_left() > 0 {
                l.show_lockout();
            } else {
                l.status.set_text("Wrong password");
            }
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

pub fn keyboard(on_key: Rc<dyn Fn(Key)>) -> gtk::Widget {
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
                b.set_focusable(false); // keep focus in the text field
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

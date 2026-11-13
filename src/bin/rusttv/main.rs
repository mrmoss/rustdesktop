//! RustTV: a home screen for a TV box, made for a remote control.
//!
//! Arrows move around, OK opens, Back goes back, Home (or the Windows key)
//! comes back here from any app. Hold OK - or press Menu - on a card for its
//! options (move, edit, remove, close). Everything opens full screen on a
//! workspace of its own (sway).
//!
//! `rusttv --do home` (sway's Home key binding) talks to the running RustTV.
//! It is separate from RustTouch: its own program, login session, settings
//! (~/.config/rusttv) and sway config.

// (RustTouch's own: Wi-Fi passwords over D-Bus, and the sound devices)
#[allow(dead_code)]
#[path = "../../audio.rs"]
mod audio;
#[allow(dead_code)]
#[path = "../../nm.rs"]
mod nm;

mod airplay;
mod appkbd;
mod autologin;
mod battery;
mod config;
mod home;
mod icons;
mod idle;
#[allow(dead_code)]
mod im;
mod keyboard;
mod launch;
mod media;
#[allow(dead_code)]
mod net;
mod netui;
mod osd;
mod remote;
mod screens;
mod settings;
mod sway;
mod ui;
mod updates;

use config::Config;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

pub const APP_ID: &str = "dev.rusttv.TV";
const CSS: &str = include_str!("style.css");

/// A page on top of home (settings, a list to choose from, the keyboard...).
pub struct Page {
    pub widget: gtk::Widget,
    /// what had the focus before it opened (gets it back after)
    pub back_to: Option<gtk::Widget>,
    /// handles keys first (the keyboard page types with them)
    pub keys: Option<Rc<dyn Fn(gdk::Key, u32) -> bool>>,
    /// redraws it (after a change made on a page above it)
    pub refresh: Option<Rc<dyn Fn()>>,
}

pub struct TvInner {
    pub app: gtk::Application,
    pub win: gtk::ApplicationWindow,
    pub stack: gtk::Stack,
    pub cfg: RefCell<Config>,
    pub pages: RefCell<Vec<Page>>,
    pub home: RefCell<Option<home::Home>>,
    /// card colours (one CSS class per launcher)
    colors: gtk::CssProvider,
    toast: gtk::Label,
    toast_timer: RefCell<Option<glib::SourceId>>,
    /// card options: widget -> what Menu (or holding OK) opens for it
    menus: RefCell<Vec<(glib::WeakRef<gtk::Widget>, Rc<dyn Fn()>)>>,
    page_n: Cell<u32>,
    /// the volume pop-up (None: can't float over apps here)
    pub osd: Option<Rc<osd::Osd>>,
}

pub type Tv = Rc<TvInner>;

thread_local! {
    static TV: RefCell<Option<Tv>> = const { RefCell::new(None) };
}

/// The running TV screen (on the main thread).
pub fn with_tv(f: impl FnOnce(&Tv)) {
    if let Some(tv) = TV.with(|t| t.borrow().clone()) {
        f(&tv);
    }
}

/// The running TV screen's answer to `f` (None: not running).
pub fn with_tv_ret<T>(f: impl FnOnce(&Tv) -> T) -> Option<T> {
    TV.with(|t| t.borrow().clone()).map(|tv| f(&tv))
}

/// Run `f` on the main thread (from a worker thread).
pub fn on_main(f: impl FnOnce() + Send + 'static) {
    glib::MainContext::default().invoke(f);
}

impl TvInner {
    pub fn cfg(&self) -> Config {
        self.cfg.borrow().clone()
    }

    /// Change the settings, save them and redraw what shows them.
    pub fn change(&self, f: impl FnOnce(&mut Config)) {
        f(&mut self.cfg.borrow_mut());
        self.cfg.borrow().save();
        self.apply_colors();
        if let Some(h) = self.home.borrow().as_ref() {
            home::rebuild(h);
        }
        let refreshers: Vec<Rc<dyn Fn()>> = self
            .pages
            .borrow()
            .iter()
            .filter_map(|p| p.refresh.clone())
            .collect();
        for r in refreshers {
            r();
        }
    }

    fn apply_colors(&self) {
        let mut css = String::new();
        for l in &self.cfg.borrow().launchers {
            let c = if config::valid_color(&l.color) {
                l.color.as_str()
            } else {
                "#607d8b"
            };
            css.push_str(&format!(
                ".lc-{} {{ background: {c}; color: {}; }}\n",
                l.id,
                ui::text_on(c)
            ));
            // a card with the service's logo: a dark shade of its colour
            // behind it (the colour itself is as bright as the logo)
            let dark = ui::shade(c, 0.22);
            css.push_str(&format!(
                ".lc-{}.with-picture {{ background: {dark}; color: #f2f2f5; }}\n",
                l.id
            ));
        }
        css.push_str(&theme_css(&self.cfg.borrow()));
        self.colors.load_from_data(&css);
    }

    pub fn focused(&self) -> Option<gtk::Widget> {
        gtk::prelude::RootExt::focus(&self.win)
    }

    /// Open a page on top.
    pub fn push(
        &self,
        widget: &impl IsA<gtk::Widget>,
        keys: Option<Rc<dyn Fn(gdk::Key, u32) -> bool>>,
    ) {
        self.push_page(widget, keys, None);
    }

    pub fn push_page(
        &self,
        widget: &impl IsA<gtk::Widget>,
        keys: Option<Rc<dyn Fn(gdk::Key, u32) -> bool>>,
        refresh: Option<Rc<dyn Fn()>>,
    ) {
        let widget = widget.clone().upcast::<gtk::Widget>();
        let n = self.page_n.get() + 1;
        self.page_n.set(n);
        let back_to = self.focused();
        self.stack.add_named(&widget, Some(&format!("page{n}")));
        self.stack.set_visible_child(&widget);
        self.pages.borrow_mut().push(Page {
            widget: widget.clone(),
            back_to,
            keys,
            refresh,
        });
        let w = widget.clone();
        let _ = glib::idle_add_local_once(move || {
            w.child_focus(gtk::DirectionType::TabForward);
        });
    }

    /// Close the top page (back to the one under it, or home).
    pub fn back(&self) -> bool {
        let Some(p) = self.pages.borrow_mut().pop() else {
            return false;
        };
        let under = self.pages.borrow().last().map(|p| p.widget.clone());
        match under {
            Some(u) => self.stack.set_visible_child(&u),
            None => self.stack.set_visible_child_name("home"),
        }
        let stack = self.stack.clone();
        let old = p.widget.clone();
        // (removed once it's out of sight)
        let _ = glib::timeout_add_local_once(Duration::from_millis(400), move || {
            if old.parent().is_some() {
                stack.remove(&old);
            }
        });
        if let Some(f) = p.back_to.filter(|f| f.is_visible() && f.root().is_some()) {
            f.grab_focus();
        } else if let Some(u) = self.pages.borrow().last() {
            u.widget.child_focus(gtk::DirectionType::TabForward);
        } else if let Some(h) = self.home.borrow().as_ref() {
            home::focus_first(h);
        }
        true
    }

    /// Close every page.
    pub fn close_pages(&self) {
        while self.back() {}
    }

    /// Back to the home screen, from anywhere (an app, settings...).
    pub fn go_home(&self) {
        self.close_pages();
        // leaving an app: pause what's playing in it
        if self.cfg.borrow().pause_on_home
            && sway::available()
            && sway::current_workspace() != sway::HOME
        {
            media::run("pause-all");
        }
        if sway::available() {
            sway::run(&format!("workspace {}", sway::HOME));
        }
        self.win.present();
        if let Some(h) = self.home.borrow().as_ref() {
            home::refresh_running(h);
            if self.focused().is_none() {
                home::focus_first(h);
            }
        }
    }

    /// A short message at the bottom of the screen.
    pub fn toast(&self, text: &str) {
        self.toast.set_text(text);
        self.toast.set_visible(true);
        if let Some(t) = self.toast_timer.borrow_mut().take() {
            t.remove();
        }
        let l = self.toast.clone();
        let me = TV.with(|t| t.borrow().clone());
        let id = glib::timeout_add_local_once(Duration::from_secs(4), move || {
            l.set_visible(false);
            if let Some(me) = me {
                me.toast_timer.borrow_mut().take();
            }
        });
        *self.toast_timer.borrow_mut() = Some(id);
    }

    /// What Menu (or holding OK) on `w` opens.
    pub fn set_menu(&self, w: &impl IsA<gtk::Widget>, f: impl Fn() + 'static) {
        let mut m = self.menus.borrow_mut();
        m.retain(|(w, _)| w.upgrade().is_some());
        m.push((w.upcast_ref::<gtk::Widget>().downgrade(), Rc::new(f)));
    }

    /// The options of the focused card, if it has any.
    fn menu_for_focus(&self) -> Option<Rc<dyn Fn()>> {
        let mut at = self.focused();
        while let Some(w) = at {
            let found = self
                .menus
                .borrow()
                .iter()
                .find(|(m, _)| m.upgrade().as_ref() == Some(&w))
                .map(|(_, f)| f.clone());
            if found.is_some() {
                return found;
            }
            at = w.parent();
        }
        None
    }

    /// Open something: a launcher, or switch to it if it's already open.
    pub fn open_launcher(&self, l: &config::Launcher) {
        if let Some(w) = launch::launcher_window(l, &sway::windows()) {
            if !launch::changed_since_open(l) {
                self.switch_to(&w);
                return;
            }
            // edited since it opened: open it again with the changes?
            let l2 = l.clone();
            ui::choose(
                &TV.with(|t| t.borrow().clone()).expect("RustTV is running"),
                &format!("{} was changed", l.name),
                vec![
                    ("reload".into(), "Reload".into()),
                    ("show".into(), "Open as is".into()),
                ],
                "reload",
                move |t, v| {
                    if v == "reload" {
                        let l3 = l2.clone();
                        t.toast(&format!("Reloading {}...", l2.name));
                        launch::close_then(vec![w.clone()], move || {
                            with_tv(|t| t.open_launcher(&l3));
                        });
                    } else {
                        t.switch_to(&w);
                    }
                },
            );
            return;
        }
        let real = launch::effective(l);
        if !launch::installed(&real.browser) {
            self.toast(&if l.spatial && !launch::is_chromium(&l.browser) {
                format!(
                    "Spatial selecting needs Chromium or Chrome (sudo apt install chromium) - or turn it off for {}",
                    l.name
                )
            } else {
                format!(
                    "{} isn't installed - pick another browser for {} (hold OK on it > Edit)",
                    ui::browser_name(&l.browser),
                    l.name
                )
            });
            return;
        }
        if l.url.trim().is_empty() {
            self.toast(&format!(
                "{} has no address yet (hold OK on it > Edit)",
                l.name
            ));
            return;
        }
        let shared = self.cfg.borrow().own_profile;
        // Chromium open already with other switches: start it afresh
        let in_way = launch::chromium_in_the_way(l, shared, &sway::windows());
        if !in_way.is_empty() {
            let l2 = l.clone();
            self.toast(&format!("Opening {}...", l.name));
            launch::close_then(in_way, move || with_tv(|t| t.start_launcher(&l2)));
            return;
        }
        self.start_launcher(l);
    }

    /// Open a launcher as a plain web page - no TV headers, no spatial
    /// selecting (they get in the way of some sites' sign-in pages) - in its
    /// usual browser and profile, so the sign-in is kept for next time.
    /// (Opening it normally afterwards offers to reload it.)
    pub fn open_for_sign_in(&self, l: &config::Launcher) {
        let mut plain = l.clone();
        plain.tv = false;
        plain.spatial = false;
        // (same browser it normally opens in: same sign-ins)
        plain.browser = launch::effective(l).browser;
        self.toast(&format!("Opening {} for signing in...", l.name));
        match launch::launcher_window(l, &sway::windows()) {
            Some(w) => {
                launch::close_then(vec![w], move || with_tv(|t| t.start_or_restart(&plain)));
            }
            None => self.start_or_restart(&plain),
        }
    }

    /// Start a launcher, first closing a Chromium that's in the way.
    fn start_or_restart(&self, l: &config::Launcher) {
        let shared = self.cfg.borrow().own_profile;
        let in_way = launch::chromium_in_the_way(l, shared, &sway::windows());
        if in_way.is_empty() {
            self.start_launcher(l);
        } else {
            let l2 = l.clone();
            launch::close_then(in_way, move || with_tv(|t| t.start_launcher(&l2)));
        }
    }

    fn start_launcher(&self, l: &config::Launcher) {
        launch::close_others_when_new(self.cfg.borrow().max_apps);
        let shared = self.cfg.borrow().own_profile;
        if launch::open_launcher(l, shared) {
            self.toast(&format!("Opening {}...", l.name));
        } else {
            self.toast(&format!("Couldn't open {}", l.name));
        }
    }
}

impl TvInner {
    /// Show an open window.
    pub fn switch_to(&self, w: &sway::Window) {
        launch::focus(w);
    }
}

/// The look from the settings: the accent colour (focus outline,
/// highlights) and the background.
fn theme_css(c: &Config) -> String {
    let a = if config::valid_color(&c.accent) {
        c.accent.as_str()
    } else {
        "#ffffff"
    };
    let on_a = ui::text_on(a);
    let mut css = format!(
        "button.card:focus, button.card:focus-visible {{ outline-color: {a}; }}\n\
         button.row:focus, button.row:focus-visible, button.key:focus, button.key:focus-visible \
         {{ background: {a}; color: {on_a}; }}\n\
         button.top-button:focus, button.top-button:focus-visible {{ background: {a}; color: {on_a}; }}\n\
         button.key.done {{ background: {}; }}\n\
         .osd levelbar block.filled, .vol-row levelbar block.filled {{ background: {a}; }}\n\
         .app-kbd-box button.key.sel {{ background: {a}; color: {on_a}; }}\n",
        if a == "#ffffff" { "#2f6fed" } else { a }
    );
    let bg = match c.background.as_str() {
        "black" => "background: #000000;".to_string(),
        "glow" => format!(
            "background: radial-gradient(ellipse at 15% 0%, {} 0%, #0a0c12 60%);",
            ui::shade(a, 0.35)
        ),
        "picture" if !c.wallpaper.is_empty() && std::path::Path::new(&c.wallpaper).is_file() => {
            // (as a file:// address: spaces and the like written as %20...)
            let uri = gio::File::for_path(&c.wallpaper).uri().replace('"', "%22");
            format!(
                "background: linear-gradient(rgba(0,0,0,0.55), rgba(0,0,0,0.55)), url(\"{uri}\"); \
                 background-size: cover; background-position: center;"
            )
        }
        _ => String::new(),
    };
    if !bg.is_empty() {
        css.push_str(&format!("window.rusttv {{ {bg} }}\n"));
    }
    css
}

fn is_ok(k: gdk::Key) -> bool {
    matches!(k, gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::Select)
}

fn is_back(k: gdk::Key) -> bool {
    matches!(
        k,
        gdk::Key::Escape | gdk::Key::BackSpace | gdk::Key::Back | gdk::Key::Cancel
    )
}

fn is_menu(k: gdk::Key) -> bool {
    matches!(k, gdk::Key::Menu | gdk::Key::MenuKB | gdk::Key::Option)
}

/// The remote's keys: Back, Home, Menu, and OK (tap = open, hold = options).
fn add_keys(tv: &Tv) {
    let held: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let fired: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let k = gtk::EventControllerKey::new();
    k.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (t, h, f, tm) = (tv.clone(), held.clone(), fired.clone(), timer.clone());
    k.connect_key_pressed(move |_, key, keycode, _| {
        // a page that types (the keyboard) gets the keys first
        // (keycode: X's, = the evdev code + 8)
        let keys = t.pages.borrow().last().and_then(|p| p.keys.clone());
        if let Some(keys) = keys {
            if keys(key, keycode.saturating_sub(8)) {
                return glib::Propagation::Stop;
            }
        }
        // your remote's own buttons (Settings > Remote buttons)
        let mine = remote::action_for(&t.cfg.borrow(), key);
        let dir = |d| {
            t.win.child_focus(d);
            glib::Propagation::Stop
        };
        match mine.as_deref() {
            Some("up") => return dir(gtk::DirectionType::Up),
            Some("down") => return dir(gtk::DirectionType::Down),
            Some("left") => return dir(gtk::DirectionType::Left),
            Some("right") => return dir(gtk::DirectionType::Right),
            Some(
                a @ ("home" | "close" | "keyboard" | "volume-up" | "volume-down" | "mute"
                | "play-pause" | "seek-back" | "seek-forward" | "previous" | "next" | "stop"),
            ) => {
                do_action(&t, a);
                return glib::Propagation::Stop;
            }
            _ => {}
        }
        let mine_is = |a: &str| mine.as_deref() == Some(a);
        if key == gdk::Key::HomePage {
            t.go_home();
            return glib::Propagation::Stop;
        }
        if mine_is("back") || (mine.is_none() && is_back(key)) {
            t.back();
            return glib::Propagation::Stop;
        }
        if mine_is("menu") || (mine.is_none() && is_menu(key)) {
            if let Some(m) = t.menu_for_focus() {
                m();
            }
            return glib::Propagation::Stop;
        }
        if mine_is("ok") || (mine.is_none() && is_ok(key)) {
            if h.replace(true) {
                return glib::Propagation::Stop; // (key repeat while held)
            }
            f.set(false);
            let (t2, f2, tm2) = (t.clone(), f.clone(), tm.clone());
            let id = glib::timeout_add_local_once(Duration::from_millis(650), move || {
                tm2.borrow_mut().take();
                if let Some(m) = t2.menu_for_focus() {
                    f2.set(true);
                    m();
                }
            });
            if let Some(old) = tm.borrow_mut().replace(id) {
                old.remove();
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    let (t, h, f, tm) = (tv.clone(), held, fired, timer);
    k.connect_key_released(move |_, key, _, _| {
        let mine = remote::action_for(&t.cfg.borrow(), key);
        let ok = mine.as_deref() == Some("ok") || (mine.is_none() && is_ok(key));
        if !ok || !h.replace(false) {
            return;
        }
        if let Some(id) = tm.borrow_mut().take() {
            id.remove();
        }
        if !f.replace(false) {
            if let Some(w) = t.focused() {
                w.activate();
            }
        }
    });
    tv.win.add_controller(k);
}

/// `rusttv --do <action>` from sway's key bindings, to the running RustTV.
fn control_path() -> std::path::PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| config::home().join(".cache"));
    dir.join("rusttv.control")
}

fn send_action(action: &str) -> bool {
    let s = std::os::unix::net::UnixDatagram::unbound();
    s.is_ok_and(|s| s.send_to(action.as_bytes(), control_path()).is_ok())
}

fn listen_actions() {
    let path = control_path();
    let _ = std::fs::remove_file(&path);
    let Ok(sock) = std::os::unix::net::UnixDatagram::bind(&path) else {
        eprintln!("rusttv: can't listen on {}", path.display());
        return;
    };
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    std::thread::spawn(move || {
        let mut buf = [0u8; 256];
        while let Ok(n) = sock.recv(&mut buf) {
            let action = String::from_utf8_lossy(&buf[..n]).trim().to_string();
            on_main(move || with_tv(|tv| do_action(tv, &action)));
        }
    });
}

/// Do an app's own button's job (`--do app-key NAME`).
fn app_key(tv: &Tv, name: &str) {
    let job = sway::focused_window().and_then(|w| {
        let cfg = tv.cfg();
        let l = cfg
            .launchers
            .iter()
            .find(|l| launch::launcher_window(l, std::slice::from_ref(&w)).is_some())?;
        l.keys
            .iter()
            .find(|k| k.name == name)
            .map(|k| k.action.clone())
    });
    let Some(job) = job else { return };
    if let Some(code) = remote::send_code(&job) {
        appkbd::tap(code);
    } else {
        do_action(tv, &job);
    }
}

/// Bind the buttons of the app on screen (and give back the last one's).
pub fn app_keys_changed(tv: &Tv) {
    let cfg = tv.cfg();
    let keys = sway::focused_window()
        .and_then(|w| {
            cfg.launchers
                .iter()
                .find(|l| launch::launcher_window(l, std::slice::from_ref(&w)).is_some())
                .map(|l| l.keys.clone())
        })
        .unwrap_or_default();
    remote::app_on_screen(&keys, &cfg.keys);
}

fn do_action(tv: &Tv, action: &str) {
    if let Some(name) = action.strip_prefix("app-key ") {
        app_key(tv, name.trim());
        return;
    }
    match action {
        // play, pause, skip, seek (whatever is playing)
        a @ ("play-pause" | "seek-back" | "seek-forward" | "previous" | "next" | "stop") => {
            media::run(a)
        }
        "home" => tv.go_home(),
        "settings" => {
            tv.go_home();
            settings::open(tv);
        }
        "back" => {
            tv.back();
        }
        // the keyboard for apps, again (the text box still has the focus)
        "keyboard" => {
            if !appkbd::show_again() {
                tv.toast("No text box to type in");
            }
        }
        // (from your remote's buttons: Settings > Remote buttons)
        "close" => {
            // the app on screen (never RustTV itself)
            if sway::available() && sway::current_workspace() != sway::HOME {
                if let Some(w) = sway::focused_window() {
                    sway::run(&format!("[con_id={}] kill", w.id));
                }
            }
        }
        // the remote's power button (Settings > Power button)
        "power-button" => settings::power_button(tv),
        // (and the volume pop-up shows how loud it is now)
        "volume-up" | "volume-down" | "mute" => osd::volume(action),
        other => eprintln!("rusttv: unknown action {other:?}"),
    }
}

/// Things happening to app windows (sway): keep the "Open now" row up to
/// date, put an app that opened on the home workspace on its own, and come
/// home when the last app on screen closes.
fn watch_sway() {
    if !sway::available() {
        return;
    }
    let pending = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    sway::watch_windows(move |change| {
        if pending.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return; // (one update for a burst of events)
        }
        let p = pending.clone();
        on_main(move || {
            let _ = glib::timeout_add_local_once(Duration::from_millis(250), move || {
                p.store(false, std::sync::atomic::Ordering::SeqCst);
                with_tv(|tv| windows_changed(tv, &change));
            });
        });
    });
}

thread_local! {
    /// the windows open at the last look (to tell which are new)
    static KNOWN: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
}

fn windows_changed(tv: &Tv, change: &str) {
    // (screens plugged in or out: the copies follow)
    screens::refresh(&tv.cfg());
    // (the app on screen's own remote buttons)
    app_keys_changed(tv);
    let wins = sway::windows();
    KNOWN.with(|k| {
        let mut k = k.borrow_mut();
        airplay::windows_changed(&wins);
        // a device started mirroring: its window on screen. (Not only a new
        // window: UxPlay's X11 one gets its name - how it's known - a moment
        // after it opens.)
        for w in wins
            .iter()
            .filter(|w| airplay::is_airplay(w) && !airplay::handled(w.id))
        {
            airplay::window_opened(w);
        }
        for w in wins
            .iter()
            .filter(|w| !k.contains(&w.id) && !airplay::is_airplay(w))
        {
            launch::new_window(w);
        }
        *k = wins.iter().map(|w| w.id).collect();
    });
    // (the AirPlay window isn't an app RustTV opened: not counted as one,
    // even if it was taken for one before it had its name)
    let apps: Vec<sway::Window> = wins
        .iter()
        .filter(|w| !airplay::is_airplay(w))
        .cloned()
        .collect();
    launch::windows_left(&apps);
    // an app that showed up on the home screen's workspace (opened by
    // something else, or Home was pressed while it was starting): give it a
    // workspace of its own
    for w in wins
        .iter()
        .filter(|w| w.workspace == sway::HOME && !airplay::is_airplay(w))
    {
        let ws = sway::free_workspace();
        // (shown if RustTV just opened it; otherwise it waits there)
        let show = if launch::take_just_opened() {
            format!("; workspace number {ws}")
        } else {
            String::new()
        };
        sway::run(&format!(
            "[con_id={}] move container to workspace number {ws}{show}",
            w.id
        ));
    }
    if change == "close" {
        let now = sway::current_workspace();
        if now != sway::HOME && !wins.iter().any(|w| w.workspace == now) {
            tv.go_home();
        }
    }
    if let Some(h) = tv.home.borrow().as_ref() {
        home::refresh_running(h);
    }
}

fn apply_scale(cfg: &Config) {
    if sway::available() && !cfg.scale.is_empty() {
        sway::run(&format!("output * scale {}", cfg.scale));
    }
}

fn build(app: &gtk::Application) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS);
    let colors = gtk::CssProvider::new();
    if let Some(d) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &d,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        gtk::style_context_add_provider_for_display(
            &d,
            &colors,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    }
    if let Some(s) = gtk::Settings::default() {
        s.set_gtk_application_prefer_dark_theme(true);
    }

    let win = gtk::ApplicationWindow::builder()
        .application(app)
        .title("RustTV")
        .default_width(1280)
        .default_height(720)
        .build();
    win.add_css_class("rusttv");
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(150);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&stack));
    let toast = gtk::Label::new(None);
    toast.add_css_class("toast");
    toast.set_halign(gtk::Align::Center);
    // (at the top: subtitles are at the bottom)
    toast.set_valign(gtk::Align::Start);
    toast.set_wrap(true);
    toast.set_visible(false);
    toast.set_can_target(false);
    overlay.add_overlay(&toast);
    win.set_child(Some(&overlay));

    let cfg = Config::load();
    apply_scale(&cfg);
    screens::apply_modes(&cfg);
    screens::refresh(&cfg);
    // (a button being learned when RustTV last stopped: keys back to normal)
    remote::learning(false);
    remote::apply(&[], &cfg.keys);
    idle::apply(cfg.auto_suspend);
    let tv: Tv = Rc::new(TvInner {
        app: app.clone(),
        win: win.clone(),
        stack: stack.clone(),
        cfg: RefCell::new(cfg),
        pages: RefCell::new(Vec::new()),
        home: RefCell::new(None),
        colors,
        toast,
        toast_timer: RefCell::new(None),
        menus: RefCell::new(Vec::new()),
        page_n: Cell::new(0),
        osd: osd::build(app),
    });
    TV.with(|t| *t.borrow_mut() = Some(tv.clone()));
    tv.apply_colors();
    let h = home::build(&tv);
    stack.add_named(&h.root, Some("home"));
    *tv.home.borrow_mut() = Some(h.clone());
    add_keys(&tv);
    battery::watch(&tv);
    if !appkbd::start(&tv) {
        eprintln!("rusttv: no on-screen keyboard for apps");
    }
    listen_actions();
    watch_sway();
    // the power button is RustTV's (Settings > Power button)
    settings::hold_power_key();
    // AirPlay (UxPlay), when it's on
    airplay::clean_up();
    airplay::apply(&tv.cfg());
    // the first run: nothing saved yet - save the starting launchers
    if !config::dir().join("rusttv.json").exists() {
        tv.cfg.borrow().save();
    }
    win.fullscreen();
    win.present();
    let h2 = h.clone();
    let _ = glib::idle_add_local_once(move || home::focus_first(&h2));
}

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--do") {
        // ("app-key Return": an app's own button)
        let rest = args[i + 1..].join(" ");
        let action = if rest.is_empty() {
            "home"
        } else {
            rest.as_str()
        };
        if send_action(action) {
            return glib::ExitCode::SUCCESS;
        }
        eprintln!("rusttv: RustTV isn't running");
        return glib::ExitCode::FAILURE;
    }
    if args.iter().any(|a| a == "--version") {
        println!("rusttv {}", env!("CARGO_PKG_VERSION"));
        return glib::ExitCode::SUCCESS;
    }
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(build);
    app.run_with_args::<&str>(&[])
}

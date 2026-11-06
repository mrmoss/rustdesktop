//! Shared shell state: sheets, home, lock, window list, status, sensors.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::config::{self, Config};
use crate::gestures::{self, Target};
use crate::icons::Glyph;
use crate::layer::{self, Anchors, KeyboardMode, Layer, Zone};
use crate::sheet::{self, Dir, Sheet};
use crate::sys::{self, Osk, Status};
use crate::wl::{self, Toplevel};
use crate::{anim, bars, drawer, home, lock, osd, power, quick, recents, sensors, theme, tray};

/// Messages from background threads to the GTK thread.
pub enum Event {
    Toplevels(Vec<Toplevel>),
    Orientation(String),
    Sleep(bool),
    /// a text field gained (true) / lost (false) focus
    TextInput(bool),
    /// from swayidle: "blank", "unblank", "sleep"
    Idle(String),
    /// hardware button (key code, pressed)
    Key(u16, bool),
    /// first status read finished (used to create/apply the config at login)
    Startup(Status),
    /// monitors were (re)configured; rusttouch lives on this one
    ShellOutput(Option<String>),
    /// sway: windows or workspaces changed (split screen handles, extra screens)
    Layout,
    /// sway: a window was created (container id)
    NewWindow(i64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    Drawer,
    Quick,
    Recents,
}

const ALL: [Overlay; 3] = [Overlay::Drawer, Overlay::Quick, Overlay::Recents];

pub struct ShellInner {
    pub layered: bool,
    pub drawer: Sheet,
    pub quick: Sheet,
    pub recents: Sheet,

    home_win: Option<gtk::ApplicationWindow>,
    home: RefCell<Option<gtk::Widget>>,
    home_visible: Cell<bool>,
    home_anim: RefCell<Option<gtk::TickCallbackId>>,
    bars: RefCell<Option<(gtk::Widget, gtk::Widget)>>,
    lock: RefCell<Option<lock::Lock>>,

    status: RefCell<Status>,
    status_listeners: RefCell<Vec<Box<dyn Fn(&Status)>>>,
    polling: Cell<bool>,

    wl: Option<wl::Client>,
    toplevels: RefCell<Vec<Toplevel>>,
    mru: RefCell<Vec<u32>>,
    previews: RefCell<HashMap<u32, PathBuf>>,
    toplevel_listeners: RefCell<Vec<Box<dyn Fn()>>>,
    capture_seq: Cell<u32>,

    pub auto_rotate: Cell<bool>,
    pub has_accel: Cell<bool>,
    orientation: RefCell<Option<String>>,

    /// which on-screen keyboard we use (fixed for the session)
    pub keyboard: Osk,
    /// on-screen keyboard process (squeekboard or wvkbd), if we started one
    pub osk: RefCell<Option<Child>>,
    osk_visible: Cell<bool>,
    osk_gen: Cell<u32>,
    inhibitor: RefCell<Option<Child>>,

    cfg: RefCell<Config>,
    cfg_gen: Cell<u32>,
    cfg_monitor: RefCell<Option<gio::FileMonitor>>,
    idle: RefCell<Option<Child>>,
    power_key_inhibitor: RefCell<Option<Child>>,
    blanked: Cell<bool>,
    saved_brightness: Cell<Option<f64>>,
    pk_down: Cell<bool>,
    pk_hold_fired: Cell<bool>,
    pk_gen: Cell<u32>,
    pk_pending: Cell<bool>,
    held_key: Cell<Option<u16>>,
    key_gen: Cell<u32>,
    /// volume as we last set it (fresher than the 5s status poll)
    volume_now: Cell<Option<f64>>,
    volume_listeners: RefCell<Vec<Box<dyn Fn(f64)>>>,
    pub osd: osd::Osd,
    /// [top bar, button bar]
    bar_windows: RefCell<Vec<gtk::ApplicationWindow>>,
    /// thin touch strips that bring auto-hidden bars back: [top, bottom]
    edge_windows: RefCell<Vec<gtk::ApplicationWindow>>,
    bars_poked: Cell<i64>,
    last_activate: Cell<i64>,
    kb_check_at: Cell<i64>,
    /// when an app was last launched from home/drawer (sway keeps home up until it shows)
    launched_at: Cell<i64>,
    refit_gen: Cell<u32>,
    kb_listeners: RefCell<Vec<Box<dyn Fn(bool)>>>,
    home_ui: RefCell<Option<home::Home>>,
    display_gen: Cell<u32>,
    tray: RefCell<Option<tray::Tray>>,
    tray_area: gtk::Box,
    tray_opener: RefCell<Option<Rc<dyn Fn(String)>>>,
    tx: mpsc::Sender<Event>,
}

pub type Shell = Rc<ShellInner>;

const PHONE_W: i32 = 420;
const PHONE_H: i32 = 860;

thread_local! {
    /// connector name of the monitor rusttouch lives on (multi-monitor)
    static SHELL_OUTPUT: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn monitors() -> Vec<gdk::Monitor> {
    let Some(d) = gdk::Display::default() else {
        return Vec::new();
    };
    let list = d.monitors();
    (0..list.n_items())
        .filter_map(|i| list.item(i).and_downcast::<gdk::Monitor>())
        .collect()
}

/// The monitor rusttouch's bars/home are on (or the first one).
pub fn shell_monitor() -> Option<gdk::Monitor> {
    let mons = monitors();
    let want = SHELL_OUTPUT.with(|s| s.borrow().clone());
    want.and_then(|w| {
        mons.iter()
            .find(|m| m.connector().as_deref() == Some(w.as_str()))
            .cloned()
    })
    .or_else(|| mons.first().cloned())
}

/// Connector name ("eDP-1") of the monitor rusttouch lives on.
pub fn shell_output_name() -> Option<String> {
    SHELL_OUTPUT
        .with(|s| s.borrow().clone())
        .or_else(|| shell_monitor().and_then(|m| m.connector().map(|c| c.to_string())))
}

/// (x, y, width, height) of rusttouch's monitor in logical pixels.
pub fn screen_size() -> (i32, i32, i32, i32) {
    match shell_monitor() {
        Some(m) => {
            let g = m.geometry();
            (g.x(), g.y(), g.width(), g.height())
        }
        None => (0, 0, PHONE_W, PHONE_H),
    }
}

pub fn build(app: &gtk::Application, cfg: Config) -> Shell {
    let layered = layer::supported();
    if !layered {
        eprintln!("rusttouch: layer-shell not available -> windowed (phone emulator) mode");
    }
    let (tx, rx) = mpsc::channel::<Event>();

    let layer_win = |title: &str, layer: Layer, zone: i32, kb: KeyboardMode| {
        layered.then(|| {
            let w = shell_window(app, title);
            layer::setup(&w, layer, Anchors::ALL, Zone::Fixed(zone), kb);
            w
        })
    };

    // Sheets sit on the Top layer between the bars (exclusive zone 0). Not
    // Overlay: the on-screen keyboard (Top layer, mapped later) must stay above them.
    let drawer = sheet::new(
        Dir::FromBottom,
        layer_win("rusttouch apps", Layer::Top, 0, KeyboardMode::Exclusive),
    );
    let quick = sheet::new(
        Dir::FromTop,
        layer_win(
            "rusttouch quick settings",
            Layer::Top,
            -1,
            KeyboardMode::Exclusive,
        ),
    );
    let recents = sheet::new(
        Dir::FromBottom,
        layer_win("rusttouch recents", Layer::Top, 0, KeyboardMode::None),
    );
    // Home sits on the Top layer *above apps* and is shown/hidden, like phosh.
    // Home: on sway it lives *below* the apps and shows whenever the screen's
    // workspace is empty (going home = switching to an empty workspace), so
    // the pull-down and app drawer always slide in over it. Elsewhere it sits
    // above the apps and is shown/hidden, like phosh.
    let home_on_sway = layered && crate::sway::available();
    let home_win = layer_win(
        "rusttouch home",
        if home_on_sway {
            Layer::Bottom
        } else {
            Layer::Top
        },
        0,
        KeyboardMode::None,
    );
    let lock_win = layer_win(
        "rusttouch lock",
        Layer::Overlay,
        -1,
        KeyboardMode::Exclusive,
    );

    let keyboard = if layered {
        sys::osk_kind(&cfg.get("keyboard_program"))
    } else {
        Osk::Missing
    };
    eprintln!(
        "rusttouch: on-screen keyboard: {}",
        match keyboard {
            Osk::Wvkbd => "wvkbd (auto-show via input-method)",
            Osk::Squeekboard => "squeekboard",
            Osk::Missing => "none (apt install wvkbd)",
        }
    );
    let wl = wl::start(tx.clone(), keyboard == Osk::Wvkbd);
    match &wl {
        Some(c) if !c.has_toplevels => {
            eprintln!("rusttouch: no wlr-foreign-toplevel - recents disabled")
        }
        None => eprintln!("rusttouch: no Wayland window list - recents disabled"),
        _ => {}
    }
    if keyboard == Osk::Wvkbd && !wl.as_ref().map(|c| c.has_input_method).unwrap_or(false) {
        eprintln!("rusttouch: compositor has no input-method-v2 - keyboard won't auto-show");
    }
    if layered {
        sensors::start_orientation(tx.clone());
        sensors::start_sleep_monitor(tx.clone());
    }

    let shell: Shell = Rc::new(ShellInner {
        layered,
        drawer,
        quick,
        recents,
        home_win,
        home: RefCell::new(None),
        home_visible: Cell::new(true),
        home_anim: RefCell::new(None),
        bars: RefCell::new(None),
        lock: RefCell::new(None),
        status: RefCell::new(Status::default()),
        status_listeners: RefCell::new(Vec::new()),
        polling: Cell::new(false),
        wl,
        toplevels: RefCell::new(Vec::new()),
        mru: RefCell::new(Vec::new()),
        previews: RefCell::new(HashMap::new()),
        toplevel_listeners: RefCell::new(Vec::new()),
        capture_seq: Cell::new(0),
        auto_rotate: Cell::new(cfg.on("auto_rotate")),
        has_accel: Cell::new(false),
        orientation: RefCell::new(None),
        keyboard,
        osk: RefCell::new(None),
        osk_visible: Cell::new(false),
        osk_gen: Cell::new(0),
        inhibitor: RefCell::new(None),
        cfg: RefCell::new(cfg),
        cfg_gen: Cell::new(0),
        cfg_monitor: RefCell::new(None),
        idle: RefCell::new(None),
        power_key_inhibitor: RefCell::new(None),
        blanked: Cell::new(false),
        saved_brightness: Cell::new(None),
        pk_down: Cell::new(false),
        pk_hold_fired: Cell::new(false),
        pk_gen: Cell::new(0),
        pk_pending: Cell::new(false),
        held_key: Cell::new(None),
        key_gen: Cell::new(0),
        volume_now: Cell::new(None),
        volume_listeners: RefCell::new(Vec::new()),
        osd: osd::Osd::new(app, layered),
        bar_windows: RefCell::new(Vec::new()),
        edge_windows: RefCell::new(Vec::new()),
        bars_poked: Cell::new(0),
        last_activate: Cell::new(0),
        kb_check_at: Cell::new(0),
        launched_at: Cell::new(0),
        refit_gen: Cell::new(0),
        kb_listeners: RefCell::new(Vec::new()),
        home_ui: RefCell::new(None),
        display_gen: Cell::new(0),
        tray: RefCell::new(None),
        tray_area: {
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            b.add_css_class("tray");
            b
        },
        tray_opener: RefCell::new(None),
        tx: tx.clone(),
    });
    SELF.with(|me| *me.borrow_mut() = Some(Rc::downgrade(&shell)));

    // tray first: quick settings lists its apps
    if layered {
        let t = tray::start(&shell.tray_area);
        let s = shell.clone();
        tray::on_tap(&t, move |id| s.open_tray_app(&id));
        *shell.tray.borrow_mut() = Some(t);
    }

    let (dc, dp) = drawer::build(&shell);
    shell.drawer.set_content(&dc, &dp);
    let (qc, qp) = quick::build(&shell);
    shell.quick.set_content(&qc, &qp);
    // the pull-down covers the top bar's space (the bar fades away as you
    // pull - everything on it is in the panel) and stops above the button bar
    if layered {
        shell.update_quick_margin();
    }
    let rc = recents::build(&shell);
    shell.recents.set_content(&rc, &rc);

    let top = bars::topbar(&shell);
    let nav = bars::navbar(&shell);
    let home_ui = home::build(&shell);
    let home = home_ui.root.clone();
    *shell.home_ui.borrow_mut() = Some(home_ui);
    *shell.bars.borrow_mut() = Some((
        top.clone().upcast::<gtk::Widget>(),
        nav.clone().upcast::<gtk::Widget>(),
    ));
    *shell.home.borrow_mut() = Some(home.clone().upcast::<gtk::Widget>());

    let lock = lock::build(&shell, lock_win);

    if layered {
        if let Some(w) = &shell.home_win {
            w.set_child(Some(&home));
            w.present();
        }

        let w = shell_window(app, "rusttouch top bar");
        layer::setup(&w, Layer::Top, Anchors::TOP, Zone::Auto, KeyboardMode::None);
        w.set_child(Some(&top));
        w.present();
        shell.bar_windows.borrow_mut().push(w);

        // Overlay layer: compositors lay out Overlay surfaces before Top ones,
        // so the button bar keeps the bottom edge and the keyboard (Top layer)
        // opens above it, like Android.
        let w = shell_window(app, "rusttouch nav bar");
        layer::setup(
            &w,
            Layer::Overlay,
            Anchors::BOTTOM,
            Zone::Auto,
            KeyboardMode::None,
        );
        w.set_child(Some(&nav));
        w.present();
        shell.bar_windows.borrow_mut().push(w);

        // auto-hide: any touch on a bar keeps it up a little longer
        for w in shell.bar_windows.borrow().iter() {
            let c = gtk::GestureClick::new();
            c.set_propagation_phase(gtk::PropagationPhase::Capture);
            let s = shell.clone();
            c.connect_pressed(move |_, _, _, _| s.poke_bars());
            w.add_controller(c);
        }
        for top in [true, false] {
            let w = edge_strip(&shell, app, top);
            shell.edge_windows.borrow_mut().push(w);
        }
        shell.apply_autohide();
    } else {
        let w = gtk::ApplicationWindow::builder()
            .application(app)
            .title("rusttouch")
            .default_width(PHONE_W)
            .default_height(PHONE_H)
            .build();
        // bars + (home under the sheets), lock on top of everything
        let middle = gtk::Overlay::new();
        middle.set_vexpand(true);
        middle.set_child(Some(&home));
        for s in [&shell.drawer, &shell.quick, &shell.recents] {
            if let Some(e) = s.embedded() {
                middle.add_overlay(&e);
            }
        }
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        vbox.append(&top);
        vbox.append(&middle);
        vbox.append(&nav);
        let outer = gtk::Overlay::new();
        outer.set_child(Some(&vbox));
        if let Some(e) = lock.embedded() {
            outer.add_overlay(&e);
        }
        w.set_child(Some(&outer));
        add_escape(&shell, &w);
        let a = app.clone();
        w.connect_close_request(move |_| {
            a.quit();
            glib::Propagation::Proceed
        });
        w.present();
    }
    *shell.lock.borrow_mut() = Some(lock);

    for s in [&shell.drawer, &shell.quick, &shell.recents] {
        if let Some(w) = s.window() {
            add_escape(&shell, w);
        }
    }

    shell.apply_cursor();

    if layered {
        let auto = shell.cfg().on("keyboard_auto");
        *shell.osk.borrow_mut() = match keyboard {
            Osk::Squeekboard => sys::start_squeekboard(auto),
            Osk::Wvkbd => None, // started below with the configured look
            Osk::Missing => None,
        };
        if keyboard == Osk::Wvkbd {
            shell.restart_keyboard();
        }
        *shell.inhibitor.borrow_mut() = sensors::inhibit_sleep();
        shell.restart_idle();
        let (_, has_power) = power::start_buttons(tx.clone());
        if has_power {
            *shell.power_key_inhibitor.borrow_mut() = power::inhibit_power_key();
        }
    }
    watch_config(&shell);
    crate::tiling::start(&shell);

    if layered {
        // monitors: apply saved settings now and whenever one is plugged in
        apply_displays(&shell);
        if let Some(d) = gdk::Display::default() {
            let s = shell.clone();
            d.monitors().connect_items_changed(move |_, _, _, _| {
                let gen = s.display_gen.get().wrapping_add(1);
                s.display_gen.set(gen);
                let s2 = s.clone();
                // let the compositor finish setting the new screen up first
                let _ = glib::timeout_add_local_once(Duration::from_millis(1200), move || {
                    if s2.display_gen.get() == gen {
                        apply_displays(&s2);
                    }
                });
            });
        }
        // the same login items GNOME starts (~/.config/autostart) - a moment
        // after the tray is up, since tray apps look for it when they start
        if std::env::var_os("RUSTTOUCH_NO_AUTOSTART").is_none() {
            let _ = glib::timeout_add_local_once(Duration::from_millis(1500), || {
                let ctx = gdk::Display::default().map(|d| d.app_launch_context());
                if let Some(c) = &ctx {
                    c.unsetenv("GDK_DEBUG");
                    c.unsetenv("GTK_A11Y");
                }
                let ctx = ctx.map(|c| c.upcast::<gio::AppLaunchContext>());
                crate::autostart::start_all(ctx.as_ref());
            });
        }
    }

    // first status read: create the config file, or restore Wi-Fi/Bluetooth
    let txs = tx.clone();
    std::thread::spawn(move || {
        let _ = txs.send(Event::Startup(sys::read_status()));
    });

    refresh_status(&shell);
    let s = shell.clone();
    let _ = glib::timeout_add_seconds_local(5, move || {
        refresh_status(&s);
        glib::ControlFlow::Continue
    });

    // keep the front app's recents preview fresh (cheap: half size, no compression)
    if layered {
        let s = shell.clone();
        let _ = glib::timeout_add_seconds_local(4, move || {
            let locked = s
                .lock
                .borrow()
                .as_ref()
                .map(|l| l.is_locked())
                .unwrap_or(false);
            if !locked && !s.blanked.get() && !s.osk_visible.get() {
                capture_then(&s, || {});
            }
            glib::ControlFlow::Continue
        });
    }

    // pump background events into the GTK thread
    let s = shell.clone();
    let _ = glib::timeout_add_local(Duration::from_millis(30), move || {
        while let Ok(ev) = rx.try_recv() {
            handle_event(&s, ev);
        }
        s.update_autohide();
        crate::tiling::refresh(&s, false);
        let at = s.kb_check_at.get();
        if at != 0 && glib::monotonic_time() >= at {
            s.kb_check_at.set(0);
            s.check_keyboard_started();
        }
        glib::ControlFlow::Continue
    });

    shell
}

fn handle_event(shell: &Shell, ev: Event) {
    match ev {
        Event::Toplevels(list) => shell.set_toplevels(list),
        Event::Orientation(o) => {
            shell.has_accel.set(true);
            if shell.auto_rotate.get() {
                sys::rotate_to(&o);
                // the grid has a different shape after rotating
                let s = shell.clone();
                let _ = glib::timeout_add_local_once(Duration::from_millis(700), move || {
                    s.refresh_home()
                });
            }
            *shell.orientation.borrow_mut() = Some(o);
        }
        Event::Sleep(true) => {
            shell.lock();
            // give the lock screen a few frames, then let the system sleep
            let s = shell.clone();
            let _ = glib::timeout_add_local_once(Duration::from_millis(400), move || {
                let c = s.inhibitor.borrow_mut().take();
                if let Some(c) = c {
                    sensors::release(c);
                }
            });
        }
        Event::Sleep(false) => {
            if shell.inhibitor.borrow().is_none() {
                *shell.inhibitor.borrow_mut() = sensors::inhibit_sleep();
            }
            shell.set_blank(false);
        }
        Event::TextInput(active) => text_input(shell, active),
        Event::ShellOutput(name) => {
            shell.pin_to(name);
            crate::screens::refresh(shell);
        }
        Event::Layout => crate::tiling::layout_changed(shell),
        Event::NewWindow(id) => crate::tiling::on_new_window(id),
        Event::Idle(what) => match what.as_str() {
            "blank" => {
                if shell.cfg().on("lock_on_screen_off") {
                    shell.lock();
                }
                shell.set_blank(true);
            }
            "unblank" => shell.set_blank(false),
            "sleep" => shell.run_power_action("sleep"),
            _ => {}
        },
        Event::Key(code, down) => match code {
            power::KEY_POWER => power_key(shell, down),
            power::KEY_VOLUMEUP => repeat_key(shell, code, down, 5.0),
            power::KEY_VOLUMEDOWN => repeat_key(shell, code, down, -5.0),
            power::KEY_BRIGHTNESSUP => repeat_key(shell, code, down, 5.0),
            power::KEY_BRIGHTNESSDOWN => repeat_key(shell, code, down, -5.0),
            power::KEY_MUTE if down => {
                sys::toggle_mute();
                shell.osd.show(Glyph::Volume, 0.0, "Mute");
            }
            _ => {}
        },
        Event::Startup(st) => {
            if !config::path().exists() {
                // first run: write the file with the current radio states
                let mut c = shell.cfg();
                if let Some(w) = st.wifi_on {
                    c.insert("wifi", if w { "on" } else { "off" });
                }
                if let Some(b) = st.bt_on {
                    c.insert("bluetooth", if b { "on" } else { "off" });
                }
                let _ = c.save();
                *shell.cfg.borrow_mut() = c;
            } else if shell.layered {
                let c = shell.cfg();
                if st.wifi_on.is_some() && st.wifi_on != Some(c.on("wifi")) {
                    sys::set_wifi(c.on("wifi"));
                }
                if st.bt_on.is_some() && st.bt_on != Some(c.on("bluetooth")) {
                    sys::set_bluetooth(c.on("bluetooth"));
                }
                refresh_soon(shell);
            }
        }
    }
}

// ---------------------------------------------------------------- monitors

/// Apply saved monitor settings (wlr-randr, on a thread), then move rusttouch's
/// surfaces to the right monitor.
fn apply_displays(shell: &Shell) {
    let cfg = shell.cfg();
    let auto_rotate = shell.auto_rotate.get();
    let tx = shell.tx.clone();
    std::thread::spawn(move || {
        crate::display::apply_saved(&cfg, auto_rotate);
        let outs = crate::display::list();
        let _ = tx.send(Event::ShellOutput(crate::display::shell_output(
            &cfg, &outs,
        )));
    });
}

// ---------------------------------------------------------------- keyboard

fn text_input(shell: &Shell, active: bool) {
    if !shell.cfg().on("keyboard_auto") {
        return;
    }
    // small delay before hiding, so moving between fields doesn't flicker
    let gen = shell.osk_gen.get().wrapping_add(1);
    shell.osk_gen.set(gen);
    let now = glib::monotonic_time();
    if active {
        shell.last_activate.set(now);
        shell.show_keyboard(true);
    } else if now - shell.last_activate.get() < 400_000 {
        // switching windows: the old window's "field closed" can arrive just
        // after the new window's "field opened" - don't let it hide the keyboard
    } else {
        let s = shell.clone();
        let _ = glib::timeout_add_local_once(Duration::from_millis(250), move || {
            if s.osk_gen.get() == gen {
                s.show_keyboard(false);
            }
        });
    }
}

// ---------------------------------------------------------------- volume / brightness keys

/// One step now, then repeat while held (after 350ms, every 90ms) - so you
/// can hold a volume button instead of tapping it over and over.
fn repeat_key(shell: &Shell, code: u16, down: bool, step: f64) {
    if !down {
        if shell.held_key.get() == Some(code) {
            shell.held_key.set(None);
        }
        return;
    }
    if shell.blanked.get() {
        return;
    }
    shell.held_key.set(Some(code));
    let gen = shell.key_gen.get().wrapping_add(1);
    shell.key_gen.set(gen);
    let volume = code == power::KEY_VOLUMEUP || code == power::KEY_VOLUMEDOWN;
    let apply = move |s: &Shell| {
        if volume {
            let v = (s.volume_level() + step).clamp(0.0, 100.0);
            s.volume_now.set(Some(v));
            sys::set_volume(v);
            for f in s.volume_listeners.borrow().iter() {
                f(v);
            }
            s.osd.show(Glyph::Volume, v / 100.0, "");
        } else {
            let b = (sys::brightness().unwrap_or(50.0) + step).clamp(3.0, 100.0);
            sys::set_brightness(b);
            s.osd.show(Glyph::Sun, b / 100.0, "");
        }
    };
    apply(shell);
    let s = shell.clone();
    let _ = glib::timeout_add_local_once(Duration::from_millis(350), move || {
        let s2 = s.clone();
        let _ = glib::timeout_add_local(Duration::from_millis(90), move || {
            if s2.held_key.get() != Some(code) || s2.key_gen.get() != gen {
                return glib::ControlFlow::Break;
            }
            apply(&s2);
            glib::ControlFlow::Continue
        });
    });
}

// ---------------------------------------------------------------- power key

const HOLD_MS: u64 = 700;
const DOUBLE_MS: u64 = 300;

fn power_key(shell: &Shell, down: bool) {
    if down {
        if shell.pk_down.replace(true) {
            return; // same press reported by two devices
        }
        // screen off: any press just wakes it (like a phone)
        if shell.blanked.get() {
            shell.set_blank(false);
            shell.pk_hold_fired.set(true);
            return;
        }
        shell.pk_hold_fired.set(false);
        let gen = shell.pk_gen.get().wrapping_add(1);
        shell.pk_gen.set(gen);
        let s = shell.clone();
        let _ = glib::timeout_add_local_once(Duration::from_millis(HOLD_MS), move || {
            if s.pk_down.get() && s.pk_gen.get() == gen && !s.pk_hold_fired.get() {
                s.pk_hold_fired.set(true);
                s.pk_pending.set(false);
                let a = s.cfg().get("power_button_hold");
                s.run_power_action(&a);
            }
        });
        return;
    }
    if !shell.pk_down.replace(false) || shell.pk_hold_fired.get() {
        return;
    }
    let cfg = shell.cfg();
    let double = cfg.get("power_button_double");
    if double == "nothing" {
        shell.run_power_action(&cfg.get("power_button"));
    } else if shell.pk_pending.replace(false) {
        shell.run_power_action(&double);
    } else {
        shell.pk_pending.set(true);
        let gen = shell.pk_gen.get();
        let s = shell.clone();
        let _ = glib::timeout_add_local_once(Duration::from_millis(DOUBLE_MS), move || {
            if s.pk_pending.replace(false) && s.pk_gen.get() == gen {
                let a = s.cfg().get("power_button");
                s.run_power_action(&a);
            }
        });
    }
}

// ---------------------------------------------------------------- config

/// Re-read rusttouch.conf whenever it changes (Settings app, a text editor, or
/// rusttouch itself) and apply what changed.
fn watch_config(shell: &Shell) {
    let _ = std::fs::create_dir_all(config::dir());
    let file = gio::File::for_path(config::path());
    let Ok(monitor) = file.monitor_file(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
    else {
        return;
    };
    let s = shell.clone();
    monitor.connect_changed(move |_, _, _, _| {
        // debounce: editors often write in several steps
        let gen = s.cfg_gen.get().wrapping_add(1);
        s.cfg_gen.set(gen);
        let s2 = s.clone();
        let _ = glib::timeout_add_local_once(Duration::from_millis(150), move || {
            if s2.cfg_gen.get() == gen {
                reload_config(&s2);
            }
        });
    });
    *shell.cfg_monitor.borrow_mut() = Some(monitor);
}

fn reload_config(shell: &Shell) {
    let new = Config::load();
    let old = shell.cfg.replace(new.clone());
    if old == new {
        return;
    }
    let changed = |k: &str| old.get(k) != new.get(k);

    if changed("time_24h") || changed("bar_clock") || changed("date_format") {
        crate::clock::set_prefs(&new);
    }
    if changed("top_bar_autohide") || changed("nav_bar_autohide") {
        shell.apply_autohide();
    }
    if changed("top_bar_size") || changed("nav_bar_size") {
        theme::apply(&new);
        // a mapped surface only grows; re-show the bars so they can shrink too
        for w in shell.bar_windows.borrow().iter() {
            w.set_visible(false);
            w.present();
        }
        shell.update_quick_margin();
        let s = shell.clone();
        let _ = glib::timeout_add_local_once(Duration::from_millis(300), move || s.refresh_home());
    }
    if changed("theme")
        || changed("accent")
        || changed("background")
        || changed("background_fit")
        || changed("lock_background")
        || changed("lock_background_fit")
    {
        theme::apply(&new);
        if changed("theme") {
            sys::set_color_scheme(config::palette(&new).dark);
        }
    }
    let status = shell.status();
    if changed("wifi") && status.wifi_on != Some(new.on("wifi")) {
        sys::set_wifi(new.on("wifi"));
        refresh_soon(shell);
    }
    if changed("bluetooth") && status.bt_on != Some(new.on("bluetooth")) {
        sys::set_bluetooth(new.on("bluetooth"));
        refresh_soon(shell);
    }
    if changed("auto_rotate") {
        shell.auto_rotate.set(new.on("auto_rotate"));
        if new.on("auto_rotate") {
            if let Some(o) = shell.orientation() {
                sys::rotate_to(&o);
            }
        }
    }
    if changed("keyboard_auto") && shell.keyboard == Osk::Squeekboard {
        sys::set_osk_auto(new.on("keyboard_auto"));
    }
    if changed("keyboard_height")
        || changed("keyboard_height_landscape")
        || changed("keyboard_font_size")
        || changed("keyboard_style")
        || changed("keyboard_rounding")
        || changed("theme")
        || changed("accent")
    {
        shell.restart_keyboard();
    }
    if changed("screen_off_after") || changed("sleep_after") {
        shell.restart_idle();
    }
    if changed("show_cursor") {
        shell.apply_cursor();
    }
    if changed("shell_output") || old.prefixed("output.") != new.prefixed("output.") {
        apply_displays(shell);
    }
    if changed("lock_message") {
        if let Some(l) = shell.lock.borrow().as_ref() {
            l.set_message(&new.get("lock_message"));
        }
    }
    if changed("home_apps") || changed("home_apps_landscape") {
        shell.refresh_home();
    }
}

/// A thin invisible strip along the top/bottom edge, shown while that bar
/// auto-hides: tap it to bring the bar back, or swipe from it like from the bar.
fn edge_strip(shell: &Shell, app: &gtk::Application, top: bool) -> gtk::ApplicationWindow {
    let w = shell_window(
        app,
        if top {
            "rusttouch top edge"
        } else {
            "rusttouch bottom edge"
        },
    );
    layer::setup(
        &w,
        if top { Layer::Top } else { Layer::Overlay },
        if top { Anchors::TOP } else { Anchors::BOTTOM },
        Zone::Fixed(0),
        KeyboardMode::None,
    );
    let strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    strip.add_css_class("edge-strip");
    let (s1, s2) = (shell.clone(), shell.clone());
    gestures::attach(
        &strip,
        gestures::Spec {
            pick: Box::new(move |_dx: f64, dy: f64| {
                if top && dy > 0.0 {
                    s1.target(Overlay::Quick, 1.0, 0.0)
                } else if !top && dy < 0.0 {
                    s1.target(Overlay::Drawer, -1.0, 0.0)
                } else {
                    None
                }
            }),
            tap: Some(Box::new(move || s2.poke_bars())),
        },
    );
    w.set_child(Some(&strip));
    w
}

pub fn shell_window(app: &gtk::Application, title: &str) -> gtk::ApplicationWindow {
    let w = gtk::ApplicationWindow::builder()
        .application(app)
        .title(title)
        .build();
    w.add_css_class("rusttouch-transparent");
    watch_pointer(&w);
    w
}

thread_local! {
    /// a real mouse/touchpad was used last (show_cursor = auto)
    static MOUSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// show_cursor = auto: show the pointer while a mouse is in use, hide it
/// again on touch.
fn watch_pointer(w: &gtk::ApplicationWindow) {
    let c = gtk::EventControllerLegacy::new();
    c.connect_event(|_, ev| {
        let mouse = match ev.event_type() {
            gdk::EventType::MotionNotify | gdk::EventType::ButtonPress | gdk::EventType::Scroll => {
                ev.device()
                    .map(|d| d.source() != gdk::InputSource::Touchscreen)
                    .unwrap_or(false)
            }
            gdk::EventType::TouchBegin => false,
            _ => return glib::Propagation::Proceed,
        };
        if MOUSE.with(|m| m.replace(mouse)) != mouse {
            set_cursor_all();
        }
        glib::Propagation::Proceed
    });
    w.add_controller(c);
    let w2 = w.clone();
    let _ = glib::idle_add_local_once(move || w2.set_cursor_from_name(cursor_name()));
}

fn cursor_name() -> Option<&'static str> {
    let cfg = Config::load();
    let mode = cfg.get("show_cursor");
    let show = mode == "on"
        || std::env::var_os("RUSTTOUCH_SHOW_CURSOR").is_some()
        || (mode != "off" && MOUSE.with(|m| m.get()));
    if show {
        None
    } else {
        Some("none")
    }
}

fn set_cursor_all() {
    if let Some(app) = gio::Application::default().and_downcast::<gtk::Application>() {
        let name = cursor_name();
        for w in app.windows() {
            w.set_cursor_from_name(name);
        }
    }
}

fn add_escape(shell: &Shell, w: &gtk::ApplicationWindow) {
    let k = gtk::EventControllerKey::new();
    let s = shell.clone();
    k.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape && s.any_open() {
            s.hide_all();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    w.add_controller(k);
}

impl ShellInner {
    pub fn sheet(&self, o: Overlay) -> &Sheet {
        match o {
            Overlay::Drawer => &self.drawer,
            Overlay::Quick => &self.quick,
            Overlay::Recents => &self.recents,
        }
    }

    /// Any sheet open or partly dragged in.
    pub fn any_showing(&self) -> bool {
        ALL.into_iter()
            .any(|o| self.sheet(o).is_open() || self.sheet(o).progress() > 0.001)
    }

    pub fn any_open(&self) -> bool {
        ALL.into_iter().any(|o| self.sheet(o).is_open())
    }

    pub fn show(&self, o: Overlay) {
        self.dismiss_keyboard();
        for other in ALL {
            if other != o {
                sheet::close(self.sheet(other));
            }
        }
        sheet::open(self.sheet(o));
    }

    pub fn toggle(&self, o: Overlay) {
        if self.sheet(o).is_open() {
            self.hide_all();
        } else {
            self.show(o);
        }
    }

    pub fn hide_all(&self) {
        for o in ALL {
            sheet::close(self.sheet(o));
        }
    }

    /// For gestures: which sheet a drag should move. Opening one closes the others.
    pub fn target(&self, o: Overlay, sign: f64, from: f64) -> Option<Target> {
        if from < 0.5 {
            self.dismiss_keyboard();
            for other in ALL {
                if other != o {
                    sheet::close(self.sheet(other));
                }
            }
        }
        Some(Target {
            sheet: self.sheet(o).clone(),
            sign,
            from,
        })
    }

    // ------------------------------------------------------------ config

    /// A copy of the current settings.
    pub fn cfg(&self) -> Config {
        self.cfg.borrow().clone()
    }

    /// Change one setting from the shell (quick settings toggles).
    pub fn save(&self, key: &str, value: &str) {
        self.cfg.borrow_mut().insert(key, value);
        config::set(key, value);
    }

    fn restart_idle(&self) {
        let old = self.idle.borrow_mut().take();
        if let Some(c) = old {
            power::stop(c);
        }
        if !self.layered {
            return;
        }
        let c = self.cfg();
        *self.idle.borrow_mut() = power::start_idle(
            self.tx.clone(),
            c.num("screen_off_after"),
            c.num("sleep_after"),
        );
    }

    fn apply_cursor(&self) {
        if self.layered {
            set_cursor_all();
        }
    }

    // ------------------------------------------------------------ monitors

    /// Put every rusttouch surface on monitor `name` (re-mapping visible ones).
    fn pin_to(&self, name: Option<String>) {
        let prev = SHELL_OUTPUT.with(|s| s.borrow().clone());
        // first call with a single monitor: everything is already there
        let same = prev == name || (prev.is_none() && monitors().len() <= 1);
        SHELL_OUTPUT.with(|s| *s.borrow_mut() = name.clone());
        let Some(m) = name.as_deref().and_then(|n| {
            monitors()
                .into_iter()
                .find(|m| m.connector().as_deref() == Some(n))
        }) else {
            return;
        };
        if same {
            return;
        }
        eprintln!(
            "rusttouch: using monitor {}",
            name.as_deref().unwrap_or("?")
        );
        let Some(app) = gio::Application::default().and_downcast::<gtk::Application>() else {
            return;
        };
        for w in app.windows() {
            if let Ok(aw) = w.downcast::<gtk::ApplicationWindow>() {
                layer::set_monitor(&aw, Some(&m));
                if aw.is_visible() {
                    aw.set_visible(false);
                    aw.present();
                }
            }
        }
        self.refresh_home();
    }

    // ------------------------------------------------------------ keyboard

    /// Run `f(volume)` when the volume buttons change it (keeps the slider in sync).
    pub fn on_volume(&self, f: impl Fn(f64) + 'static) {
        self.volume_listeners.borrow_mut().push(Box::new(f));
    }

    /// Current volume 0..100, including changes not yet seen by the status poll.
    pub fn volume_level(&self) -> f64 {
        self.volume_now
            .get()
            .or(self.status.borrow().volume)
            .unwrap_or(50.0)
    }

    /// (Re)start wvkbd with the keyboard settings (size, font, colors).
    pub fn restart_keyboard(&self) {
        if self.keyboard != Osk::Wvkbd {
            return;
        }
        let old = self.osk.borrow_mut().take();
        if let Some(mut c) = old {
            let _ = c.kill();
            let _ = c.wait();
        }
        let was_up = self.osk_visible.replace(false);
        let opts = sys::keyboard_args(&self.cfg());
        *self.osk.borrow_mut() = sys::start_wvkbd(&opts);
        // older wvkbd versions don't know every option: if it quits right
        // away, it's started again with just the basics (see the event pump)
        self.kb_check_at.set(glib::monotonic_time() + 700_000);
        if was_up {
            for f in self.kb_listeners.borrow().iter() {
                f(false);
            }
        }
    }

    /// Called a moment after starting wvkbd: fall back to plain options if it died.
    pub fn check_keyboard_started(&self) {
        let died = self
            .osk
            .borrow_mut()
            .as_mut()
            .map(|c| matches!(c.try_wait(), Ok(Some(_))))
            .unwrap_or(false);
        if died {
            eprintln!(
                "rusttouch: wvkbd rejected some keyboard options - starting it with defaults"
            );
            *self.osk.borrow_mut() = sys::start_wvkbd(&sys::keyboard_args_basic(&self.cfg()));
        }
    }

    /// Show auto-hidden bars for a few seconds.
    pub fn poke_bars(&self) {
        self.bars_poked.set(glib::monotonic_time());
        self.update_autohide();
    }

    /// Bars that auto-hide don't reserve space (apps get the whole screen);
    /// the edge strip is there instead to bring them back.
    fn apply_autohide(&self) {
        let cfg = self.cfg();
        let keys = ["top_bar_autohide", "nav_bar_autohide"];
        let bars = self.bar_windows.borrow();
        let edges = self.edge_windows.borrow();
        for (i, key) in keys.iter().enumerate() {
            let auto = cfg.on(key);
            if let Some(w) = bars.get(i) {
                layer::set_zone(w, if auto { Zone::Fixed(0) } else { Zone::Auto });
                // re-map so the compositor picks up the new zone
                w.set_visible(false);
                w.present();
            }
            if let Some(e) = edges.get(i) {
                e.set_visible(auto);
            }
        }
        drop((bars, edges));
        self.bars_poked.set(glib::monotonic_time());
    }

    /// Called often (event pump): hide/show auto-hiding bars.
    fn update_autohide(&self) {
        if !self.layered {
            return;
        }
        let cfg = self.cfg.borrow();
        let locked = self
            .lock
            .borrow()
            .as_ref()
            .map(|l| l.is_locked())
            .unwrap_or(false);
        let recently = glib::monotonic_time() - self.bars_poked.get() < 3_000_000;
        let keep = self.home_visible.get() || self.any_open() || locked || recently;
        let bars = self.bar_windows.borrow();
        for (i, key) in ["top_bar_autohide", "nav_bar_autohide"].iter().enumerate() {
            let Some(w) = bars.get(i) else { continue };
            // the lock screen has no buttons (the bar is above it, on the Overlay layer)
            let want = (!cfg.on(key) || keep) && !(i == 1 && locked);
            if want && !w.is_visible() {
                w.present();
            } else if !want && w.is_visible() {
                w.set_visible(false);
            }
        }
    }

    /// Keep the pull-down's bottom edge just above the button bar.
    pub fn update_quick_margin(&self) {
        if let Some(w) = self.quick.window() {
            let (_, nav) = theme::bar_sizes(&self.cfg());
            layer::set_margins(w, 0, 0, nav, 0);
        }
    }

    pub fn keyboard_up(&self) -> bool {
        self.osk_visible.get()
    }

    pub fn show_keyboard(&self, show: bool) {
        match self.keyboard {
            Osk::Wvkbd => {
                if let Some(c) = self.osk.borrow().as_ref() {
                    sys::wvkbd_show(c, show);
                }
            }
            Osk::Squeekboard => sys::squeekboard_show(show),
            Osk::Missing => return,
        }
        let was = self.osk_visible.replace(show);
        for f in self.kb_listeners.borrow().iter() {
            f(show);
        }
        if was != show {
            self.refit_apps();
        }
    }

    /// The keyboard reserves its height at the bottom of the screen (exclusive
    /// zone). sway resizes windows into the space left by itself; on phoc the
    /// front app is re-maximized so it ends above the keyboard instead of
    /// behind it - what you type into near the bottom stays visible.
    fn refit_apps(&self) {
        if !self.layered || crate::tiling::enabled(self) || !self.cfg().on("keyboard_resize_apps") {
            return;
        }
        let gen = self.refit_gen.get().wrapping_add(1);
        self.refit_gen.set(gen);
        let Some(shell) = SELF.with(|s| s.borrow().as_ref().and_then(|w| w.upgrade())) else {
            return;
        };
        // wait until the keyboard surface is (un)mapped and the space is updated
        let _ = glib::timeout_add_local_once(Duration::from_millis(280), move || {
            if shell.refit_gen.get() != gen {
                return;
            }
            let front: Vec<Toplevel> = shell
                .toplevels
                .borrow()
                .iter()
                .filter(|t| t.activated && !t.minimized)
                .cloned()
                .collect();
            if let Some(wl) = &shell.wl {
                for t in &front {
                    wl.refit(t);
                }
            }
        });
    }

    /// Run `f(visible)` whenever the on-screen keyboard shows/hides.
    pub fn on_keyboard(&self, f: impl Fn(bool) + 'static) {
        self.kb_listeners.borrow_mut().push(Box::new(f));
    }

    /// Hide the keyboard if it's up (taps on rusttouch's own surfaces).
    pub fn dismiss_keyboard(&self) {
        if self.osk_visible.get() {
            self.show_keyboard(false);
        }
    }

    pub fn tray(&self) -> Option<tray::Tray> {
        self.tray.borrow().clone()
    }

    pub fn tray_area(&self) -> Option<gtk::Box> {
        self.layered.then(|| self.tray_area.clone())
    }

    /// Quick settings registers how to show a tray app's page.
    pub fn set_tray_opener(&self, f: Rc<dyn Fn(String)>) {
        *self.tray_opener.borrow_mut() = Some(f);
    }

    /// Tap on a tray icon: open quick settings on that app's page.
    pub fn open_tray_app(&self, id: &str) {
        self.show(Overlay::Quick);
        let f = self.tray_opener.borrow().clone();
        if let Some(f) = f {
            f(id.to_string());
        }
    }

    pub fn home_ui(&self) -> Option<home::Home> {
        self.home_ui.borrow().clone()
    }

    pub fn refresh_home(&self) {
        if let Some(h) = self.home_ui() {
            home::render(&h);
        }
    }

    /// Drop any icon drag in progress; true if there was one.
    pub fn exit_home_edit(&self) -> bool {
        self.home_ui().map(|h| home::cancel(&h)).unwrap_or(false)
    }

    // ------------------------------------------------------------ screen / power

    /// Screen off/on. Uses wlr-output-power-management; falls back to
    /// turning the backlight down.
    pub fn set_blank(&self, blank: bool) {
        if self.blanked.replace(blank) == blank {
            return;
        }
        let ok = self
            .wl
            .as_ref()
            .map(|c| c.set_screens(!blank))
            .unwrap_or(false);
        if ok {
            return;
        }
        if blank {
            self.saved_brightness.set(sys::brightness());
            sys::set_brightness_raw(0);
        } else if let Some(b) = self.saved_brightness.take() {
            sys::set_brightness(b);
        }
    }

    /// Actions for the power button / idle timer (see config::POWER_ACTIONS).
    pub fn run_power_action(&self, action: &str) {
        match action {
            "sleep" => {
                self.lock();
                let _ = glib::timeout_add_local_once(Duration::from_millis(250), sys::suspend);
            }
            "lock" => {
                self.lock();
                self.set_blank(true);
            }
            "screen_off" => {
                if self.cfg().on("lock_on_screen_off") {
                    self.lock();
                }
                self.set_blank(true);
            }
            "menu" => {
                self.set_blank(false);
                self.show(Overlay::Quick);
            }
            "poweroff" => sys::poweroff(),
            _ => {}
        }
    }

    // ------------------------------------------------------------ home

    pub fn tx(&self) -> mpsc::Sender<Event> {
        self.tx.clone()
    }

    pub fn is_locked(&self) -> bool {
        self.lock
            .borrow()
            .as_ref()
            .map(|l| l.is_locked())
            .unwrap_or(false)
    }

    pub fn is_blanked(&self) -> bool {
        self.blanked.get()
    }

    pub fn home_visible(&self) -> bool {
        self.home_visible.get()
    }

    fn stop_home_anim(&self) {
        let id = self.home_anim.borrow_mut().take();
        if let Some(id) = id {
            id.remove();
        }
    }

    pub fn show_home(&self) {
        if !self.layered {
            return;
        }
        if crate::tiling::enabled(self) {
            // an empty workspace on the main screen = the home screen
            self.home_visible.set(true);
            let out = shell_output_name();
            if !crate::sway::showing_nothing(out.as_deref()) {
                let k = crate::sway::free_workspace();
                let focus = out
                    .map(|o| format!("focus output {o}; "))
                    .unwrap_or_default();
                crate::sway::run(&format!("{focus}workspace number {k}"));
            }
            if let Some(w) = &self.home_win {
                if !w.is_visible() {
                    w.present();
                }
            }
            return;
        }
        if self.home_visible.replace(true) {
            return;
        }
        let (Some(w), Some(h)) = (self.home_win.clone(), self.home.borrow().clone()) else {
            return;
        };
        self.stop_home_anim();
        h.set_opacity(0.0);
        w.present();
        let h2 = h.clone();
        let id = anim::run(&h, 170, move |e| h2.set_opacity(e), || {});
        *self.home_anim.borrow_mut() = Some(id);
    }

    pub fn hide_home(&self) {
        if !self.layered || !self.home_visible.replace(false) {
            return;
        }
        self.exit_home_edit();
        if crate::tiling::enabled(self) {
            // sway already shows the app's workspace (it got focus). Take the
            // home screen away too, so the gaps between split apps are black.
            if let Some(w) = &self.home_win {
                w.set_visible(false);
            }
            return;
        }
        let (Some(w), Some(h)) = (self.home_win.clone(), self.home.borrow().clone()) else {
            return;
        };
        self.stop_home_anim();
        let h2 = h.clone();
        let id = anim::run(
            &h,
            130,
            move |e| h2.set_opacity(1.0 - e),
            move || w.set_visible(false),
        );
        *self.home_anim.borrow_mut() = Some(id);
    }

    /// An app is about to be in front: get everything of ours out of the way.
    pub fn go_app(&self) {
        self.hide_all();
        self.hide_home();
    }

    /// Just before launching from home or the drawer.
    pub fn before_launch(&self) {
        if crate::tiling::enabled(self) {
            crate::tiling::prepare_launch(shell_output_name().as_deref());
        }
    }

    /// An app was just launched from home or the drawer.
    pub fn launching(&self) {
        if crate::tiling::enabled(self) {
            // give the new app a workspace of its own; home stays up until its
            // window shows (set_toplevels), so there's no empty screen meanwhile
            self.launched_at.set(glib::monotonic_time());
            self.hide_all();
            self.home_visible.set(true);
        } else {
            self.go_app();
        }
    }

    /// sway: the home screen is "visible" exactly when the main screen's
    /// workspace is empty - keep our flag in step (last window closed, etc.).
    pub fn sync_home(&self, empty: bool) {
        if self.home_visible.get() != empty {
            if !empty {
                self.exit_home_edit();
            }
            self.home_visible.set(empty);
        }
        if let Some(w) = &self.home_win {
            if empty && !w.is_visible() {
                w.present();
            } else if !empty && w.is_visible() && !self.launching_now() {
                w.set_visible(false);
            }
        }
    }

    fn launching_now(&self) -> bool {
        glib::monotonic_time() - self.launched_at.get() < 10_000_000 && self.home_visible.get()
    }

    // ------------------------------------------------------------ windows

    pub fn has_window_list(&self) -> bool {
        self.wl.as_ref().map(|c| c.has_toplevels).unwrap_or(false)
    }

    fn set_toplevels(&self, list: Vec<Toplevel>) {
        let prev = self.active_key();
        // the window that was last in front, even if focus has since moved to one of
        // our own sheets (the drawer and shade take the keyboard while open)
        let last_front = self.mru.borrow().first().copied();
        // mirrored screens (wl-mirror) aren't apps
        let list: Vec<Toplevel> = list
            .into_iter()
            .filter(|t| t.app_id != crate::tiling::MIRROR_APP)
            .collect();
        *self.toplevels.borrow_mut() = list;
        let now = self.active_key();
        {
            let tl = self.toplevels.borrow();
            let mut mru = self.mru.borrow_mut();
            if let Some(k) = now {
                mru.retain(|x| *x != k);
                mru.insert(0, k);
            }
            mru.retain(|k| tl.iter().any(|t| t.key == *k));
            for t in tl.iter() {
                if !mru.contains(&t.key) {
                    mru.push(t.key);
                }
            }
            self.previews.borrow_mut().retain(|k, path| {
                let keep = tl.iter().any(|t| t.key == *k);
                if !keep {
                    let _ = std::fs::remove_file(path);
                }
                keep
            });
        }
        // a (new) window came to the front -> hide home, like Android
        // Focus merely coming back to the window that was already last in front
        // (e.g. after the drawer closes) must not pull it over the home screen.
        let launching = glib::monotonic_time() - self.launched_at.get() < 10_000_000;
        let front_here = || {
            // on sway the window may have come up on another monitor
            !crate::tiling::enabled(self)
                || self
                    .toplevels
                    .borrow()
                    .iter()
                    .find(|t| Some(t.key) == now)
                    .map(crate::tiling::on_main_screen)
                    .unwrap_or(true)
        };
        if now.is_some() && now != prev && (now != last_front || launching) && front_here() {
            self.launched_at.set(0);
            self.hide_home();
        }
        if self.toplevels.borrow().is_empty() {
            self.show_home();
        }
        for f in self.toplevel_listeners.borrow().iter() {
            f();
        }
    }

    pub fn on_toplevels(&self, f: impl Fn() + 'static) {
        self.toplevel_listeners.borrow_mut().push(Box::new(f));
    }

    /// Most recently used first.
    pub fn toplevels_mru(&self) -> Vec<Toplevel> {
        let tl = self.toplevels.borrow();
        self.mru
            .borrow()
            .iter()
            .filter_map(|k| tl.iter().find(|t| t.key == *k).cloned())
            .collect()
    }

    fn active_key(&self) -> Option<u32> {
        self.toplevels
            .borrow()
            .iter()
            .find(|t| t.activated)
            .map(|t| t.key)
    }

    pub fn active_app_id(&self) -> Option<String> {
        self.toplevels
            .borrow()
            .iter()
            .find(|t| t.activated)
            .map(|t| t.app_id.clone())
    }

    pub fn preview(&self, key: u32) -> Option<PathBuf> {
        self.previews.borrow().get(&key).cloned()
    }

    pub fn activate(&self, t: &Toplevel) {
        if let Some(wl) = &self.wl {
            wl.activate(t);
        }
        self.go_app();
    }

    pub fn close_toplevel(&self, t: &Toplevel) {
        if let Some(wl) = &self.wl {
            wl.close(t);
        }
    }

    /// (top bar height, nav bar height)
    pub fn bar_heights(&self) -> (i32, i32) {
        match self.bars.borrow().as_ref() {
            Some((t, n)) => {
                let (dt, dn) = theme::bar_sizes(&self.cfg());
                (t.height().max(dt), n.height().max(dn))
            }
            None => (32, 56),
        }
    }

    // ------------------------------------------------------------ actions

    /// Android back button.
    pub fn back(&self) {
        let locked = self
            .lock
            .borrow()
            .as_ref()
            .map(|l| l.is_locked())
            .unwrap_or(false);
        if locked {
            return;
        }
        if crate::tiling::close_arrange() {
            return;
        }
        if self.any_open() {
            self.hide_all();
            return;
        }
        if self.exit_home_edit() {
            return;
        }
        if self.keyboard == Osk::Wvkbd && self.osk_visible.get() {
            self.show_keyboard(false);
            return;
        }
        if self.home_visible.get() {
            return;
        }
        sys::back(self.active_app_id(), self.keyboard == Osk::Squeekboard);
    }

    pub fn lock(&self) {
        self.hide_all();
        if let Some(l) = self.lock.borrow().as_ref() {
            l.show();
        }
    }

    pub fn orientation(&self) -> Option<String> {
        self.orientation.borrow().clone()
    }

    // ------------------------------------------------------------ status

    pub fn status(&self) -> Status {
        self.status.borrow().clone()
    }

    /// Run `f` now and on every status refresh.
    pub fn on_status(&self, f: impl Fn(&Status) + 'static) {
        f(&self.status.borrow());
        self.status_listeners.borrow_mut().push(Box::new(f));
    }
}

thread_local! {
    /// the shell, for methods that need to schedule work on themselves
    static SELF: RefCell<Option<std::rc::Weak<ShellInner>>> = const { RefCell::new(None) };
    /// why the last screenshot failed (shown on recents cards without a picture)
    static CAPTURE_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn capture_error() -> Option<String> {
    CAPTURE_ERROR.with(|e| e.borrow().clone())
}

/// Screenshot the app(s) in front (for their recents cards), then run `f`.
/// Skipped (f runs immediately) when home or a sheet is showing. On sway every
/// window of a split screen gets its own picture.
pub fn capture_then(shell: &Shell, f: impl FnOnce() + 'static) {
    if !shell.layered || shell.home_visible() || shell.any_open() || shell.is_locked() {
        f();
        return;
    }
    if !sys::which("grim") {
        CAPTURE_ERROR
            .with(|e| *e.borrow_mut() = Some("Previews need grim:\nsudo apt install grim".into()));
        f();
        return;
    }

    // (window key, "x,y wxh") for each window to capture
    let mut jobs: Vec<(u32, String)> = Vec::new();
    if crate::tiling::enabled(shell) {
        let tl = shell.toplevels.borrow().clone();
        for w in crate::tiling::visible_windows() {
            let key = tl
                .iter()
                .find(|t| t.app_id == w.app_id && t.title == w.title)
                .or_else(|| tl.iter().find(|t| t.app_id == w.app_id))
                .map(|t| t.key);
            if let Some(key) = key {
                let r = w.rect;
                jobs.push((
                    key,
                    format!("{},{} {}x{}", r.x, r.y, r.w.max(1), r.h.max(1)),
                ));
            }
        }
    }
    if jobs.is_empty() {
        // the app area: the whole screen minus our bars
        let key = shell
            .active_key()
            .or_else(|| shell.mru.borrow().first().copied());
        if let Some(key) = key {
            let (x, y, w, h) = screen_size();
            let (top, nav) = shell.bar_heights();
            jobs.push((
                key,
                format!("{},{} {}x{}", x, y + top, w, (h - top - nav).max(1)),
            ));
        }
    }
    if jobs.is_empty() {
        f();
        return;
    }

    let jobs: Vec<(u32, String, PathBuf)> = jobs
        .into_iter()
        .map(|(k, g)| {
            let seq = shell.capture_seq.get().wrapping_add(1);
            shell.capture_seq.set(seq);
            (k, g, sys::runtime_dir().join(format!("preview-{seq}.png")))
        })
        .collect();
    let shell = shell.clone();
    glib::spawn_future_local(async move {
        let work = jobs.clone();
        let results = gio::spawn_blocking(move || {
            work.iter()
                .map(|(_, g, p)| sys::grim(p, g))
                .collect::<Vec<Result<(), String>>>()
        })
        .await
        .unwrap_or_default();
        for ((key, _, path), res) in jobs.into_iter().zip(results) {
            match res {
                Ok(()) => {
                    if CAPTURE_ERROR.with(|e| e.borrow_mut().take()).is_some()
                        || shell.capture_seq.get() <= 1
                    {
                        eprintln!("rusttouch: recents previews working (grim)");
                    }
                    let old = shell.previews.borrow_mut().insert(key, path);
                    if let Some(old) = old {
                        let _ = std::fs::remove_file(old);
                    }
                }
                Err(e) => CAPTURE_ERROR.with(|c| *c.borrow_mut() = Some(e)),
            }
        }
        f();
    });
}

pub fn go_home(shell: &Shell) {
    crate::tiling::close_arrange();
    shell.exit_home_edit();
    shell.dismiss_keyboard();
    if shell.any_open() {
        shell.hide_all();
        shell.show_home();
        return;
    }
    let s = shell.clone();
    capture_then(shell, move || s.show_home());
}

pub fn toggle_recents(shell: &Shell) {
    crate::tiling::close_arrange();
    if shell.recents.is_open() {
        shell.hide_all();
        return;
    }
    let s = shell.clone();
    capture_then(shell, move || s.show(Overlay::Recents));
}

/// Poll system status on a worker thread and push it to listeners.
pub fn refresh_status(shell: &Shell) {
    if shell.polling.replace(true) {
        return; // already in flight
    }
    let shell = shell.clone();
    glib::spawn_future_local(async move {
        if let Ok(st) = gio::spawn_blocking(sys::read_status).await {
            if shell.held_key.get().is_none() {
                shell.volume_now.set(None);
            }
            *shell.status.borrow_mut() = st.clone();
            for f in shell.status_listeners.borrow().iter() {
                f(&st);
            }
        }
        shell.polling.set(false);
    });
}

/// Refresh a little later (after a toggle, give the system time to react).
pub fn refresh_soon(shell: &Shell) {
    let s = shell.clone();
    let _ = glib::timeout_add_local_once(Duration::from_millis(900), move || refresh_status(&s));
}

//! The home screen: the clock and Settings at the top, then rows of cards -
//! what's open now, your launchers ("Watch") and your apps.

use crate::config::Launcher;
use crate::launch;
use crate::sway;
use crate::{settings, ui, Tv};
use gio_unix::DesktopAppInfo;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;

pub struct HomeInner {
    tv: std::rc::Weak<crate::TvInner>,
    pub root: gtk::Box,
    running_row: gtk::Box,
    running: gtk::Box,
    watch: gtk::Box,
    apps: gtk::Box,
    settings_btn: gtk::Button,
    battery: crate::battery::Indicator,
    /// the app ids shown in the "Open now" row (redrawn only when it changes)
    shown: std::cell::RefCell<Vec<(i64, String, String)>>,
}

pub type Home = Rc<HomeInner>;

fn tv(h: &Home) -> Option<Tv> {
    h.tv.upgrade()
}

/// A row of cards with a title over it; returns (the whole row, where the
/// cards go).
fn card_row(title: &str) -> (gtk::Box, gtk::Box) {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    row.add_css_class("card-row");
    let t = gtk::Label::new(Some(title));
    t.add_css_class("row-title");
    t.set_halign(gtk::Align::Start);
    row.append(&t);
    let cards = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    cards.add_css_class("cards");
    let sc = gtk::ScrolledWindow::new();
    sc.set_vscrollbar_policy(gtk::PolicyType::Never);
    sc.set_hscrollbar_policy(gtk::PolicyType::External);
    sc.set_child(Some(&cards));
    // (scrolled by hand - see build: scrolling just to the card cut off
    // the outline around it)
    if let Some(vp) = sc.child().and_downcast::<gtk::Viewport>() {
        vp.set_scroll_to_focus(false);
    }
    row.append(&sc);
    (row, cards)
}

/// Scroll a row of cards so the focused card shows with room for its
/// outline (which reaches past the card's edges).
fn show_card(card: &gtk::Widget, cards: &gtk::Widget, sc: &gtk::ScrolledWindow) {
    const ROOM: f64 = 16.0;
    let Some(b) = card.compute_bounds(cards) else {
        return;
    };
    let adj = sc.hadjustment();
    let (left, right) = (b.x() as f64 - ROOM, (b.x() + b.width()) as f64 + ROOM);
    let (v, page) = (adj.value(), adj.page_size());
    if left < v {
        adj.set_value(left.max(0.0));
    } else if right > v + page {
        adj.set_value(right - page);
    }
}

fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

/// A big card: a name (and a smaller line under it).
fn card(name: &str, sub: &str, class: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("card");
    b.add_css_class(class);
    let v = gtk::Box::new(gtk::Orientation::Vertical, 6);
    v.set_valign(gtk::Align::Center);
    let n = gtk::Label::new(Some(name));
    n.add_css_class("card-name");
    n.set_wrap(true);
    n.set_justify(gtk::Justification::Center);
    n.set_max_width_chars(14);
    v.append(&n);
    if !sub.is_empty() {
        let s = gtk::Label::new(Some(sub));
        s.add_css_class("card-sub");
        s.set_ellipsize(gtk::pango::EllipsizeMode::End);
        s.set_max_width_chars(22);
        v.append(&s);
    }
    b.set_child(Some(&v));
    b
}

/// A launcher's card: the service's own icon (when found) on its colour,
/// with the name under it - or just the name, big.
fn launcher_card(l: &Launcher, sub: &str) -> gtk::Button {
    let Some(pic) = crate::icons::picture(l) else {
        let b = card(&l.name, sub, &format!("lc-{}", l.id));
        b.add_css_class("launcher");
        return b;
    };
    let b = gtk::Button::new();
    b.add_css_class("card");
    b.add_css_class("launcher");
    b.add_css_class("with-picture");
    b.add_css_class(&format!("lc-{}", l.id));
    let v = gtk::Box::new(gtk::Orientation::Vertical, 8);
    v.set_valign(gtk::Align::Center);
    v.set_halign(gtk::Align::Center);
    let p = gtk::Picture::for_filename(&pic);
    p.add_css_class("card-picture");
    p.set_content_fit(gtk::ContentFit::Contain);
    p.set_can_shrink(true);
    p.set_size_request(112, 112);
    p.set_overflow(gtk::Overflow::Hidden);
    v.append(&p);
    let n = gtk::Label::new(Some(&l.name));
    n.add_css_class("card-name-small");
    n.set_ellipsize(gtk::pango::EllipsizeMode::End);
    n.set_max_width_chars(18);
    v.append(&n);
    if !sub.is_empty() {
        let s = gtk::Label::new(Some(sub));
        s.add_css_class("card-sub");
        v.append(&s);
    }
    b.set_child(Some(&v));
    b
}

/// An app's card: its icon and name.
fn app_card(app: &DesktopAppInfo) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("card");
    b.add_css_class("app-card");
    let v = gtk::Box::new(gtk::Orientation::Vertical, 10);
    v.set_valign(gtk::Align::Center);
    let icon = match app.icon() {
        Some(i) => gtk::Image::from_gicon(&i),
        None => gtk::Image::from_icon_name("application-x-executable"),
    };
    icon.set_pixel_size(96);
    v.append(&icon);
    let n = gtk::Label::new(Some(&app.name()));
    n.add_css_class("card-name-small");
    n.set_ellipsize(gtk::pango::EllipsizeMode::End);
    n.set_max_width_chars(16);
    v.append(&n);
    b.set_child(Some(&v));
    b
}

pub fn build(t: &Tv) -> Home {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("home");

    // top: the clock, and Settings
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    top.add_css_class("top");
    let clock = gtk::Label::new(None);
    clock.add_css_class("clock");
    let date = gtk::Label::new(None);
    date.add_css_class("date");
    date.set_valign(gtk::Align::Baseline);
    clock.set_valign(gtk::Align::Baseline);
    top.append(&clock);
    top.append(&date);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    top.append(&spacer);
    let settings_btn = gtk::Button::with_label("⚙  Settings");
    settings_btn.add_css_class("top-button");
    {
        let t = t.clone();
        settings_btn.connect_clicked(move |_| settings::open(&t));
    }
    let battery = crate::battery::indicator();
    battery.root.set_margin_end(20);
    battery.root.set_visible(false);
    top.append(&battery.root);
    top.append(&settings_btn);
    root.append(&top);
    let tick = {
        let (clock, date) = (clock.clone(), date.clone());
        move || {
            if let Ok(now) = glib::DateTime::now_local() {
                if let Ok(s) = now.format("%H:%M") {
                    clock.set_text(&s);
                }
                if let Ok(s) = now.format("%A, %B %-d") {
                    date.set_text(&s);
                }
            }
        }
    };
    tick();
    glib::timeout_add_seconds_local(5, move || {
        tick();
        glib::ControlFlow::Continue
    });

    let rows = gtk::Box::new(gtk::Orientation::Vertical, 18);
    rows.add_css_class("rows");
    let (running_row, running) = card_row("Open now");
    let (watch_row, watch) = card_row("Watch");
    let (apps_row, apps) = card_row("Apps");
    rows.append(&running_row);
    rows.append(&watch_row);
    rows.append(&apps_row);
    let sc = gtk::ScrolledWindow::new();
    sc.set_hscrollbar_policy(gtk::PolicyType::Never);
    sc.set_vscrollbar_policy(gtk::PolicyType::External);
    sc.set_vexpand(true);
    sc.set_child(Some(&rows));
    root.append(&sc);
    // Up and down: show the whole row the focus is in, its title too.
    // (Scrolling just to the card left the row's title tucked under the
    // clock, only the tail of a "p" showing.)
    if let Some(vp) = sc.child().and_downcast::<gtk::Viewport>() {
        vp.set_scroll_to_focus(false);
    }
    {
        let (sc, rows) = (sc.clone(), rows.clone());
        t.win
            .connect_notify_local(Some("focus-widget"), move |w, _| {
                let Some(f) = gtk::prelude::RootExt::focus(w) else {
                    return;
                };
                if !f.is_ancestor(&rows) {
                    return;
                }
                // across: the card in its row (with room for its outline)
                let mut at = Some(f.clone());
                while let Some(x) = at {
                    if x.has_css_class("card") {
                        let cards = x.parent();
                        let hsc = x
                            .ancestor(gtk::ScrolledWindow::static_type())
                            .and_downcast::<gtk::ScrolledWindow>();
                        if let (Some(c), Some(hsc)) = (cards, hsc) {
                            show_card(&x, &c, &hsc);
                        }
                        break;
                    }
                    at = x.parent();
                }
                // up and down: the whole row
                let mut at = Some(f);
                while let Some(x) = at {
                    if x.has_css_class("card-row") {
                        let Some(b) = x.compute_bounds(&rows) else {
                            return;
                        };
                        let adj = sc.vadjustment();
                        let (top, bottom) = (b.y() as f64, (b.y() + b.height()) as f64);
                        let (v, page) = (adj.value(), adj.page_size());
                        if top < v {
                            adj.set_value(top);
                        } else if bottom > v + page {
                            adj.set_value((bottom - page).min(top));
                        }
                        return;
                    }
                    at = x.parent();
                }
            });
    }

    root.append(&ui::hints(&[
        ("OK", "Open"),
        ("Hold OK", "Options"),
        ("Home", "This screen"),
    ]));

    let h: Home = Rc::new(HomeInner {
        tv: Rc::downgrade(t),
        root,
        running_row,
        running,
        watch,
        apps,
        settings_btn,
        battery,
        // (never matches: the first look draws the row)
        shown: std::cell::RefCell::new(vec![(-1, String::new(), String::new())]),
    });
    rebuild(&h);
    refresh_running(&h);
    h
}

/// The battery at the top (hidden without one, or when turned off).
pub fn battery(h: &Home, s: Option<crate::battery::State>, show: bool, low: u32) {
    h.battery.update(s, show, low);
}

/// Focus the first card (or Settings when there is none).
pub fn focus_first(h: &Home) {
    let rows = [
        (&h.running, h.running_row.is_visible()),
        (&h.watch, true),
        (&h.apps, true),
    ];
    for (row, shown) in rows {
        if !shown {
            continue;
        }
        // (the first card - the "nothing open" note can't have the focus)
        let mut c = row.first_child();
        while let Some(w) = c {
            if w.is_focusable() {
                w.grab_focus();
                return;
            }
            c = w.next_sibling();
        }
    }
    h.settings_btn.grab_focus();
}

/// Card options for a launcher.
fn launcher_menu(t: &Tv, l: &Launcher) {
    let mut items: Vec<(String, Rc<dyn Fn(&Tv)>)> = Vec::new();
    let id = l.id.clone();
    let l2 = l.clone();
    items.push(("Open".into(), Rc::new(move |t: &Tv| t.open_launcher(&l2))));
    if let Some(w) = launch::launcher_window(l, &sway::windows()) {
        items.push(("Close".into(), Rc::new(move |_t: &Tv| launch::close(&w))));
    }
    let l3 = l.clone();
    items.push((
        "Open for sign-in".into(),
        Rc::new(move |t: &Tv| t.open_for_sign_in(&l3)),
    ));
    let i = id.clone();
    items.push((
        "Edit".into(),
        Rc::new(move |t: &Tv| settings::edit_launcher(t, &i)),
    ));
    let i = id.clone();
    items.push((
        "Move left".into(),
        Rc::new(move |t: &Tv| move_launcher(t, &i, -1)),
    ));
    let i = id.clone();
    items.push((
        "Move right".into(),
        Rc::new(move |t: &Tv| move_launcher(t, &i, 1)),
    ));
    let (i, name) = (id.clone(), l.name.clone());
    items.push((
        "Remove".into(),
        Rc::new(move |t: &Tv| {
            let i = i.clone();
            ui::confirm(t, &format!("Remove {name}?"), "Remove", move |t| {
                t.change(|c| c.launchers.retain(|l| l.id != i));
            });
        }),
    ));
    ui::actions(t, &l.name, items);
}

pub fn move_launcher(t: &Tv, id: &str, by: i32) {
    t.change(|c| {
        if let Some(i) = c.launchers.iter().position(|l| l.id == id) {
            let j = (i as i32 + by).clamp(0, c.launchers.len() as i32 - 1) as usize;
            c.launchers.swap(i, j);
        }
    });
    // keep the focus on the card that moved
    if let Some(h) = t.home.borrow().as_ref() {
        let idx = t.cfg.borrow().launchers.iter().position(|l| l.id == id);
        let mut c = h.watch.first_child();
        let mut n = 0;
        while let Some(w) = c {
            if Some(n) == idx {
                let _ = glib::idle_add_local_once(move || {
                    w.grab_focus();
                });
                break;
            }
            n += 1;
            c = w.next_sibling();
        }
    }
}

fn app_menu(t: &Tv, id: &str, name: &str) {
    let mut items: Vec<(String, Rc<dyn Fn(&Tv)>)> = Vec::new();
    let i = id.to_string();
    items.push(("Open".into(), Rc::new(move |t: &Tv| open_app(t, &i))));
    let i = id.to_string();
    items.push((
        "Move left".into(),
        Rc::new(move |t: &Tv| move_app(t, &i, -1)),
    ));
    let i = id.to_string();
    items.push((
        "Move right".into(),
        Rc::new(move |t: &Tv| move_app(t, &i, 1)),
    ));
    let i = id.to_string();
    items.push((
        "Remove".into(),
        Rc::new(move |t: &Tv| {
            let i = i.clone();
            t.change(|c| c.apps.retain(|a| *a != i));
        }),
    ));
    ui::actions(t, name, items);
}

fn move_app(t: &Tv, id: &str, by: i32) {
    t.change(|c| {
        if let Some(i) = c.apps.iter().position(|a| a == id) {
            let j = (i as i32 + by).clamp(0, c.apps.len() as i32 - 1) as usize;
            c.apps.swap(i, j);
        }
    });
}

/// "snap-store_snap-store.desktop" / "Org.Gnome.Nautilus" -> "snapstore..."
/// (lower case, letters and digits only)
fn plain(s: &str) -> String {
    s.trim_end_matches(".desktop")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The desktop file a snap's or a Flatpak's process comes from, from its
/// control group ("snap.snap-store.snap-store-....scope" ->
/// "snap-store_snap-store.desktop", "app-flatpak-org.foo.Bar-....scope" ->
/// "org.foo.Bar.desktop").
fn packaged_desktop_id(pid: i64) -> Option<String> {
    let cg = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    let last = cg.lines().last()?.rsplit('/').next()?.to_string();
    if let Some(rest) = last.strip_prefix("snap.") {
        // "<snap>.<app>-<uuid>.scope" (or "<snap>.<app>.service"):
        // drop "-..." pieces from the end until a desktop file is found
        let rest = rest.trim_end_matches(".scope").trim_end_matches(".service");
        let (name, app) = rest.split_once('.')?;
        let mut app = app.to_string();
        loop {
            let id = format!("{name}_{app}.desktop");
            if DesktopAppInfo::new(&id).is_some() {
                return Some(id);
            }
            match app.rsplit_once('-') {
                Some((a, _)) if !a.is_empty() => app = a.to_string(),
                _ => break,
            }
        }
        return Some(format!("{name}_{name}.desktop"));
    }
    if let Some(rest) = last.strip_prefix("app-flatpak-") {
        let id = rest.rsplit_once('-').map(|(a, _)| a).unwrap_or(rest);
        return Some(format!("{id}.desktop"));
    }
    None
}

/// The installed app a window belongs to: by its window name (the desktop
/// file's name or its StartupWMClass), by the snap or Flatpak it runs in,
/// or by a looser match of the names (snaps call their desktop files
/// "<snap>_<app>.desktop").
pub fn app_for_window(w: &sway::Window) -> Option<DesktopAppInfo> {
    let all: Vec<DesktopAppInfo> = gio::AppInfo::all()
        .into_iter()
        .filter_map(|a| a.downcast::<DesktopAppInfo>().ok())
        .collect();
    let exact = all.iter().find(|d| {
        d.id().is_some_and(|id| {
            id.trim_end_matches(".desktop")
                .eq_ignore_ascii_case(&w.app_id)
        }) || d
            .startup_wm_class()
            .is_some_and(|c| c.eq_ignore_ascii_case(&w.app_id))
    });
    if let Some(d) = exact {
        return Some(d.clone());
    }
    if let Some(id) = packaged_desktop_id(w.pid) {
        if let Some(d) = DesktopAppInfo::new(&id) {
            return Some(d);
        }
    }
    let want = plain(&w.app_id);
    if want.is_empty() {
        return None;
    }
    all.into_iter().find(|d| {
        let id = d.id().map(|i| i.to_string()).unwrap_or_default();
        // the last part of "org.gnome.Nautilus", either half of "snap_app"
        let parts = id
            .trim_end_matches(".desktop")
            .split(['.', '_'])
            .map(plain)
            .collect::<Vec<_>>();
        let exe = d
            .executable()
            .file_name()
            .map(|f| plain(&f.to_string_lossy()))
            .unwrap_or_default();
        parts.last().is_some_and(|p| *p == want) || parts.iter().any(|p| *p == want) || exe == want
    })
}

pub fn open_app(t: &Tv, id: &str) {
    let Some(app) = DesktopAppInfo::new(id) else {
        t.toast("That app isn't installed any more");
        return;
    };
    let open = sway::windows().into_iter().find(|w| {
        app_for_window(w)
            .and_then(|d| d.id())
            .is_some_and(|i| i.as_str() == id)
    });
    if let Some(w) = open {
        t.switch_to(&w);
        return;
    }
    match launch::app_command(&app) {
        Some(cmd) => {
            launch::close_others_when_new(t.cfg.borrow().max_apps);
            if launch::open(&cmd) {
                t.toast(&format!("Opening {}...", app.name()));
            } else {
                t.toast(&format!("Couldn't open {}", app.name()));
            }
        }
        None => t.toast(&format!("Couldn't open {}", app.name())),
    }
}

/// Redraw the launcher and app rows (after a change).
pub fn rebuild(h: &Home) {
    let Some(t) = tv(h) else { return };
    let had_focus = t
        .focused()
        .is_some_and(|f| f.is_ancestor(&h.watch) || f.is_ancestor(&h.apps));
    clear(&h.watch);
    let cfg = t.cfg();
    for l in &cfg.launchers {
        let sub = if l.url.is_empty() {
            "needs an address".to_string()
        } else {
            String::new()
        };
        let b = launcher_card(l, &sub);
        let (t2, l2) = (t.clone(), l.clone());
        b.connect_clicked(move |_| t2.open_launcher(&l2));
        let (t2, l2) = (t.clone(), l.clone());
        t.set_menu(&b, move || launcher_menu(&t2, &l2));
        h.watch.append(&b);
    }
    let add = card("+", "Add", "add-card");
    {
        let t2 = t.clone();
        add.connect_clicked(move |_| settings::add_launcher(&t2));
    }
    h.watch.append(&add);
    // (pictures still to find: looked for in the background, then redrawn)
    crate::icons::fetch_missing(&cfg.launchers);

    clear(&h.apps);
    for id in &cfg.apps {
        let Some(app) = DesktopAppInfo::new(id) else {
            continue;
        };
        let b = app_card(&app);
        let (t2, i) = (t.clone(), id.clone());
        b.connect_clicked(move |_| open_app(&t2, &i));
        let (t2, i, n) = (t.clone(), id.clone(), app.name().to_string());
        t.set_menu(&b, move || app_menu(&t2, &i, &n));
        h.apps.append(&b);
    }
    let add = card("+", "Add an app", "add-card");
    {
        let t2 = t.clone();
        add.connect_clicked(move |_| settings::add_app(&t2));
    }
    h.apps.append(&add);
    if had_focus {
        focus_first(h);
    }
}

/// Redraw the "Open now" row from the open windows (hidden when none).
pub fn refresh_running(h: &Home) {
    let Some(t) = tv(h) else { return };
    let wins = if sway::available() {
        sway::windows()
    } else {
        Vec::new()
    };
    let now: Vec<(i64, String, String)> = wins
        .iter()
        .map(|w| (w.id, w.app_id.clone(), w.title.clone()))
        .collect();
    if *h.shown.borrow() == now {
        return;
    }
    *h.shown.borrow_mut() = now;
    // (the focus is on a card of this row - or on nothing, on the home
    // screen: it goes to the first card after the redraw)
    let had_focus = t.focused().is_some_and(|f| f.is_ancestor(&h.running))
        || (t.focused().is_none() && t.pages.borrow().is_empty());
    clear(&h.running);
    let cfg = t.cfg();
    for w in wins {
        // a launcher's window: its name and colour
        let launcher = cfg
            .launchers
            .iter()
            .find(|l| launch::launcher_window(l, std::slice::from_ref(&w)).is_some());
        let b = match launcher {
            Some(l) => launcher_card(l, "open"),
            None => {
                // an installed app: its icon and name, like in Apps
                match app_for_window(&w) {
                    Some(d) => {
                        let b = app_card(&d);
                        b.add_css_class("running-card");
                        b
                    }
                    None => card(&w.app_id, &w.title, "running-card"),
                }
            }
        };
        b.add_css_class("running");
        // (a launcher's: through open_launcher, which asks to reload it if
        // it was edited since it opened)
        let (w2, t3, l3) = (w.clone(), t.clone(), launcher.cloned());
        b.connect_clicked(move |_| match &l3 {
            Some(l) => t3.open_launcher(l),
            None => t3.switch_to(&w2),
        });
        let (w2, title) = (w.clone(), w.title.clone());
        let t2 = t.clone();
        let l5 = launcher.cloned();
        t.set_menu(&b, move || {
            let (a, c, l4) = (w2.clone(), w2.clone(), l5.clone());
            ui::actions(
                &t2,
                &title,
                vec![
                    (
                        "Switch".into(),
                        Rc::new(move |t: &Tv| match &l4 {
                            Some(l) => t.open_launcher(l),
                            None => t.switch_to(&a),
                        }),
                    ),
                    ("Close".into(), Rc::new(move |_t: &Tv| launch::close(&c))),
                ],
            );
        });
        h.running.append(&b);
    }
    // nothing open: a note in its place (the row stays where it is)
    if h.running.first_child().is_none() {
        let note = gtk::Label::new(Some("Nothing open right now"));
        note.add_css_class("placeholder-card");
        h.running.append(&note);
    }
    if had_focus {
        focus_first(h);
    }
}

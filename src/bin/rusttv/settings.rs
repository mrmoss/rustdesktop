//! Settings, all reachable with the remote: launchers, apps on the home
//! screen, autologin, scale, auto suspend, updates, power.

use crate::config::{Launcher, BROWSERS, COLORS, PRESETS, SCALES};
use crate::{autologin, keyboard, launch, sway, ui, Tv};
use gio_unix::DesktopAppInfo;
use gtk::gio;
use gtk::prelude::*;
use std::rc::Rc;

fn scale_name(v: &str) -> String {
    SCALES
        .iter()
        .find(|s| s.0 == v)
        .map(|s| s.1.to_string())
        .unwrap_or_else(|| v.to_string())
}

fn autologin_text(s: &autologin::State) -> String {
    match s {
        autologin::State::On => "On".into(),
        autologin::State::Off => "Off".into(),
        autologin::State::OtherUser(u) => format!("Off ({u} has it)"),
        autologin::State::Unsupported => "Not available".into(),
    }
}

fn on_off(b: bool) -> &'static str {
    if b {
        "On"
    } else {
        "Off"
    }
}

fn warn_text(n: u32) -> String {
    if n == 0 {
        "Off".into()
    } else {
        format!("At {n}%")
    }
}

fn accent_name(c: &str) -> String {
    crate::config::ACCENTS
        .iter()
        .find(|a| a.0 == c)
        .map(|a| a.1.to_string())
        .unwrap_or_else(|| c.to_string())
}

fn background_name(c: &crate::config::Config) -> String {
    if c.background == "picture" {
        return std::path::Path::new(&c.wallpaper)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "Picture".into());
    }
    crate::config::BACKGROUNDS
        .iter()
        .find(|b| b.0 == c.background)
        .map(|b| b.1.to_string())
        .unwrap_or_else(|| "Dark".into())
}

/// Pictures to choose a background from: ~/Pictures (and folders in it)
/// and the system's wallpapers.
fn pictures() -> Vec<std::path::PathBuf> {
    fn walk(d: &std::path::Path, depth: u32, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        let mut items: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        items.sort();
        for p in items {
            if out.len() >= 300 {
                return;
            }
            let hidden = p
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'));
            if hidden {
                continue;
            }
            if p.is_dir() {
                if depth > 0 {
                    walk(&p, depth - 1, out);
                }
            } else if p
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .is_some_and(|e| matches!(e.as_str(), "jpg" | "jpeg" | "png" | "webp"))
            {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&crate::config::home().join("Pictures"), 2, &mut out);
    walk(std::path::Path::new("/usr/share/backgrounds"), 2, &mut out);
    out
}

fn pick_picture(tv: &Tv) {
    let pics = pictures();
    if pics.is_empty() {
        tv.toast("No pictures found - put some in your Pictures folder");
        return;
    }
    let home = crate::config::home();
    let opts = pics
        .iter()
        .map(|p| {
            let shown = p
                .strip_prefix(&home)
                .map(|r| format!("~/{}", r.to_string_lossy()))
                .unwrap_or_else(|_| p.to_string_lossy().to_string());
            (p.to_string_lossy().to_string(), shown)
        })
        .collect();
    let cur = tv.cfg().wallpaper;
    ui::choose(tv, "Background picture", opts, &cur, |t, v| {
        t.change(|c| {
            c.wallpaper = v;
            c.background = "picture".into();
        });
    });
}

fn max_apps_text(n: u32) -> String {
    if n == 0 {
        "No limit".into()
    } else {
        n.to_string()
    }
}

fn minutes_text(m: u32) -> String {
    match m {
        0 => "Off".into(),
        m if m % 60 == 0 => format!("{} h", m / 60),
        m => format!("{m} min"),
    }
}

pub fn open(tv: &Tv) {
    let body = ui::list();
    let cfg = tv.cfg();

    body.append(&ui::heading("Home screen"));
    let t = tv.clone();
    let (launchers, launchers_v) =
        ui::row("Launchers", &cfg.launchers.len().to_string(), move || {
            launchers_page(&t)
        });
    body.append(&launchers);
    let t = tv.clone();
    let (apps, apps_v) = ui::row("Apps", &cfg.apps.len().to_string(), move || apps_page(&t));
    body.append(&apps);
    let t = tv.clone();
    let (max, max_v) = ui::row("Max open apps", &max_apps_text(cfg.max_apps), move || {
        let cur = t.cfg().max_apps.to_string();
        let mut opts: Vec<(String, String)> =
            (1..=5).map(|n| (n.to_string(), n.to_string())).collect();
        opts.push(("0".into(), "No limit".into()));
        ui::choose(&t, "Max open apps", opts, &cur, |t, v| {
            let n = v.parse().unwrap_or(1);
            t.change(|c| c.max_apps = n);
        });
    });
    body.append(&max);
    let t = tv.clone();
    let (remote_b, _) = ui::row("Remote buttons", "", move || remote_page(&t));
    body.append(&remote_b);

    body.append(&ui::heading("Browser"));
    let t = tv.clone();
    let (browser, browser_v) = ui::row(
        "Default browser",
        ui::browser_name(&cfg.browser),
        move || {
            let cur = t.cfg().browser;
            ui::choose(&t, "Default browser", browser_options(), &cur, |t, v| {
                t.change(|c| c.browser = v);
            });
        },
    );
    body.append(&browser);
    let t = tv.clone();
    let (profile, profile_v) = ui::row("Separate history", on_off(!cfg.own_profile), move || {
        t.change(|c| c.own_profile = !c.own_profile);
    });
    body.append(&profile);

    body.append(&ui::heading("Screen"));
    let t = tv.clone();
    let (scale, scale_v) = ui::row("Scale", &scale_name(&cfg.scale), move || {
        let cur = t.cfg().scale;
        let opts = SCALES
            .iter()
            .map(|s| (s.0.to_string(), s.1.to_string()))
            .collect();
        ui::choose(&t, "Scale", opts, &cur, |t, v| {
            if v.is_empty() {
                t.toast("Automatic scale: from the next login");
            } else if sway::available() {
                sway::run(&format!("output * scale {v}"));
            }
            t.change(|c| c.scale = v);
        });
    });
    body.append(&scale);

    body.append(&ui::heading("Look"));
    let t = tv.clone();
    let (accent, accent_v) = ui::row("Accent color", &accent_name(&cfg.accent), move || {
        let cur = t.cfg().accent;
        let opts = crate::config::ACCENTS
            .iter()
            .map(|a| (a.0.to_string(), a.1.to_string()))
            .collect();
        ui::choose(&t, "Accent color", opts, &cur, |t, v| {
            t.change(|c| c.accent = v)
        });
    });
    body.append(&accent);
    let t = tv.clone();
    let (bg, bg_v) = ui::row("Background", &background_name(&cfg), move || {
        let cur = t.cfg().background;
        let opts = crate::config::BACKGROUNDS
            .iter()
            .map(|b| (b.0.to_string(), b.1.to_string()))
            .collect();
        ui::choose(&t, "Background", opts, &cur, |t, v| {
            if v == "picture" {
                pick_picture(t);
            } else {
                t.change(|c| c.background = v);
            }
        });
    });
    body.append(&bg);

    // (only with a battery)
    let mut battery_rows: Option<(gtk::Label, gtk::Label)> = None;
    if crate::battery::read().is_some() {
        body.append(&ui::heading("Battery"));
        let t = tv.clone();
        let (show, show_v) = ui::row("Battery icon", on_off(cfg.battery_show), move || {
            t.change(|c| c.battery_show = !c.battery_show);
            crate::battery::check(&t);
        });
        body.append(&show);
        let t = tv.clone();
        let (warn, warn_v) = ui::row(
            "Low battery warning",
            &warn_text(cfg.battery_warn),
            move || {
                let cur = t.cfg().battery_warn.to_string();
                let opts = [0u32, 5, 10, 15, 20, 25, 30]
                    .iter()
                    .map(|n| (n.to_string(), warn_text(*n)))
                    .collect();
                ui::choose(&t, "Low battery warning", opts, &cur, |t, v| {
                    let n = v.parse().unwrap_or(15);
                    t.change(|c| c.battery_warn = n);
                    crate::battery::check(t);
                });
            },
        );
        body.append(&warn);
        battery_rows = Some((show_v, warn_v));
    }

    body.append(&ui::heading("System"));
    let t = tv.clone();
    let (auto, auto_v) = ui::row(
        "Autologin",
        &autologin_text(&autologin::state()),
        move || toggle_autologin(&t),
    );
    body.append(&auto);
    let t = tv.clone();
    let (susp, susp_v) = ui::row("Auto suspend", &minutes_text(cfg.auto_suspend), move || {
        let cur = t.cfg().auto_suspend.to_string();
        let opts = [0u32, 15, 30, 60, 120, 240]
            .iter()
            .map(|m| (m.to_string(), minutes_text(*m)))
            .collect();
        ui::choose(&t, "Auto suspend", opts, &cur, |t, v| {
            let m: u32 = v.parse().unwrap_or(0);
            if !crate::idle::apply(m) {
                t.toast("Auto suspend needs swayidle: sudo apt install swayidle");
            }
            t.change(|c| c.auto_suspend = m);
        });
    });
    body.append(&susp);
    let t = tv.clone();
    let (upd, _) = ui::row("Updates", "", move || updates_page(&t));
    body.append(&upd);

    body.append(&ui::heading("Power"));
    for (label, what) in [
        ("Suspend", "suspend"),
        ("Restart", "reboot"),
        ("Shut down", "poweroff"),
        ("Log out", "logout"),
    ] {
        let t = tv.clone();
        let (b, _) = ui::row(label, "", move || {
            let what = what.to_string();
            ui::confirm(&t, &format!("{label}?"), label, move |_t| power(&what));
        });
        body.append(&b);
    }

    body.append(&ui::heading("About"));
    let t = tv.clone();
    let (about, _) = ui::row("About", env!("CARGO_PKG_VERSION"), move || about_page(&t));
    body.append(&about);

    let pg = ui::page("Settings", &body);
    // (the values, redrawn after a change on a page above this one)
    let t = tv.clone();
    let refresh: Rc<dyn Fn()> = Rc::new(move || {
        let c = t.cfg();
        launchers_v.set_text(&c.launchers.len().to_string());
        apps_v.set_text(&c.apps.len().to_string());
        max_v.set_text(&max_apps_text(c.max_apps));
        browser_v.set_text(ui::browser_name(&c.browser));
        profile_v.set_text(on_off(!c.own_profile));
        scale_v.set_text(&scale_name(&c.scale));
        accent_v.set_text(&accent_name(&c.accent));
        bg_v.set_text(&background_name(&c));
        if let Some((s, w)) = &battery_rows {
            s.set_text(on_off(c.battery_show));
            w.set_text(&warn_text(c.battery_warn));
        }
        auto_v.set_text(&autologin_text(&autologin::state()));
        susp_v.set_text(&minutes_text(c.auto_suspend));
    });
    tv.push_page(&pg, None, Some(refresh));
}

/// Updates: look for them, then install them.
fn updates_page(tv: &Tv) {
    let body = ui::list();
    let found: Rc<std::cell::RefCell<Option<crate::updates::Found>>> =
        Rc::new(std::cell::RefCell::new(None));
    let busy = Rc::new(std::cell::Cell::new(false));
    let count = |n: Option<usize>| match n {
        None => "-".to_string(),
        Some(0) => "Up to date".to_string(),
        Some(n) => n.to_string(),
    };
    let t = tv.clone();
    let (check, check_v) = ui::row("Check now", "", || {});
    body.append(&check);
    let (apt, apt_v) = ui::row("System", "-", || {});
    body.append(&apt);
    let (snap, snap_v) = ui::row("Snaps", "-", || {});
    body.append(&snap);
    let (flat, flat_v) = ui::row("Flatpaks", "-", || {});
    body.append(&flat);
    let (install, install_v) = ui::row("Install updates", "", || {});
    body.append(&install);
    let show = {
        let (found, check_v, apt_v, snap_v, flat_v) = (
            found.clone(),
            check_v.clone(),
            apt_v.clone(),
            snap_v.clone(),
            flat_v.clone(),
        );
        Rc::new(move |text: &str| {
            check_v.set_text(text);
            if let Some(f) = found.borrow().as_ref() {
                apt_v.set_text(&count(f.apt));
                snap_v.set_text(&count(f.snap));
                flat_v.set_text(&count(f.flatpak));
            }
        })
    };
    {
        let (found, busy, show) = (found.clone(), busy.clone(), show.clone());
        let run_check = Rc::new(move |t: &Tv| {
            if busy.replace(true) {
                return;
            }
            show("Checking...");
            let (found, busy, show) = (found.clone(), busy.clone(), show.clone());
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::updates::check());
            });
            let t = t.clone();
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
                let Ok(r) = rx.try_recv() else {
                    return gtk::glib::ControlFlow::Continue;
                };
                busy.set(false);
                match r {
                    Ok(f) => {
                        let n = f.total();
                        *found.borrow_mut() = Some(f);
                        show(if n == 0 {
                            "Up to date"
                        } else {
                            "Updates found"
                        });
                    }
                    Err(e) => {
                        show("Couldn't check");
                        t.toast(&format!("Couldn't check for updates: {e}"));
                    }
                }
                gtk::glib::ControlFlow::Break
            });
        });
        let rc = run_check.clone();
        let t2 = t.clone();
        check.connect_clicked(move |_| rc(&t2));
        // (looks as soon as the page opens)
        run_check(&t);
    }
    {
        let (busy, t2) = (busy.clone(), t.clone());
        install.connect_clicked(move |_| {
            if busy.get() {
                t2.toast("Wait a moment - still busy");
                return;
            }
            let (busy, iv) = (busy.clone(), install_v.clone());
            ui::confirm(&t2, "Install all updates?", "Install", move |t| {
                busy.set(true);
                iv.set_text("Installing...");
                t.toast("Installing updates - this can take a while");
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::updates::install());
                });
                let (busy, iv) = (busy.clone(), iv.clone());
                gtk::glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                    let Ok(r) = rx.try_recv() else {
                        return gtk::glib::ControlFlow::Continue;
                    };
                    busy.set(false);
                    crate::with_tv(|t| match r {
                        Ok(true) => {
                            iv.set_text("Done - restart to finish");
                            t.toast("Updates installed - restart to finish");
                        }
                        Ok(false) => {
                            iv.set_text("Done");
                            t.toast("Updates installed");
                        }
                        Err(e) => {
                            iv.set_text("Failed");
                            t.toast(&format!("Updating failed: {e}"));
                        }
                    });
                    gtk::glib::ControlFlow::Break
                });
            });
        });
    }
    let pg = ui::page("Updates", &body);
    tv.push(&pg, None);
}

fn power(what: &str) {
    let r = match what {
        "logout" => {
            if sway::available() {
                sway::run("exit");
            }
            return;
        }
        w => std::process::Command::new("systemctl").arg(w).spawn(),
    };
    if let Ok(mut c) = r {
        std::thread::spawn(move || {
            let _ = c.wait();
        });
    }
}

fn toggle_autologin(tv: &Tv) {
    let now = autologin::state();
    let on = match now {
        autologin::State::On => false,
        autologin::State::Unsupported => {
            tv.toast("Autologin works with the GDM, LightDM or SDDM login screens");
            return;
        }
        _ => true,
    };
    let go = move |t: &Tv| {
        t.toast(if on {
            "Turning on autologin..."
        } else {
            "Turning off autologin..."
        });
        std::thread::spawn(move || {
            let r = autologin::set(on);
            crate::on_main(move || {
                crate::with_tv(|t| {
                    match r {
                        Ok(()) => t.toast(if on {
                            "This TV now logs in by itself, straight into RustTV"
                        } else {
                            "Autologin is off"
                        }),
                        Err(e) => t.toast(&format!("Couldn't change it: {e}")),
                    }
                    t.change(|_| {});
                });
            });
        });
    };
    if on {
        ui::confirm(tv, "Turn on autologin?", "Turn on", go);
    } else {
        go(tv);
    }
}

fn browser_options() -> Vec<(String, String)> {
    BROWSERS
        .iter()
        .map(|b| {
            let label = if launch::installed(b.0) {
                b.1.to_string()
            } else {
                format!("{} (not installed)", b.1)
            };
            (b.0.to_string(), label)
        })
        .collect()
}

fn launcher(tv: &Tv, id: &str) -> Option<Launcher> {
    tv.cfg().launchers.into_iter().find(|l| l.id == id)
}

fn update(tv: &Tv, id: &str, f: impl FnOnce(&mut Launcher)) {
    tv.change(|c| {
        if let Some(l) = c.launchers.iter_mut().find(|l| l.id == id) {
            f(l);
        }
    });
}

/// A web address as typed: "frigate.local:8971" -> "https://frigate.local:8971"
/// (type http:// yourself for a server without https).
fn fix_url(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() || s.contains("://") {
        s.to_string()
    } else {
        format!("https://{s}")
    }
}

pub fn launchers_page(tv: &Tv) {
    let body = ui::list();
    let fill = {
        let (t, body) = (tv.clone(), body.clone());
        Rc::new(move || {
            while let Some(c) = body.first_child() {
                body.remove(&c);
            }
            let t2 = t.clone();
            let (add, _) = ui::row("Add launcher", "＋", move || add_launcher(&t2));
            body.append(&add);
            for l in t.cfg().launchers {
                let (t2, id) = (t.clone(), l.id.clone());
                let (b, _) = ui::row(&l.name, &l.url, move || edit_launcher(&t2, &id));
                body.append(&b);
            }
        })
    };
    fill();
    let pg = ui::page("Launchers", &body);
    let f = fill.clone();
    tv.push_page(&pg, None, Some(Rc::new(move || f())));
}

/// Pick a ready-made launcher (or make your own).
pub fn add_launcher(tv: &Tv) {
    let body = ui::list();
    let t = tv.clone();
    let (own, _) = ui::row("Custom", "", move || {
        keyboard::ask(&t, "Name", "", move |t, name| {
            if name.is_empty() {
                return;
            }
            keyboard::ask(
                t,
                &format!("Web address for {name}"),
                "https://",
                move |t, url| {
                    let url = fix_url(&url);
                    add(t, &name, &url, "#607d8b");
                },
            );
        });
    });
    body.append(&own);
    for p in PRESETS {
        let t = tv.clone();
        let sub = if p.url.is_empty() {
            "your own server"
        } else {
            p.url
        };
        let (b, _) = ui::row(p.name, sub, move || {
            if p.url.is_empty() {
                // your own server: ask where it is
                keyboard::ask(
                    &t,
                    &format!("Your {} server's address", p.name),
                    "https://",
                    move |t, url| {
                        let url = fix_url(&url);
                        if url.trim_end_matches('/').ends_with("://") {
                            return;
                        }
                        add(t, p.name, &url, p.color);
                    },
                );
            } else {
                add(&t, p.name, p.url, p.color);
            }
        });
        body.append(&b);
    }
    let pg = ui::page("Add a launcher", &body);
    tv.push(&pg, None);
}

fn add(tv: &Tv, name: &str, url: &str, color: &str) {
    let browser = tv.cfg().browser;
    let mut new_id = String::new();
    tv.change(|c| {
        new_id = c.new_id(name);
        c.launchers.push(crate::config::new_launcher(
            new_id.clone(),
            name,
            url,
            color,
            browser,
        ));
    });
    // (off the "add" page, back to where it was opened from)
    let on_add_page = tv.pages.borrow().len();
    if on_add_page > 0 {
        tv.back();
    }
    tv.toast(&format!("Added {name} to Watch"));
}

pub fn edit_launcher(tv: &Tv, id: &str) {
    let Some(l) = launcher(tv, id) else { return };
    let body = ui::list();
    let id = id.to_string();

    let (t, i) = (tv.clone(), id.clone());
    let (name, name_v) = ui::row("Name", &l.name, move || {
        let Some(l) = launcher(&t, &i) else { return };
        let i = i.clone();
        keyboard::ask(&t, "Name", &l.name, move |t, v| {
            if !v.is_empty() {
                update(t, &i, |l| l.name = v);
            }
        });
    });
    body.append(&name);
    let (t, i) = (tv.clone(), id.clone());
    let (url, url_v) = ui::row("Address", &l.url, move || {
        let Some(l) = launcher(&t, &i) else { return };
        let i = i.clone();
        keyboard::ask(&t, "Address", &l.url, move |t, v| {
            let v = fix_url(&v);
            update(t, &i, |l| l.url = v);
        });
    });
    body.append(&url);
    let (t, i) = (tv.clone(), id.clone());
    let (browser, browser_v) = ui::row("Browser", ui::browser_name(&l.browser), move || {
        let Some(l) = launcher(&t, &i) else { return };
        let i = i.clone();
        ui::choose(&t, "Browser", browser_options(), &l.browser, move |t, v| {
            update(t, &i, |l| l.browser = v);
        });
    });
    body.append(&browser);
    let (t, i) = (tv.clone(), id.clone());
    let color_name = |c: &str| {
        COLORS
            .iter()
            .find(|x| x.0 == c)
            .map(|x| x.1.to_string())
            .unwrap_or_else(|| c.to_string())
    };
    let (color, color_v) = ui::row("Color", &color_name(&l.color), move || {
        let Some(l) = launcher(&t, &i) else { return };
        let i = i.clone();
        let mut opts: Vec<(String, String)> = COLORS
            .iter()
            .map(|c| (c.0.to_string(), c.1.to_string()))
            .collect();
        if !opts.iter().any(|o| o.0 == l.color) {
            opts.insert(0, (l.color.clone(), format!("Its own ({})", l.color)));
        }
        ui::choose(&t, "Color", opts, &l.color, move |t, v| {
            update(t, &i, |l| l.color = v);
        });
    });
    body.append(&color);
    let on_off = |b: bool| if b { "On" } else { "Off" };
    let (t, i) = (tv.clone(), id.clone());
    let (pic, pic_v) = ui::row("Icon", on_off(l.picture), move || {
        update(&t, &i, |l| l.picture = !l.picture);
    });
    body.append(&pic);
    let (t, i) = (tv.clone(), id.clone());
    let (again, _) = ui::row("Reload icon", "", move || {
        let Some(l) = launcher(&t, &i) else { return };
        crate::icons::forget(&l);
        t.toast(&format!("Looking for {}'s icon...", l.name));
        t.change(|_| {});
    });
    body.append(&again);
    let (t, i) = (tv.clone(), id.clone());
    let (tvl, tv_v) = ui::row("Send TV headers", on_off(l.tv), move || {
        update(&t, &i, |l| l.tv = !l.tv);
        if let Some(l) = launcher(&t, &i) {
            if l.tv && t.cfg().own_profile {
                t.toast(&format!(
                    "{} gets its own history for this in this browser (use Chromium to keep yours)",
                    l.name
                ));
            } else if launch::launcher_window(&l, &sway::windows()).is_some() {
                t.toast(&format!("From the next time {} opens", l.name));
            }
        }
    });
    body.append(&tvl);
    let (t, i) = (tv.clone(), id.clone());
    let (spatial, spatial_v) = ui::row(
        "Spatial selecting (Chrome-based browsers only)",
        on_off(l.spatial),
        move || {
            // (turning it on in another browser moves the launcher to
            // Chromium for good - switching it on and off again then keeps
            // the same browser, and so the same sign-in)
            let Some(before) = launcher(&t, &i) else {
                return;
            };
            let chrome = ["chromium", "google-chrome"]
                .into_iter()
                .find(|b| launch::installed(b));
            if !before.spatial && !launch::is_chromium(&before.browser) && chrome.is_none() {
                t.toast("Spatial selecting needs Chromium or Chrome: sudo apt install chromium");
                return;
            }
            update(&t, &i, |l| {
                l.spatial = !l.spatial;
                if l.spatial && !launch::is_chromium(&l.browser) {
                    if let Some(b) = chrome {
                        l.browser = b.to_string();
                    }
                }
            });
            if let Some(l) = launcher(&t, &i) {
                if l.browser != before.browser {
                    t.toast(&format!(
                        "{} now opens in {} - sign in there once",
                        l.name,
                        ui::browser_name(&l.browser)
                    ));
                }
            }
        },
    );
    body.append(&spatial);
    let (t, i) = (tv.clone(), id.clone());
    let (trust, trust_v) = ui::row("Trust certificate", on_off(l.trust_cert), move || {
        update(&t, &i, |l| l.trust_cert = !l.trust_cert);
        let Some(l) = launcher(&t, &i) else { return };
        if !l.trust_cert {
            return;
        }
        if !l.url.starts_with("https://") {
            t.toast("Only for https:// addresses");
        } else if !launch::is_chromium(&launch::effective(&l).browser) {
            t.toast("Firefox and Waterfox remember \"Accept the risk\" by themselves");
        } else if launch::cert_fingerprint(&l.url).is_none() {
            t.toast(&format!(
                "Couldn't reach {} to read its certificate",
                l.name
            ));
        }
    });
    body.append(&trust);
    let (t, i) = (tv.clone(), id.clone());
    let (left, _) = ui::row("Move left", "", move || {
        crate::home::move_launcher(&t, &i, -1)
    });
    body.append(&left);
    let (t, i) = (tv.clone(), id.clone());
    let (right, _) = ui::row("Move right", "", move || {
        crate::home::move_launcher(&t, &i, 1)
    });
    body.append(&right);
    let (t, i) = (tv.clone(), id.clone());
    let (forget, _) = ui::row("Clear history", "", move || {
        let Some(l) = launcher(&t, &i) else { return };
        if launch::uses_own_profile(&l, t.cfg().own_profile) {
            t.toast(&format!(
                "{} uses your browser's own sign-ins - sign out on the site itself",
                l.name
            ));
            return;
        }
        let name = l.name.clone();
        ui::confirm(&t, &format!("Clear {name}'s history?"), "Clear", move |t| {
            if let Some(w) = launch::launcher_window(&l, &sway::windows()) {
                launch::close(&w);
            }
            launch::forget_profile(&l);
            t.toast(&format!("{} starts fresh next time", l.name));
        });
    });
    body.append(&forget);
    let (t, i) = (tv.clone(), id.clone());
    let (remove, _) = ui::row("Remove", "", move || {
        let i = i.clone();
        let name = launcher(&t, &i).map(|l| l.name).unwrap_or_default();
        ui::confirm(&t, &format!("Remove {name}?"), "Remove", move |t| {
            t.change(|c| c.launchers.retain(|l| l.id != i));
            t.back(); // (its page too)
        });
    });
    body.append(&remove);

    let pg = ui::page(&l.name, &body);
    let (t, i) = (tv.clone(), id.clone());
    let refresh: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(l) = launcher(&t, &i) {
            name_v.set_text(&l.name);
            url_v.set_text(&l.url);
            browser_v.set_text(ui::browser_name(&l.browser));
            color_v.set_text(&color_name(&l.color));
            pic_v.set_text(on_off(l.picture));
            tv_v.set_text(on_off(l.tv));
            spatial_v.set_text(on_off(l.spatial));
            trust_v.set_text(on_off(l.trust_cert));
        }
    });
    tv.push_page(&pg, None, Some(refresh));
}

pub fn apps_page(tv: &Tv) {
    let body = ui::list();
    let fill = {
        let (t, body) = (tv.clone(), body.clone());
        Rc::new(move || {
            while let Some(c) = body.first_child() {
                body.remove(&c);
            }
            let t2 = t.clone();
            let (add, _) = ui::row("Add app", "＋", move || add_app(&t2));
            body.append(&add);
            for id in t.cfg().apps {
                let name = DesktopAppInfo::new(&id)
                    .map(|a| a.name().to_string())
                    .unwrap_or_else(|| id.clone());
                let (t2, i, n) = (t.clone(), id.clone(), name.clone());
                let (b, _) = ui::row(&name, "Remove", move || {
                    let i = i.clone();
                    ui::confirm(&t2, &format!("Remove {n}?"), "Remove", move |t| {
                        t.change(|c| c.apps.retain(|a| *a != i));
                    });
                });
                body.append(&b);
            }
        })
    };
    fill();
    let pg = ui::page("Apps on the home screen", &body);
    let f = fill.clone();
    tv.push_page(&pg, None, Some(Rc::new(move || f())));
}

/// Pick an installed app to put on the home screen.
pub fn add_app(tv: &Tv) {
    let have = tv.cfg().apps;
    let mut all: Vec<gio::AppInfo> = gio::AppInfo::all()
        .into_iter()
        .filter(|a| a.should_show())
        .filter(|a| {
            a.id()
                .is_some_and(|id| !have.iter().any(|h| *h == id.as_str()))
        })
        .collect();
    all.sort_by_key(|a| a.name().to_lowercase());
    let options: Vec<(String, String)> = all
        .iter()
        .filter_map(|a| Some((a.id()?.to_string(), a.name().to_string())))
        .collect();
    if options.is_empty() {
        tv.toast("Every app is on the home screen already");
        return;
    }
    ui::choose(tv, "Add an app", options, "", |t, id| {
        let name = DesktopAppInfo::new(&id)
            .map(|a| a.name().to_string())
            .unwrap_or_default();
        t.change(|c| {
            if !c.apps.contains(&id) {
                c.apps.push(id);
            }
        });
        t.toast(&format!("Added {name} to Apps"));
    });
}

/// The buttons given to a job, for showing.
fn keys_text(tv: &Tv, action: &str) -> String {
    let mine: Vec<String> = tv
        .cfg()
        .keys
        .iter()
        .filter(|k| k.action == action)
        .map(|k| k.name.clone())
        .collect();
    let built = crate::remote::built_in(action);
    match (mine.is_empty(), built.is_empty()) {
        (true, _) => built.to_string(),
        (false, true) => mine.join(", "),
        (false, false) => format!("{} + {built}", mine.join(", ")),
    }
}

/// Give your remote's buttons jobs.
pub fn remote_page(tv: &Tv) {
    let body = ui::list();
    let mut values = Vec::new();
    let note = gtk::Label::new(Some(
        "Pick a job, then press the button on your remote for it. Buttons RustTV doesn't know yet work after this.",
    ));
    note.add_css_class("note");
    note.set_wrap(true);
    note.set_xalign(0.0);
    body.append(&note);
    for (action, label, everywhere) in crate::remote::ACTIONS {
        if *action == "home" {
            body.append(&ui::heading("In every app"));
        } else if *action == "ok" {
            body.append(&ui::heading("In RustTV"));
        }
        let _ = everywhere;
        let t = tv.clone();
        let (b, v) = ui::row(label, &keys_text(tv, action), move || {
            let a1 = action.to_string();
            let a2 = action.to_string();
            ui::actions(
                &t,
                label,
                vec![
                    ("Add button".into(), Rc::new(move |t: &Tv| learn(t, &a1))),
                    (
                        "Clear buttons".into(),
                        Rc::new(move |t: &Tv| set_keys(t, |k| k.retain(|k| k.action != a2))),
                    ),
                ],
            );
        });
        body.append(&b);
        values.push((action.to_string(), v));
    }
    let pg = ui::page("Remote buttons", &body);
    let t = tv.clone();
    let refresh: Rc<dyn Fn()> = Rc::new(move || {
        for (a, v) in &values {
            v.set_text(&keys_text(&t, a));
        }
    });
    tv.push_page(&pg, None, Some(refresh));
}

fn set_keys(tv: &Tv, f: impl FnOnce(&mut Vec<crate::config::RemoteKey>)) {
    let old = tv.cfg().keys;
    tv.change(|c| f(&mut c.keys));
    crate::remote::apply(&old, &tv.cfg().keys);
}

/// "Press the button for ...": the next key pressed gets the job (10
/// seconds; Back on the screen's list doesn't count - every key is taken).
fn learn(tv: &Tv, action: &str) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 24);
    root.add_css_class("page");
    root.set_valign(gtk::Align::Center);
    let t = gtk::Label::new(Some(&format!(
        "Press the button on your remote for:\n{}",
        crate::remote::label(action)
    )));
    t.add_css_class("page-title");
    t.set_justify(gtk::Justification::Center);
    root.append(&t);
    let hint = gtk::Label::new(Some("Wait 10 seconds to cancel"));
    hint.add_css_class("hint");
    root.append(&hint);
    // something to hold the focus (keys go to the page first)
    let holder = gtk::Button::with_label(" ");
    holder.add_css_class("invisible");
    root.append(&holder);

    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    crate::remote::learning(true);
    let (t2, d2) = (tv.clone(), done.clone());
    let a = action.to_string();
    let keys: Rc<dyn Fn(gtk::gdk::Key) -> bool> = Rc::new(move |key| {
        if d2.replace(true) {
            return true;
        }
        crate::remote::learning(false);
        t2.back();
        let Some(name) = crate::remote::key_name(key) else {
            t2.toast("RustTV can't use that button");
            return true;
        };
        use gtk::glib::translate::IntoGlib;
        let k = crate::config::RemoteKey {
            action: a.clone(),
            keyval: key.into_glib(),
            name: name.clone(),
        };
        // (a button has one job)
        set_keys(&t2, |ks| {
            ks.retain(|x| x.keyval != k.keyval);
            ks.push(k);
        });
        t2.toast(&format!("{name}: {}", crate::remote::label(&a)));
        true
    });
    tv.push(&root, Some(keys));
    let t3 = tv.clone();
    let _ = gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(10), move || {
        if !done.replace(true) {
            crate::remote::learning(false);
            t3.back();
            t3.toast("No button pressed - nothing changed");
        }
    });
}

/// About RustTV: the version, where things are kept, what it found.
fn about_page(tv: &Tv) {
    let body = ui::list();
    let info = |label: &str, value: &str| {
        let (b, _) = ui::row(label, value, || {});
        b
    };
    let intro = gtk::Label::new(Some(
        "RustTV is a home screen for a TV box, made for a remote control. \
         It's RustTouch's sibling: a separate program and login session.",
    ));
    intro.add_css_class("note");
    intro.set_wrap(true);
    intro.set_xalign(0.0);
    body.append(&intro);

    body.append(&ui::heading("This RustTV"));
    body.append(&info("Version", env!("CARGO_PKG_VERSION")));
    let user = autologin::user();
    let host = std::fs::read_to_string("/etc/hostname")
        .map(|h| h.trim().to_string())
        .unwrap_or_default();
    body.append(&info(
        "Signed in as",
        &format!(
            "{user}{}",
            if host.is_empty() {
                String::new()
            } else {
                format!(" on {host}")
            }
        ),
    ));
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                l.strip_prefix("PRETTY_NAME=")
                    .map(|v| v.trim_matches('"').to_string())
            })
        })
        .unwrap_or_default();
    if !os.is_empty() {
        body.append(&info("System", &os));
    }
    body.append(&info(
        "Window manager",
        if sway::available() {
            "sway"
        } else {
            "none (RustTV in a window)"
        },
    ));
    let cfg = tv.cfg();
    body.append(&info(
        "Launchers / apps",
        &format!("{} / {}", cfg.launchers.len(), cfg.apps.len()),
    ));

    body.append(&ui::heading("Browsers found"));
    for (id, name) in BROWSERS {
        let how = match launch::install(id) {
            Some(launch::Install::Program(p)) => p.to_string_lossy().to_string(),
            Some(launch::Install::Snap(_)) => "snap".into(),
            Some(launch::Install::Flatpak(app)) => format!("Flatpak ({app})"),
            None => "not installed".into(),
        };
        body.append(&info(name, &how));
    }

    body.append(&ui::heading("Where things are"));
    let home = crate::config::home();
    let short = |p: std::path::PathBuf| {
        let s = p.to_string_lossy().to_string();
        match s.strip_prefix(&*home.to_string_lossy()) {
            Some(rest) => format!("~{rest}"),
            None => s,
        }
    };
    body.append(&info(
        "Settings",
        &short(crate::config::dir().join("rusttv.json")),
    ));
    body.append(&info(
        "Your sway additions",
        &short(crate::config::dir().join("sway.conf")),
    ));
    let log = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|d| std::path::PathBuf::from(d).join("rusttv/sway.log"))
        .unwrap_or_else(|| home.join(".cache/rusttv-run/sway.log"));
    body.append(&info("Log (RustTV and sway)", &short(log)));
    body.append(&info("Help", "docs/RUSTTV.md in the RustTouch project"));
    body.append(&info("Licence", env!("CARGO_PKG_LICENSE")));

    let pg = ui::page("About RustTV", &body);
    tv.push(&pg, None);
}

//! Builds the CSS from the config (theme, accent, background) and (re)loads it.

use std::cell::RefCell;

use gtk::gdk;

use crate::config::{self, Config};

thread_local! {
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

pub fn apply(cfg: &Config) {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let dark = config::palette(cfg).dark;
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
    apply_cursor_blink(cfg);

    let css = format!(
        "{}\n{}\n{}\n{}",
        colors(cfg, dark),
        include_str!("style.css"),
        background(cfg),
        sizes(cfg)
    );

    PROVIDER.with(|p| {
        let mut p = p.borrow_mut();
        if p.is_none() {
            let provider = gtk::CssProvider::new();
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            // Optional user override: ~/.config/rusttouch/style.css
            let user_css = config::dir().join("style.css");
            if user_css.exists() {
                let user = gtk::CssProvider::new();
                user.load_from_path(&user_css);
                gtk::style_context_add_provider_for_display(
                    &display,
                    &user,
                    gtk::STYLE_PROVIDER_PRIORITY_USER,
                );
            }
            *p = Some(provider);
        }
        if let Some(provider) = p.as_ref() {
            provider.load_from_data(&css);
        }
    });
}

/// (blinks?, full on+off cycle in ms) for `cursor_blink`.
fn blink(cfg: &Config) -> (bool, i32) {
    match cfg.get("cursor_blink").as_str() {
        "off" => (false, 1200),
        "slow" => (true, 2000),
        "fast" => (true, 800),
        _ => (true, 1200), // GNOME's standard
    }
}

thread_local! {
    static BLINK_WATCH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Text cursor blinking, for RustTouch's own windows (lock screen, search)
/// and for apps: GTK apps, Firefox and many terminals read GNOME's
/// cursor-blink settings, so those are set too.
pub fn apply_cursor_blink(cfg: &Config) {
    let (on, ms) = blink(cfg);
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_cursor_blink(on);
        settings.set_gtk_cursor_blink_time(ms);
        // the desktop portal can push its own value later: keep ours
        if !BLINK_WATCH.with(|w| w.replace(true)) {
            settings.connect_gtk_cursor_blink_time_notify(|s| {
                let (_, ms) = blink(&Config::load());
                if s.gtk_cursor_blink_time() != ms {
                    s.set_gtk_cursor_blink_time(ms);
                }
            });
        }
    }
    let schema = "org.gnome.desktop.interface";
    for (key, value) in [
        ("cursor-blink", on.to_string()),
        ("cursor-blink-time", ms.to_string()),
    ] {
        let _ = std::process::Command::new("gsettings")
            .args(["set", schema, key, &value])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|mut c| std::thread::spawn(move || c.wait()));
    }
}

fn colors(cfg: &Config, _dark: bool) -> String {
    let accent = config::accent_hex(&cfg.get("accent"));
    let p = config::palette(cfg);
    let (fg, bar_bg, bar_fg, nav_bg, nav_fg, panel_bg, tile_bg, card_bg) = (
        p.fg, p.bar_bg, p.bar_fg, p.nav_bg, p.nav_fg, p.panel_bg, p.tile_bg, p.card_bg,
    );
    format!(
        "@define-color rd_fg {fg};\n\
         @define-color rd_bar_bg {bar_bg};\n\
         @define-color rd_bar_fg {bar_fg};\n\
         @define-color rd_nav_bg {nav_bg};\n\
         @define-color rd_nav_fg {nav_fg};\n\
         @define-color rd_panel_bg {panel_bg};\n\
         @define-color rd_tile_bg {tile_bg};\n\
         @define-color rd_card_bg {card_bg};\n\
         @define-color rd_accent {accent};\n\
         @define-color rd_accent_fg #10131a;\n"
    )
}

/// Bar heights from the config (applied after style.css, so they win).
pub fn bar_sizes(cfg: &Config) -> (i32, i32) {
    let top = cfg.get("top_bar_size").parse().unwrap_or(32).clamp(20, 64);
    let nav = cfg.get("nav_bar_size").parse().unwrap_or(56).clamp(28, 96);
    (top, nav)
}

fn sizes(cfg: &Config) -> String {
    let (top, nav) = bar_sizes(cfg);
    let top_font = (f64::from(top) * 0.44).round() as i32;
    let nav_font = (f64::from(nav) * 0.36).round() as i32;
    let btn_h = (nav - 8).max(24);
    let btn_w = (f64::from(nav) * 1.4).round() as i32;
    format!(
        ".topbar {{ min-height: {top}px; font-size: {top_font}px; }}\n\
         .navbar {{ min-height: {nav}px; }}\n\
         .navbar button {{ min-height: {btn_h}px; min-width: {btn_w}px; font-size: {nav_font}px; border-radius: {r}px; padding: 0; margin: 0; }}\n\
         .navbar button.nav-kb {{ min-width: {slot}px; font-size: {kb_font}px; }}\n\
         .nav-slot {{ min-width: {slot}px; }}\n",
        r = btn_h / 2,
        slot = nav,
        kb_font = (f64::from(nav) * 0.3).round() as i32
    )
}

/// Home and lock screen wallpapers: preset gradients or pictures.
fn background(cfg: &Config) -> String {
    format!(
        ".home {{ background: {}; }}\n.lock {{ background: {}; }}\n",
        config::background_css(cfg),
        config::lock_background_css(cfg)
    )
}

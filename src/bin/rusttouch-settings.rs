//! rusttouch-settings - a small companion settings app. Everything it shows is
//! stored in ~/.config/rusttouch/rusttouch.conf, which rusttouch watches and applies live.

#[path = "../autostart.rs"]
mod autostart;
#[path = "../config.rs"]
mod config;
#[path = "../display.rs"]
mod display;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use config::Config;

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("dev.rusttouch.Settings")
        .build();
    app.connect_activate(build);
    app.run()
}

const CSS: &str = "
.page { padding: 12px 14px 24px 14px; }
.group-title { font-weight: 700; font-size: 13px; opacity: 0.7; margin: 14px 6px 6px 6px; }
.group { border-radius: 16px; background: alpha(currentColor, 0.06); }
.row { padding: 12px 14px; min-height: 36px; }
.row-sub { font-size: 12px; opacity: 0.7; }
.nav-row { padding: 16px 14px; }
.swatch { min-width: 44px; min-height: 44px; border-radius: 22px; padding: 0; border: 3px solid transparent; }
.swatch.selected { border-color: currentColor; }
.bg-swatch { min-width: 92px; min-height: 140px; border-radius: 16px; padding: 0; border: 3px solid transparent; }
.bg-swatch.selected { border-color: currentColor; }
.bg-same { background: alpha(currentColor, 0.08); font-size: 12px; }
.note { font-size: 12px; opacity: 0.7; margin: 8px 6px; }
.settings-search { margin: 4px 2px 10px 2px; min-height: 40px; }
.close-btn { min-width: 24px; min-height: 24px; padding: 0; font-size: 11px; font-weight: 700; }
";

#[allow(deprecated)] // FileChooserNative: deprecated in GTK 4.10 but works fine
struct Ctx {
    window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    back: gtk::Button,
    title: gtk::Label,
    /// keeps a file chooser alive while it is open
    chooser: RefCell<Option<gtk::FileChooserNative>>,
}

fn build(app: &gtk::Application) {
    if let Some(w) = app.windows().first() {
        w.present();
        return;
    }
    let cfg = Config::load();
    apply_dark(&cfg);
    // The desktop's own color-scheme setting (via the settings portal) can
    // arrive a moment later and override us - keep RustTouch's choice.
    if let Some(s) = gtk::Settings::default() {
        s.connect_gtk_application_prefer_dark_theme_notify(|s| {
            let want = config::palette(&Config::load()).dark;
            if s.is_gtk_application_prefer_dark_theme() != want {
                s.set_gtk_application_prefer_dark_theme(want);
            }
        });
    }
    update_preview_css(&cfg);
    update_theme_css(&cfg);
    // follow changes (from here, RustTouch, or a text editor) live
    let file = gio::File::for_path(config::path());
    if let Ok(m) = file.monitor_file(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>) {
        m.connect_changed(|_, _, _, _| {
            let cfg = Config::load();
            apply_dark(&cfg);
            update_preview_css(&cfg);
            update_theme_css(&cfg);
        });
        std::mem::forget(m);
    }
    if let Some(display) = gdk::Display::default() {
        let provider = gtk::CssProvider::new();
        let mut css = CSS.to_string();
        for (name, hex) in config::ACCENTS {
            css.push_str(&format!(".accent-{name} {{ background: {hex}; }}\n"));
        }
        // the custom swatch: a color wheel look
        css.push_str(".accent-custom { background-image: linear-gradient(135deg, #f44, #fb3, #4d6, #4cf, #66f, #d4f); color: white; font-weight: 700; }\n");
        for (name, grad) in config::BACKGROUNDS {
            css.push_str(&format!(".bg-{name} {{ background: {grad}; }}\n"));
        }
        provider.load_from_data(&css);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Settings")
        .default_width(420)
        .default_height(820)
        .build();

    let header = gtk::HeaderBar::new();
    let back = gtk::Button::with_label("‹ Back");
    back.set_visible(false);
    header.pack_start(&back);
    // The desktop's own close button (round, like other apps' title bars),
    // only the close one. If the icon theme has no close icon it would be an
    // empty button: then our own small round ✕ instead.
    let has_icon = gdk::Display::default()
        .map(|d| gtk::IconTheme::for_display(&d).has_icon("window-close-symbolic"))
        .unwrap_or(false);
    header.set_decoration_layout(Some(":close"));
    header.set_show_title_buttons(has_icon);
    let close = gtk::Button::with_label("✕");
    close.add_css_class("close-btn");
    close.set_tooltip_text(Some("Close"));
    close.set_valign(gtk::Align::Center);
    if !has_icon {
        header.pack_end(&close);
    }
    let title = gtk::Label::new(Some("Settings"));
    title.add_css_class("title");
    header.set_title_widget(Some(&title));
    window.set_titlebar(Some(&header));
    let w = window.clone();
    close.connect_clicked(move |_| w.close());

    let stack = gtk::Stack::new();
    // quick crossfade: a sliding page costs a lot on slower GPUs
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(110);
    // (choice lists open over everything, see picker())
    let host = gtk::Overlay::new();
    host.set_child(Some(&stack));
    window.set_child(Some(&host));
    HOST.with(|h| *h.borrow_mut() = Some(host));

    let ctx = Rc::new(Ctx {
        window: window.clone(),
        stack: stack.clone(),
        back: back.clone(),
        title,
        chooser: RefCell::new(None),
    });

    stack.add_named(&main_page(&ctx), Some("main"));
    let c = ctx.clone();
    back.connect_clicked(move |_| go_main(&c));

    // RustTouch's ◀ sends Alt+Left (long-press: Escape); a keyboard works too
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let c = ctx.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let back = key == gdk::Key::Escape
            || key == gdk::Key::Back
            || (key == gdk::Key::Left && mods.contains(gdk::ModifierType::ALT_MASK));
        if back && c.back.is_visible() {
            go_main(&c);
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);

    window.present();
}

thread_local! {
    static THEME_CSS: gtk::CssProvider = {
        let p = gtk::CssProvider::new();
        if let Some(d) = gdk::Display::default() {
            // above the base CSS (same level as the previews, different selectors)
            gtk::style_context_add_provider_for_display(&d, &p, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
        }
        p
    };
}

/// Paint the settings app with the RustTouch theme (same colors and shapes as
/// the pull-down and the app drawer) instead of the stock GTK look.
fn update_theme_css(cfg: &Config) {
    let p = config::palette(cfg);
    let accent = config::accent_hex(&cfg.get("accent"));
    // panel colors may be see-through; lay them over a solid base
    let base = if p.dark { "#000000" } else { "#ffffff" };
    let css = format!(
        "@define-color rd_fg {fg};\n\
         @define-color rd_accent {accent};\n\
         window, window.background, headerbar, .page {{ background: linear-gradient({panel}, {panel}), {base}; color: @rd_fg; }}\n\
         headerbar {{ box-shadow: none; border: none; min-height: 52px; }}\n\
         headerbar .title {{ font-weight: 700; }}\n\
         button:not(.swatch):not(.bg-swatch):not(.picker-item):not(.picker-cancel):not(.close-btn):not(.close) {{ background: {tile}; color: @rd_fg; border: none; box-shadow: none; border-radius: 999px; padding: 6px 16px; }}\n\
         button:not(.swatch):not(.bg-swatch):not(.picker-item):not(.picker-cancel):not(.close-btn):not(.close):hover {{ background: alpha(@rd_fg, 0.16); }}\n\
         button:not(.swatch):not(.bg-swatch):not(.picker-item):not(.picker-cancel):not(.close-btn):not(.close):active, button:not(.swatch):not(.bg-swatch):not(.picker-item):not(.picker-cancel):not(.close-btn):not(.close):checked {{ background: @rd_accent; color: #10131a; }}\n\
         button.close-btn {{ background: alpha(@rd_fg, 0.1); color: @rd_fg; border: none; box-shadow: none; border-radius: 999px; padding: 0; margin: 0 4px; }}\n\
         button.close-btn:hover {{ background: alpha(@rd_fg, 0.18); }}\n\
         windowcontrols button {{ min-width: 24px; min-height: 24px; padding: 0; margin: 0 4px; border-radius: 999px; background: alpha(@rd_fg, 0.1); color: @rd_fg; border: none; box-shadow: none; }}\n\
         windowcontrols button:hover {{ background: alpha(@rd_fg, 0.18); }}\n\
         button.picker:not(.swatch) {{ background: alpha(@rd_fg, 0.08); min-height: 40px; padding: 4px 14px 4px 18px; }}\n\
         button.picker:not(.swatch):active {{ background: alpha(@rd_accent, 0.35); color: @rd_fg; }}\n\
         .picker-arrow {{ opacity: 0.6; font-size: 11px; }}\n\
         .picker-backdrop {{ background: alpha(black, 0.45); }}\n\
         .picker-card {{ background: linear-gradient({card}, {card}), {base}; color: @rd_fg; border-radius: 26px; padding: 18px 8px 10px 8px; box-shadow: 0 10px 34px alpha(black, 0.4); }}\n\
         .picker-title {{ font-weight: 700; font-size: 17px; margin: 0 16px 10px 16px; }}\n\
         button.picker-item {{ background: none; color: @rd_fg; border: none; box-shadow: none; border-radius: 16px; min-height: 52px; padding: 0 14px; }}\n\
         button.picker-item:hover {{ background: alpha(@rd_fg, 0.06); }}\n\
         button.picker-item:active {{ background: alpha(@rd_fg, 0.12); }}\n\
         button.picker-item.on {{ background: alpha(@rd_accent, 0.18); font-weight: 700; }}\n\
         .picker-radio {{ min-width: 20px; min-height: 20px; border-radius: 999px; border: 2px solid alpha(@rd_fg, 0.5); }}\n\
         button.picker-item.on .picker-radio {{ border: 6px solid @rd_accent; min-width: 12px; min-height: 12px; }}\n\
         button.picker-cancel {{ background: none; border: none; box-shadow: none; border-radius: 999px; padding: 6px 16px; margin: 8px 8px 0 0; font-weight: 700; color: @rd_accent; }}\n\
         button.swatch, button.bg-swatch {{ padding: 0; }}\n\
         .group {{ background: {tile}; border-radius: 22px; }}\n\
         list, listview, row, .group > * {{ background: transparent; color: @rd_fg; }}\n\
         list > row:hover {{ background: alpha(@rd_fg, 0.06); border-radius: 22px; }}\n\
         .group-title {{ color: @rd_fg; }}\n\
         switch {{ background: alpha(@rd_fg, 0.22); border: none; box-shadow: none; }}\n\
         switch:checked {{ background: @rd_accent; }}\n\
         switch slider {{ background: #ffffff; border: none; box-shadow: none; }}\n\
         scale trough {{ background: alpha(@rd_fg, 0.2); border-radius: 999px; min-height: 8px; }}\n\
         scale highlight {{ background: @rd_accent; border-radius: 999px; }}\n\
         scale slider {{ background: @rd_accent; border: none; box-shadow: none; min-width: 22px; min-height: 22px; }}\n\
         entry, spinbutton {{ background: alpha(@rd_fg, 0.08); color: @rd_fg; border: none; box-shadow: none; border-radius: 14px; caret-color: @rd_accent; }}\n\
         entry:focus-within, spinbutton:focus-within {{ box-shadow: inset 0 0 0 2px @rd_accent; }}\n\
         dropdown > button {{ background: alpha(@rd_fg, 0.08); min-height: 48px; padding: 6px 16px; }}\n\
         popover listview row, popover list row, popover modelbutton {{ min-height: 48px; padding: 4px 12px; }}\n\
         popover > contents, popover > arrow {{ background: {card}; color: @rd_fg; border: none; box-shadow: 0 4px 18px alpha(black, 0.35); border-radius: 18px; }}\n\
         popover row:selected, popover row:hover {{ background: alpha(@rd_accent, 0.35); border-radius: 12px; }}\n\
         check:checked, radio:checked {{ background: @rd_accent; color: #10131a; border-color: @rd_accent; }}\n\
         .bg-swatch.selected {{ border-color: @rd_accent; }}\n\
         .swatch.selected {{ border-color: transparent; box-shadow: 0 0 0 3px {base}, 0 0 0 6px @rd_fg; color: white; font-weight: 800; font-size: 20px; text-shadow: 0 1px 3px alpha(black, 0.7); }}\n\
         scrollbar slider {{ background: alpha(@rd_fg, 0.35); }}\n",
        fg = p.fg,
        panel = p.panel_bg,
        tile = p.tile_bg,
        card = p.card_bg,
        base = base,
    );
    THEME_CSS.with(|t| t.load_from_data(&css));
}

fn apply_dark(cfg: &Config) {
    if let Some(s) = gtk::Settings::default() {
        s.set_gtk_application_prefer_dark_theme(config::palette(cfg).dark);
    }
}

thread_local! {
    static PREVIEW_CSS: gtk::CssProvider = {
        let p = gtk::CssProvider::new();
        if let Some(d) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&d, &p, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
        }
        p
    };
}

/// Wallpaper and keyboard previews are styled from the config, live.
fn update_preview_css(cfg: &Config) {
    let (bg, key, special, press, text) = if cfg.get("keyboard_style") == "wvkbd" {
        // wvkbd's own default look
        (
            "202020".to_string(),
            "3a3a3a".to_string(),
            "2a2a2a".to_string(),
            "4f4f4f".to_string(),
            "ffffff".to_string(),
        )
    } else {
        config::keyboard_colors(cfg)
    };
    let height: i32 = cfg.get("keyboard_height").parse().unwrap_or(300);
    let radius: i32 = cfg.get("keyboard_rounding").parse().unwrap_or(0);
    let font: i32 = cfg.get("keyboard_font_size").parse().unwrap_or(18);
    let css = format!(
        ".bg-preview {{ min-width: 92px; min-height: 140px; border-radius: 16px; }}\n\
         .bg-preview-home {{ background: {wall}; }}\n\
         .bg-preview-lock {{ background: {lock}; }}\n\
         .kbp {{ background: #{bg}; min-height: {h}px; padding: 4px; border-radius: 12px; margin: 10px 14px; }}\n\
         .kbp-key {{ background: #{key}; color: #{text}; border-radius: {radius}px; font-size: {font}px; margin: 2px; }}\n\
         .kbp-sp {{ background: #{special}; }}\n\
         .kbp-press {{ background: #{press}; }}\n",
        wall = config::background_css(cfg),
        lock = config::lock_background_css(cfg),
        h = (height * 2 / 3).clamp(120, 300),
    );
    PREVIEW_CSS.with(|p| p.load_from_data(&css));
}

/// A mock keyboard drawn with the chosen size, shape and colors.
fn keyboard_preview() -> gtk::Widget {
    // the RustTouch keyboard's portrait page
    let rows = [
        "Esc Ctr ↑ ↓ ← → ' - / Tab",
        "1 2 3 4 5 6 7 8 9 0",
        "q w e r t y u i o p",
        "a s d f g h j k l",
        "⇧ z x c v b n m ⌫",
        "⌨→ Cmp , ␣ . Enter",
    ];
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vb.add_css_class("kbp");
    vb.set_homogeneous(true);
    for (r, row) in rows.iter().enumerate() {
        let hb = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        hb.set_vexpand(true);
        for k in row.split(' ') {
            let l = gtk::Label::new(Some(k));
            l.add_css_class("kbp-key");
            l.set_hexpand(true);
            if matches!(
                k,
                "⇧" | "⌫"
                    | "⌨→"
                    | "Enter"
                    | "Esc"
                    | "Ctr"
                    | "Tab"
                    | "Cmp"
                    | "↑"
                    | "↓"
                    | "←"
                    | "→"
            ) {
                l.add_css_class("kbp-sp");
            }
            if k == "␣" {
                l.set_width_chars(14);
            }
            if r == 3 && k == "g" {
                l.add_css_class("kbp-press"); // show the "pressed" color too
            }
            hb.append(&l);
        }
        vb.append(&hb);
    }
    vb.upcast()
}

fn go_main(ctx: &Ctx) {
    ctx.stack.set_visible_child_name("main");
    ctx.back.set_visible(false);
    ctx.title.set_text("Settings");
}

type PageFn = fn(&Rc<Ctx>) -> gtk::Widget;

const PAGES: &[(&str, &str, PageFn)] = &[
    ("Appearance", "Theme, accent color, background", appearance),
    (
        "Clock & bars",
        "Time and date format, bar sizes",
        clock_bars,
    ),
    (
        "Displays",
        "Main screen, resolution, rotation (others mirror it)",
        displays,
    ),
    ("Startup apps", "Programs to start when you log in", startup),
    ("Windows", "Split screen, side by side apps", windows),
    (
        "Power & screen",
        "Screen off, lock, sleep, power button",
        power,
    ),
    ("Keyboard", "On-screen keyboard", keyboard),
    ("Network", "Wi-Fi, Bluetooth", network),
    ("About", "Version, files", about),
];

/// What the search box finds: (setting, page it's on, extra words to match).
const SEARCH: &[(&str, &str, &str)] = &[
    (
        "Theme (dark, light...)",
        "Appearance",
        "colors colour dark mode light mode nord dracula gruvbox",
    ),
    ("Accent color", "Appearance", "colour highlight"),
    (
        "Home screen background",
        "Appearance",
        "wallpaper picture image gradient photo",
    ),
    (
        "Lock screen background",
        "Appearance",
        "wallpaper picture image gradient",
    ),
    (
        "Lock screen message",
        "Appearance",
        "text owner contact if found",
    ),
    ("12 or 24 hour time", "Clock & bars", "clock 24h am pm"),
    ("Date format", "Clock & bars", "day month year"),
    ("Top bar clock", "Clock & bars", "time status bar"),
    ("Top bar height", "Clock & bars", "size status bar"),
    (
        "Button bar height",
        "Clock & bars",
        "size nav navigation back home recents",
    ),
    (
        "Auto-hide the bars",
        "Clock & bars",
        "hide full screen immersive",
    ),
    ("Main screen", "Displays", "monitor primary output"),
    (
        "Mirror other screens",
        "Displays",
        "monitor projector hdmi external second clone",
    ),
    ("Resolution", "Displays", "mode monitor size"),
    ("Scale", "Displays", "zoom bigger smaller dpi hidpi"),
    (
        "Rotation",
        "Displays",
        "portrait landscape orientation turn",
    ),
    (
        "Auto-rotate",
        "Displays",
        "accelerometer orientation portrait landscape",
    ),
    ("Mouse pointer", "Displays", "cursor show hide"),
    (
        "Apps that start at login",
        "Startup apps",
        "autostart boot login rustsync",
    ),
    (
        "Split screen",
        "Windows",
        "side by side multitasking tiling window group",
    ),
    ("How new apps open", "Windows", "full screen tile tiling"),
    (
        "Split direction",
        "Windows",
        "side by side top bottom stacked",
    ),
    (
        "Compositor (sway, phoc, labwc)",
        "Windows",
        "window manager",
    ),
    (
        "Turn screen off after",
        "Power & screen",
        "idle timeout sleep blank display",
    ),
    (
        "Lock when the screen turns off",
        "Power & screen",
        "security password",
    ),
    ("Sleep after", "Power & screen", "suspend idle"),
    (
        "Power button",
        "Power & screen",
        "press hold double sleep shutdown",
    ),
    (
        "Show keyboard automatically",
        "Keyboard",
        "on-screen osk typing text",
    ),
    (
        "Make room for the keyboard",
        "Keyboard",
        "resize apps hidden covered",
    ),
    ("Keyboard size", "Keyboard", "height bigger smaller"),
    (
        "Key shape and colors",
        "Keyboard",
        "style rounding font label",
    ),
    ("Keyboard program", "Keyboard", "wvkbd squeekboard"),
    ("Wi-Fi", "Network", "wifi wireless internet password"),
    (
        "Text cursor blinking",
        "Keyboard",
        "caret blink speed fast slow",
    ),
    ("Bluetooth", "Network", "headphones speaker pair"),
    ("Version and log file", "About", "config file help"),
];

fn main_page(ctx: &Rc<Ctx>) -> gtk::Widget {
    // search box at the top of the main page
    let entry = gtk::SearchEntry::new();
    entry.set_placeholder_text(Some("Search settings"));
    entry.set_hexpand(true);
    entry.add_css_class("settings-search");
    // Not focused when the app opens (that would pop up the keyboard): the
    // window's focus is cleared once it's shown. Tapping it focuses it as
    // usual, and the keyboard comes up.
    ctx.window.connect_map(|w| {
        let w = w.clone();
        let _ = glib::idle_add_local_once(move || {
            gtk::prelude::GtkWindowExt::set_focus(&w, None::<&gtk::Widget>);
        });
    });

    let results = gtk::ListBox::new();
    results.set_selection_mode(gtk::SelectionMode::None);
    results.add_css_class("group");
    results.set_visible(false);
    let none = gtk::Label::new(Some("No settings match"));
    none.add_css_class("note");
    none.set_visible(false);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("group");
    for (i, (name, sub, _)) in PAGES.iter().enumerate() {
        let vb = gtk::Box::new(gtk::Orientation::Vertical, 2);
        vb.add_css_class("nav-row");
        let t = gtk::Label::new(Some(name));
        t.set_halign(gtk::Align::Start);
        let s = gtk::Label::new(Some(sub));
        s.set_halign(gtk::Align::Start);
        s.add_css_class("row-sub");
        vb.append(&t);
        vb.append(&s);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&vb));
        row.set_widget_name(&i.to_string());
        list.append(&row);
    }
    let c = ctx.clone();
    list.connect_row_activated(move |_, row| {
        let i: usize = row.widget_name().parse().unwrap_or(0);
        open_page(&c, i);
    });

    let (res, l, n) = (results.clone(), list.clone(), none.clone());
    entry.connect_search_changed(move |e| {
        while let Some(r) = res.first_child() {
            res.remove(&r);
        }
        let q = e.text().trim().to_lowercase();
        let searching = !q.is_empty();
        l.set_visible(!searching);
        res.set_visible(false);
        n.set_visible(false);
        if !searching {
            return;
        }
        let words: Vec<&str> = q.split_whitespace().collect();
        let mut found = 0;
        for (title, page_name, extra) in SEARCH {
            let hay = format!("{title} {page_name} {extra}").to_lowercase();
            if !words.iter().all(|w| hay.contains(w)) {
                continue;
            }
            let Some(i) = PAGES.iter().position(|p| p.0 == *page_name) else {
                continue;
            };
            let vb = gtk::Box::new(gtk::Orientation::Vertical, 2);
            vb.add_css_class("nav-row");
            let t = gtk::Label::new(Some(*title));
            t.set_halign(gtk::Align::Start);
            let s = gtk::Label::new(Some(*page_name));
            s.set_halign(gtk::Align::Start);
            s.add_css_class("row-sub");
            vb.append(&t);
            vb.append(&s);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&vb));
            row.set_widget_name(&i.to_string());
            res.append(&row);
            found += 1;
        }
        res.set_visible(found > 0);
        n.set_visible(found == 0);
    });
    let (c, e) = (ctx.clone(), entry.clone());
    results.connect_row_activated(move |_, row| {
        let i: usize = row.widget_name().parse().unwrap_or(0);
        e.set_text("");
        open_page(&c, i);
    });

    page(&[
        entry.upcast(),
        results.upcast(),
        none.upcast(),
        list.upcast(),
    ])
}

/// Pages are rebuilt each time they open, so they always show the current file.
fn open_page(ctx: &Rc<Ctx>, i: usize) {
    let Some((name, _, f)) = PAGES.get(i) else {
        return;
    };
    if let Some(old) = ctx.stack.child_by_name("page") {
        ctx.stack.remove(&old);
    }
    ctx.stack.add_named(&f(ctx), Some("page"));
    ctx.stack.set_visible_child_name("page");
    ctx.back.set_visible(true);
    ctx.title.set_text(name);
}

// ------------------------------------------------------------ building blocks

fn page(children: &[gtk::Widget]) -> gtk::Widget {
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vb.add_css_class("page");
    for c in children {
        vb.append(c);
    }
    gtk::ScrolledWindow::builder()
        .child(&vb)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build()
        .upcast()
}

fn group(title: &str, rows: &[gtk::Widget]) -> gtk::Widget {
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let t = gtk::Label::new(Some(title));
    t.set_halign(gtk::Align::Start);
    t.add_css_class("group-title");
    vb.append(&t);
    let g = gtk::Box::new(gtk::Orientation::Vertical, 0);
    g.add_css_class("group");
    for r in rows {
        g.append(r);
    }
    vb.append(&g);
    vb.upcast()
}

fn note(text: &str) -> gtk::Widget {
    let l = gtk::Label::new(Some(text));
    l.set_wrap(true);
    l.set_xalign(0.0);
    l.add_css_class("note");
    l.upcast()
}

fn row(title: &str, sub: Option<&str>, w: &impl IsA<gtk::Widget>) -> gtk::Widget {
    let hb = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    hb.add_css_class("row");
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 2);
    vb.set_hexpand(true);
    vb.set_valign(gtk::Align::Center);
    let t = gtk::Label::new(Some(title));
    t.set_halign(gtk::Align::Start);
    t.set_wrap(true);
    vb.append(&t);
    if let Some(sub) = sub {
        let s = gtk::Label::new(Some(sub));
        s.set_halign(gtk::Align::Start);
        s.set_wrap(true);
        s.add_css_class("row-sub");
        vb.append(&s);
    }
    hb.append(&vb);
    w.set_valign(gtk::Align::Center);
    hb.append(w);
    hb.upcast()
}

fn switch_row(cfg: &Config, key: &'static str, title: &str, sub: Option<&str>) -> gtk::Widget {
    let sw = gtk::Switch::new();
    sw.set_active(cfg.on(key));
    sw.connect_active_notify(move |s| config::set_on(key, s.is_active()));
    row(title, sub, &sw)
}

fn choice_row(
    cfg: &Config,
    key: &'static str,
    title: &str,
    sub: Option<&str>,
    options: &'static [(&'static str, &'static str)],
) -> gtk::Widget {
    let labels: Vec<String> = options.iter().map(|o| o.1.to_string()).collect();
    let current = cfg.get(key);
    let selected = options.iter().position(|o| o.0 == current);
    let (b, _) = picker(title, labels, selected, move |i| {
        if let Some((v, _)) = options.get(i) {
            config::set(key, v);
        }
    });
    row(title, sub, &b)
}

// ------------------------------------------------------------ pages

fn appearance(ctx: &Rc<Ctx>) -> gtk::Widget {
    let cfg = Config::load();

    // accent swatches
    let accents = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    accents.add_css_class("row");
    let current = cfg.get("accent");
    let mut buttons: Vec<gtk::Button> = Vec::new();
    for (name, _) in config::ACCENTS {
        let b = gtk::Button::new();
        b.add_css_class("swatch");
        b.add_css_class(&format!("accent-{name}"));
        b.set_tooltip_text(Some(name));
        if *name == current {
            b.add_css_class("selected");
            b.set_label("✓");
        }
        accents.append(&b);
        buttons.push(b);
    }
    // custom: pick any color
    let custom = gtk::Button::with_label("+");
    custom.add_css_class("swatch");
    custom.add_css_class("accent-custom");
    custom.set_tooltip_text(Some("Custom color"));
    if current.starts_with('#') {
        custom.add_css_class("selected");
        custom.set_label("✓");
    }
    accents.append(&custom);
    let named = buttons.clone();
    // the chosen color gets a ring around it and a ✓ (the custom one shows + again)
    {
        let (all, cu) = (buttons.clone(), custom.clone());
        for (i, b) in buttons.iter().enumerate() {
            let (all, cu) = (all.clone(), cu.clone());
            b.connect_clicked(move |_| {
                for (j, o) in all.iter().enumerate() {
                    if i == j {
                        o.add_css_class("selected");
                        o.set_label("✓");
                    } else {
                        o.remove_css_class("selected");
                        o.set_label("");
                    }
                }
                cu.remove_css_class("selected");
                cu.set_label("+");
                config::set("accent", config::ACCENTS[i].0);
            });
        }
    }
    {
        let (c2, w) = (custom.clone(), ctx.window.clone());
        custom.connect_clicked(move |_| {
            let dialog = gtk::ColorDialog::new();
            dialog.set_title("Accent color");
            dialog.set_with_alpha(false);
            let initial = gdk::RGBA::parse(config::accent_hex(&Config::load().get("accent"))).ok();
            let (c3, named) = (c2.clone(), named.clone());
            dialog.choose_rgba(
                Some(&w),
                initial.as_ref(),
                None::<&gtk::gio::Cancellable>,
                move |r| {
                    if let Ok(c) = r {
                        let hex = format!(
                            "#{:02x}{:02x}{:02x}",
                            (c.red() * 255.0).round() as u8,
                            (c.green() * 255.0).round() as u8,
                            (c.blue() * 255.0).round() as u8
                        );
                        config::set("accent", &hex);
                        for b in &named {
                            b.remove_css_class("selected");
                            b.set_label("");
                        }
                        c3.add_css_class("selected");
                        c3.set_label("✓");
                    }
                },
            );
        });
    }

    // lock screen message
    let msg = gtk::Entry::new();
    msg.set_text(&cfg.get("lock_message"));
    msg.set_placeholder_text(Some("Lock screen message, e.g. If found, call 555-0123"));
    msg.set_hexpand(true);
    let save = gtk::Button::with_label("Save");
    let m2 = msg.clone();
    save.connect_clicked(move |_| config::set("lock_message", m2.text().trim()));
    msg.connect_activate(|e| config::set("lock_message", e.text().trim()));
    let msg_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    msg_row.add_css_class("row");
    msg_row.append(&msg);
    msg_row.append(&save);

    let mut home_rows = wallpaper_rows(ctx, &cfg, "background", "background_fit", false);
    home_rows.insert(0, previews());
    let mut lock_rows = wallpaper_rows(ctx, &cfg, "lock_background", "lock_background_fit", true);
    lock_rows.push(msg_row.upcast());

    page(&[
        group("Theme", &[theme_row(&cfg)]),
        group("Accent color", &[accents.upcast()]),
        group("Home screen", &home_rows),
        group("Lock screen", &lock_rows),
    ])
}

/// Home and lock previews, the same shape as the swatches.
fn previews() -> gtk::Widget {
    let hb = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    hb.add_css_class("row");
    for (class, title) in [("bg-preview-home", "Home"), ("bg-preview-lock", "Lock")] {
        let vb = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let p = gtk::Box::new(gtk::Orientation::Vertical, 0);
        p.add_css_class("bg-preview");
        p.add_css_class(class);
        let l = gtk::Label::new(Some(title));
        l.add_css_class("row-sub");
        vb.append(&p);
        vb.append(&l);
        hb.append(&vb);
    }
    hb.upcast()
}

/// Swatches + "your own picture" + fit, for one wallpaper setting.
fn wallpaper_rows(
    ctx: &Rc<Ctx>,
    cfg: &Config,
    key: &'static str,
    fit_key: &'static str,
    same_option: bool,
) -> Vec<gtk::Widget> {
    let bgs = gtk::FlowBox::new();
    bgs.set_selection_mode(gtk::SelectionMode::None);
    bgs.set_max_children_per_line(6);
    bgs.set_min_children_per_line(3);
    bgs.add_css_class("row");
    let current = cfg.get(key);
    let mut names: Vec<&'static str> = Vec::new();
    let mut buttons: Vec<gtk::Button> = Vec::new();
    if same_option {
        let b = gtk::Button::with_label("Same as\nhome");
        b.add_css_class("bg-swatch");
        b.add_css_class("bg-same");
        if current.is_empty() || current == "same" {
            b.add_css_class("selected");
        }
        bgs.insert(&b, -1);
        buttons.push(b);
        names.push("same");
    }
    for (name, _) in config::BACKGROUNDS {
        let b = gtk::Button::new();
        b.add_css_class("bg-swatch");
        b.add_css_class(&format!("bg-{name}"));
        b.set_tooltip_text(Some(name));
        if *name == current {
            b.add_css_class("selected");
        }
        bgs.insert(&b, -1);
        buttons.push(b);
        names.push(name);
    }
    let names = Rc::new(names);
    select_on_click(&buttons, Rc::new(move |i| config::set(key, names[i])));

    let is_image = std::path::Path::new(&current).is_file();
    let pick = gtk::Button::with_label("Choose image…");
    let picked = gtk::Label::new(Some(if is_image { current.as_str() } else { "" }));
    picked.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    picked.add_css_class("row-sub");
    let (c, pl) = (ctx.clone(), picked.clone());
    pick.connect_clicked(move |_| choose_image(&c, &pl, key));

    vec![
        bgs.upcast(),
        row("Your own picture", None, &pick),
        picked.upcast(),
        choice_row(cfg, fit_key, "Picture fits", None, config::BG_FITS),
    ]
}

/// Theme picker: every built-in theme (colors for bars, panels, keyboard).
fn theme_row(cfg: &Config) -> gtk::Widget {
    let labels: Vec<String> = config::THEMES
        .iter()
        .map(|t| t.1.label.to_string())
        .collect();
    let current = cfg.get("theme");
    let sel = config::THEMES
        .iter()
        .position(|t| t.0 == current)
        .or(Some(0));
    dyn_choice(
        "Style",
        Some("The accent color applies on top"),
        &labels,
        sel,
        |i| {
            config::set("theme", config::THEMES[i].0);
        },
    )
}

thread_local! {
    /// covers the whole window: choice lists open on top of it
    static HOST: RefCell<Option<gtk::Overlay>> = const { RefCell::new(None) };
}

/// A choice: a button showing what's chosen. Tapping it opens the whole list
/// over the window (like Android's settings): big rows, the chosen one marked,
/// scroll with a finger, tap one to pick it. Tapping outside closes it.
/// Returns the button and the chosen index.
fn picker(
    title: &str,
    labels: Vec<String>,
    selected: Option<usize>,
    on_change: impl Fn(usize) + 'static,
) -> (gtk::Button, Rc<Cell<usize>>) {
    let sel = Rc::new(Cell::new(selected.unwrap_or(0)));
    let labels = Rc::new(labels);
    let button = gtk::Button::new();
    button.add_css_class("picker");
    let hb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let shown = gtk::Label::new(labels.get(sel.get()).map(String::as_str));
    shown.set_ellipsize(gtk::pango::EllipsizeMode::End);
    shown.set_max_width_chars(20);
    shown.set_hexpand(true);
    let arrow = gtk::Label::new(Some("▾"));
    arrow.add_css_class("picker-arrow");
    hb.append(&shown);
    hb.append(&arrow);
    button.set_child(Some(&hb));
    let on_change: Rc<dyn Fn(usize)> = Rc::new(on_change);
    let title = title.to_string();
    let (l, s) = (labels.clone(), sel.clone());
    button.connect_clicked(move |_| {
        open_picker(&title, &l, &s, &shown, &on_change);
    });
    (button, sel)
}

fn open_picker(
    title: &str,
    labels: &Rc<Vec<String>>,
    sel: &Rc<Cell<usize>>,
    shown: &gtk::Label,
    on_change: &Rc<dyn Fn(usize)>,
) {
    let Some(host) = HOST.with(|h| h.borrow().clone()) else {
        return;
    };
    // dimmed backdrop over the window, the list in a card in the middle
    let backdrop = gtk::Box::new(gtk::Orientation::Vertical, 0);
    backdrop.add_css_class("picker-backdrop");
    backdrop.set_hexpand(true);
    backdrop.set_vexpand(true);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 2);
    card.add_css_class("picker-card");
    card.set_halign(gtk::Align::Center);
    card.set_valign(gtk::Align::Center);
    card.set_vexpand(true);
    let width = host.width().max(300);
    let height = host.height().max(400);
    card.set_size_request((width - 40).min(380), -1);
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("picker-title");
    heading.set_xalign(0.0);
    card.append(&heading);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height((height as f64 * 0.65) as i32);
    scroll.set_child(Some(&list));
    card.append(&scroll);

    let close: Rc<dyn Fn()> = {
        let (host, backdrop) = (host.clone(), backdrop.clone());
        Rc::new(move || {
            if backdrop.parent().is_some() {
                host.remove_overlay(&backdrop);
            }
        })
    };
    let mut chosen_row: Option<gtk::Button> = None;
    for (i, text) in labels.iter().enumerate() {
        let row = gtk::Button::new();
        row.add_css_class("picker-item");
        let hb = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        let radio = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        radio.add_css_class("picker-radio");
        radio.set_valign(gtk::Align::Center);
        let l = gtk::Label::new(Some(text));
        l.set_xalign(0.0);
        l.set_hexpand(true);
        l.set_wrap(true);
        hb.append(&radio);
        hb.append(&l);
        row.set_child(Some(&hb));
        if i == sel.get() {
            row.add_css_class("on");
            chosen_row = Some(row.clone());
        }
        let (sel, shown, on_change, close, text) = (
            sel.clone(),
            shown.clone(),
            on_change.clone(),
            close.clone(),
            text.clone(),
        );
        row.connect_clicked(move |_| {
            close();
            if sel.replace(i) != i {
                shown.set_text(&text);
                on_change(i);
            }
        });
        list.append(&row);
    }
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("picker-cancel");
    cancel.set_halign(gtk::Align::End);
    {
        let close = close.clone();
        cancel.connect_clicked(move |_| close());
    }
    card.append(&cancel);
    backdrop.append(&card);

    // a tap on the dimmed part (not the card) closes the list
    let tap = gtk::GestureClick::new();
    {
        let (b, close) = (backdrop.clone(), close.clone());
        tap.connect_released(move |_, _, x, y| {
            if b.pick(x, y, gtk::PickFlags::DEFAULT).as_ref() == Some(b.upcast_ref::<gtk::Widget>())
            {
                close();
            }
        });
    }
    backdrop.add_controller(tap);
    // Escape closes it too
    let keys = gtk::EventControllerKey::new();
    {
        let close = close.clone();
        keys.connect_key_pressed(move |_, k, _, _| {
            if k == gdk::Key::Escape {
                close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    backdrop.add_controller(keys);
    host.add_overlay(&backdrop);

    // start with the chosen one in view
    if let Some(row) = chosen_row {
        let scroll = scroll.clone();
        let _ = glib::idle_add_local_once(move || {
            if let Some(r) = row.compute_bounds(&list) {
                let adj = scroll.vadjustment();
                let y = f64::from(r.y()) - adj.page_size() / 2.0 + f64::from(r.height()) / 2.0;
                adj.set_value(y.clamp(adj.lower(), (adj.upper() - adj.page_size()).max(0.0)));
            }
        });
    }
}

/// Swatch rows: mark the tapped one as selected and save.
fn select_on_click(buttons: &[gtk::Button], save: Rc<dyn Fn(usize)>) {
    let all: Rc<Vec<gtk::Button>> = Rc::new(buttons.to_vec());
    for (i, b) in buttons.iter().enumerate() {
        let (all, save) = (all.clone(), save.clone());
        b.connect_clicked(move |_| {
            for (j, o) in all.iter().enumerate() {
                if i == j {
                    o.add_css_class("selected");
                } else {
                    o.remove_css_class("selected");
                }
            }
            save(i);
        });
    }
}

#[allow(deprecated)]
fn choose_image(ctx: &Rc<Ctx>, label: &gtk::Label, key: &'static str) {
    let dialog = gtk::FileChooserNative::new(
        Some("Choose a picture"),
        Some(&ctx.window),
        gtk::FileChooserAction::Open,
        Some("Choose"),
        Some("Cancel"),
    );
    let filter = gtk::FileFilter::new();
    filter.add_mime_type("image/*");
    filter.set_name(Some("Images"));
    dialog.add_filter(&filter);
    let (c, l) = (ctx.clone(), label.clone());
    dialog.connect_response(move |d, resp| {
        if resp == gtk::ResponseType::Accept {
            if let Some(path) = d.file().and_then(|f| f.path()) {
                let p = path.to_string_lossy().into_owned();
                config::set(key, &p);
                l.set_text(&p);
            }
        }
        c.chooser.borrow_mut().take();
    });
    dialog.show();
    *ctx.chooser.borrow_mut() = Some(dialog);
}

const SCREEN_OFF: &[(&str, &str)] = &[
    ("0", "Never"),
    ("30", "30 seconds"),
    ("60", "1 minute"),
    ("120", "2 minutes"),
    ("300", "5 minutes"),
    ("600", "10 minutes"),
    ("1800", "30 minutes"),
];

const SLEEP: &[(&str, &str)] = &[
    ("0", "Never"),
    ("300", "5 minutes"),
    ("600", "10 minutes"),
    ("900", "15 minutes"),
    ("1800", "30 minutes"),
    ("3600", "1 hour"),
];

const BATTERY_LEVELS: &[(&str, &str)] = &[
    ("5", "5%"),
    ("10", "10%"),
    ("15", "15%"),
    ("20", "20%"),
    ("25", "25%"),
    ("30", "30%"),
];
const BATTERY_MODES: &[(&str, &str)] = &[
    ("both", "Sound and popup"),
    ("popup", "Popup only"),
    ("off", "Off"),
];

fn power(_: &Rc<Ctx>) -> gtk::Widget {
    let cfg = Config::load();
    // hear/see the warning now (RustTouch shows it)
    let try_it = gtk::Button::with_label("Try it");
    try_it.connect_clicked(|_| {
        let _ = std::process::Command::new("rusttouch")
            .args(["--do", "battery-test"])
            .spawn();
    });
    page(&[
        group(
            "Low battery",
            &[
                choice_row(
                    &cfg,
                    "battery_warn_level",
                    "Warn at",
                    Some("Once, when the battery gets this low (not while charging)"),
                    BATTERY_LEVELS,
                ),
                choice_row(&cfg, "battery_warn_mode", "Warning", None, BATTERY_MODES),
                switch_row(
                    &cfg,
                    "battery_warn_test",
                    "Test: warn at every 1% down",
                    Some("For trying out the sound - turn off for normal use"),
                ),
                row("Hear it now", None, &try_it),
            ],
        ),
        group(
            "When idle",
            &[
                choice_row(
                    &cfg,
                    "screen_off_after",
                    "Turn screen off after",
                    None,
                    SCREEN_OFF,
                ),
                switch_row(
                    &cfg,
                    "lock_on_screen_off",
                    "Lock when the screen turns off",
                    None,
                ),
                choice_row(&cfg, "sleep_after", "Sleep after", None, SLEEP),
            ],
        ),
        group(
            "Power button",
            &[
                choice_row(&cfg, "power_button", "Press", None, config::POWER_ACTIONS),
                choice_row(
                    &cfg,
                    "power_button_hold",
                    "Press and hold (if supported)",
                    Some("Many laptops only report the button once it's released, so a hold can't be seen"),
                    config::POWER_ACTIONS,
                ),
                choice_row(
                    &cfg,
                    "power_button_double",
                    "Double press (if supported)",
                    Some("Not every power button reports quick repeats. Anything but \"Do nothing\" makes a single press wait 0.3s"),
                    config::POWER_ACTIONS,
                ),
            ],
        ),
        note(
            "When the screen is off, any press of the power button turns it back on. \
             Power button handling needs the udev rule from install.sh (then reboot); \
             without rusttouch running, the button sleeps.",
        ),
    ])
}

const KB_HEIGHTS: &[(&str, &str)] = &[
    ("auto", "Automatic (300 px)"),
    ("220", "Short (220 px)"),
    ("260", "260 px"),
    ("300", "Normal (300 px)"),
    ("340", "Tall (340 px)"),
    ("400", "Very tall (400 px)"),
];

const KB_HEIGHTS_LANDSCAPE: &[(&str, &str)] = &[
    ("auto", "Automatic (260 px)"),
    ("180", "Short (180 px)"),
    ("220", "220 px"),
    ("260", "Normal (260 px)"),
    ("300", "Tall (300 px)"),
    ("340", "Very tall (340 px)"),
];

const KB_FONTS: &[(&str, &str)] = &[
    ("auto", "Automatic"),
    ("14", "Small"),
    ("18", "Medium"),
    ("22", "Large"),
    ("26", "Extra large"),
];

const KB_ROUNDING: &[(&str, &str)] = &[
    ("auto", "Automatic (square)"),
    ("0", "Square"),
    ("4", "Slightly rounded"),
    ("8", "Rounded"),
    ("14", "Very rounded"),
];

const KB_STYLES: &[(&str, &str)] = &[
    ("theme", "Match RustTouch (theme + accent)"),
    ("wvkbd", "Classic dark keys"),
];

fn keyboard(_: &Rc<Ctx>) -> gtk::Widget {
    let cfg = Config::load();
    page(&[
        group(
            "On-screen keyboard",
            &[
                switch_row(
                    &cfg,
                    "keyboard_auto",
                    "Show automatically",
                    Some("Pop up when you tap a text field"),
                ),
                choice_row(
                    &cfg,
                    "cursor_blink",
                    "Text cursor blinking",
                    Some("In RustTouch and in apps"),
                    &[
                        ("standard", "Standard"),
                        ("slow", "Slow"),
                        ("fast", "Fast"),
                        ("off", "Off (steady)"),
                    ],
                ),
                switch_row(
                    &cfg,
                    "keyboard_resize_apps",
                    "Make room for the keyboard",
                    Some("Apps shrink to fit above it, so what you type into stays visible"),
                ),
            ],
        ),
        group("Preview", &[keyboard_preview()]),
        group(
            "Size & look",
            &[
                choice_row(
                    &cfg,
                    "keyboard_height",
                    "Height (portrait)",
                    None,
                    KB_HEIGHTS,
                ),
                choice_row(
                    &cfg,
                    "keyboard_height_landscape",
                    "Height (landscape)",
                    None,
                    KB_HEIGHTS_LANDSCAPE,
                ),
                choice_row(
                    &cfg,
                    "keyboard_landscape_layout",
                    "Layout (landscape)",
                    None,
                    &[
                        ("wide", "Wide: every key on one page"),
                        ("compact", "Compact: like portrait"),
                    ],
                ),
                choice_row(&cfg, "keyboard_font_size", "Key labels", None, KB_FONTS),
                choice_row(&cfg, "keyboard_rounding", "Key shape", None, KB_ROUNDING),
                choice_row(&cfg, "keyboard_style", "Colors", None, KB_STYLES),
            ],
        ),
        note(
            "The RustTouch keyboard is used everywhere except the lock screen. Changes \
             apply right away. ⌨ at the bottom right of the button bar opens it by hand; \
             Back (⌄) closes it. ⌨→ switches to symbols and 🙂; Ctr / Alt / Sup work for \
             shortcuts (tap one, then the key); hold a key to repeat it.",
        ),
    ])
}

fn network(_: &Rc<Ctx>) -> gtk::Widget {
    let cfg = Config::load();
    let more = gtk::Button::with_label("Open");
    more.connect_clicked(|_| {
        for (cmd, args) in [
            ("nm-connection-editor", &[][..]),
            ("gnome-control-center", &["wifi"][..]),
        ] {
            let ok = std::process::Command::new(cmd)
                .args(args)
                .env("XDG_CURRENT_DESKTOP", "GNOME")
                .spawn()
                .is_ok();
            if ok {
                break;
            }
        }
    });
    page(&[group(
        "Network",
        &[
            switch_row(&cfg, "wifi", "Wi-Fi", None),
            switch_row(&cfg, "bluetooth", "Bluetooth", None),
            row("Wi-Fi networks & passwords", None, &more),
        ],
    )])
}

fn windows(_: &Rc<Ctx>) -> gtk::Widget {
    let cfg = Config::load();
    let on_sway = std::env::var_os("SWAYSOCK").is_some();
    let status = if on_sway {
        "Split screen is on (running on sway)."
    } else {
        "Split screen needs sway: sudo apt install sway, pick Automatic or sway below, then log out and back in."
    };
    page(&[
        group(
            "Window manager",
            &[choice_row(
                &cfg,
                "compositor",
                "Compositor",
                Some("Takes effect next login"),
                &[
                    ("auto", "Automatic (sway if installed)"),
                    ("sway", "sway - split screen"),
                    ("phoc", "phoc - phone style, one app at a time"),
                    ("labwc", "labwc - desktop style"),
                ],
            )],
        ),
        note(status),
        group(
            "Split screen",
            &[
                choice_row(
                    &cfg,
                    "window_mode",
                    "New apps",
                    None,
                    &[
                        ("fullscreen", "Open full screen (tablet)"),
                        ("tile", "Open next to what's showing (desktop)"),
                    ],
                ),
                choice_row(
                    &cfg,
                    "split_direction",
                    "Split puts apps",
                    None,
                    &[
                        ("auto", "Automatic (by screen shape)"),
                        ("side", "Side by side"),
                        ("stacked", "Top and bottom"),
                    ],
                ),
            ],
        ),
        group(
            "How to use split screen",
            &[{
                let l = gtk::Label::new(Some(
                    "• Recents (■) shows window groups: apps that share the screen are one card.\n\
                     • Hold a window in a card and drag it onto the edge of another window (in the \
                     same group or another one) to put it on that side, or onto empty space to give \
                     it a group of its own.\n\
                     • ✕ on a window closes just that window; flick a card up to close the whole group.\n\
                     • Drag the handle on the line between apps to resize them; tap it to arrange \
                     on the screen itself (Flip, Full, Close).",
                ));
                l.set_wrap(true);
                l.set_xalign(0.0);
                l.add_css_class("row");
                l.upcast()
            }],
        ),
    ])
}

fn about(_: &Rc<Ctx>) -> gtk::Widget {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/<uid>".into());
    let text = format!(
        "RustTouch {}\n\nSettings file:\n{}\n\nLog:\n{}/rusttouch/rusttouch.log",
        env!("CARGO_PKG_VERSION"),
        config::path().display(),
        runtime
    );
    let l = gtk::Label::new(Some(&text));
    l.set_selectable(true);
    l.set_wrap(true);
    l.set_xalign(0.0);
    l.add_css_class("row");
    page(&[group("About", &[l.upcast()])])
}

// ------------------------------------------------------------ displays

/// A dropdown built from runtime strings (monitor names, modes...).
fn dyn_choice(
    title: &str,
    sub: Option<&str>,
    labels: &[String],
    selected: Option<usize>,
    on_change: impl Fn(usize) + 'static,
) -> gtk::Widget {
    let (b, _) = picker(title, labels.to_vec(), selected, on_change);
    row(title, sub, &b)
}

const SCALES: &[&str] = &["1", "1.25", "1.5", "1.75", "2", "2.5", "3"];
const ROTATIONS: &[(&str, &str)] = &[
    ("normal", "Normal"),
    ("90", "90°"),
    ("180", "180°"),
    ("270", "270°"),
];

fn displays(ctx: &Rc<Ctx>) -> gtk::Widget {
    let cfg = Config::load();
    // Other screens always show a copy of the built-in one, so there's no
    // "main screen" to choose any more (an old choice is dropped).
    if cfg.get("shell_output") != "auto" && !cfg.get("shell_output").is_empty() {
        config::set("shell_output", "auto");
    }
    let outs = display::list();
    let mut parts: Vec<gtk::Widget> = Vec::new();
    let main = display::shell_output(&cfg, &outs).unwrap_or_default();

    // one scale for everything: the other screens show a copy of the main one
    let scale_now = outs
        .iter()
        .find(|o| o.name == main)
        .map(|o| o.scale)
        .unwrap_or(1.0);
    let labels: Vec<String> = SCALES
        .iter()
        .map(|s| format!("{}%", (s.parse::<f64>().unwrap_or(1.0) * 100.0) as i32))
        .collect();
    let sel = SCALES
        .iter()
        .position(|s| (s.parse::<f64>().unwrap_or(1.0) - scale_now).abs() < 0.01);
    let mut screen_rows = vec![dyn_choice(
        "Scale",
        Some("How big everything is"),
        &labels,
        sel,
        |i| config::set("display_scale", SCALES[i]),
    )];
    screen_rows.push(switch_row(
        &cfg,
        "auto_rotate",
        "Auto-rotate",
        Some("The built-in screen follows how you hold it"),
    ));
    screen_rows.push(choice_row(
        &cfg,
        "show_cursor",
        "Mouse pointer",
        None,
        &[
            ("auto", "While a mouse is used"),
            ("on", "Always"),
            ("off", "Never"),
        ],
    ));
    parts.push(group("Screen", &screen_rows));

    if outs.is_empty() {
        parts.push(note(
            "No screens found. This page works inside the RustTouch session (it uses wlr-randr).",
        ));
        return page(&parts);
    }

    // each screen: on/off (not the main one), resolution and refresh rate, rotation
    let mut ordered: Vec<&display::Output> = outs.iter().collect();
    ordered.sort_by_key(|o| o.name != main);
    for o in ordered {
        let name = o.name.clone();
        let is_main = o.name == main;
        let mut rows: Vec<gtk::Widget> = Vec::new();

        if !is_main {
            let sw = gtk::Switch::new();
            sw.set_active(o.enabled);
            let (n, c) = (name.clone(), ctx.clone());
            sw.connect_active_notify(move |s| {
                display::save(&n, "enabled", if s.is_active() { "on" } else { "off" });
                refresh_later(&c, "Displays");
            });
            rows.push(row("Show a copy of the main screen", None, &sw));
        }

        if o.enabled {
            let labels: Vec<String> = o.modes.iter().map(|m| m.label()).collect();
            let sel = o.modes.iter().position(|m| m.current);
            let (n, modes) = (name.clone(), o.modes.clone());
            rows.push(dyn_choice(
                "Resolution and refresh rate",
                None,
                &labels,
                sel,
                move |i| {
                    if let Some(m) = modes.get(i) {
                        display::save(&n, "mode", &m.arg());
                    }
                },
            ));

            let labels: Vec<String> = ROTATIONS.iter().map(|r| r.1.to_string()).collect();
            let sel = ROTATIONS.iter().position(|r| r.0 == o.transform);
            let n = name.clone();
            let sub = (is_main && display::is_builtin(&o.name))
                .then_some("Ignored while auto-rotate is on");
            rows.push(dyn_choice("Rotation", sub, &labels, sel, move |i| {
                display::save(&n, "transform", ROTATIONS[i].0);
            }));
        }
        let what = if is_main {
            "Main screen"
        } else {
            "Other screen"
        };
        let title = if o.desc.is_empty() {
            format!("{what}  ·  {}", o.name)
        } else {
            format!("{what}  ·  {}", o.desc)
        };
        parts.push(group(&title, &rows));
    }

    parts.push(note(
        "Other screens show a copy of the main screen (needs wl-mirror), at their own \
         resolution and rotation. Settings are remembered per screen and come back \
         whenever it's plugged in.",
    ));
    page(&parts)
}

// ------------------------------------------------------------ startup apps

const AUTOSTART_MODES: &[(&str, &str)] = &[
    ("all", "Everything GNOME starts"),
    ("chosen", "Only the ones I pick"),
];

fn startup(ctx: &Rc<Ctx>) -> gtk::Widget {
    let cfg = Config::load();
    let chosen_mode = cfg.get("autostart_mode") == "chosen";
    let entries = autostart::list(false);

    // mode: when switching to "chosen", start with what runs now
    let labels: Vec<String> = AUTOSTART_MODES.iter().map(|m| m.1.to_string()).collect();
    let sel = AUTOSTART_MODES
        .iter()
        .position(|m| m.0 == cfg.get("autostart_mode"))
        .or(Some(0));
    let (c, all_ids) = (
        ctx.clone(),
        entries
            .iter()
            .filter(|e| e.enabled)
            .map(|e| e.id.clone())
            .collect::<Vec<_>>(),
    );
    let mode_row = dyn_choice("RustTouch starts", None, &labels, sel, move |i| {
        let mode = AUTOSTART_MODES[i].0;
        let cfg = Config::load();
        if mode == "chosen" && cfg.get("autostart_apps").is_empty() {
            config::set("autostart_apps", &all_ids.join(","));
        }
        config::set("autostart_mode", mode);
        refresh_later(&c, "Startup apps");
    });

    let mut rows: Vec<gtk::Widget> = Vec::new();
    for e in entries {
        let hb = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        hb.set_valign(gtk::Align::Center);

        // RustTouch's own pick (only in "chosen" mode)
        if chosen_mode {
            let rt = gtk::CheckButton::with_label("RustTouch");
            rt.set_active(autostart::chosen(&cfg).contains(&e.id));
            let id = e.id.clone();
            rt.connect_toggled(move |b| {
                let mut list = autostart::chosen(&Config::load());
                list.retain(|x| *x != id);
                if b.is_active() {
                    list.push(id.clone());
                }
                config::set("autostart_apps", &list.join(","));
            });
            hb.append(&rt);
        }

        // the shared ~/.config/autostart entry (GNOME, and RustTouch in "all" mode)
        let gnome = gtk::CheckButton::with_label(if chosen_mode { "GNOME" } else { "On" });
        gnome.set_active(e.enabled);
        let e2 = e.clone();
        gnome.connect_toggled(move |b| {
            if let Err(err) = autostart::set_enabled(&e2, b.is_active()) {
                eprintln!("rusttouch-settings: {err}");
            }
        });
        hb.append(&gnome);

        if e.user && !e.system {
            let x = gtk::Button::with_label("×");
            x.set_tooltip_text(Some("Remove"));
            x.set_valign(gtk::Align::Center);
            let (e3, c) = (e.clone(), ctx.clone());
            x.connect_clicked(move |_| {
                let _ = autostart::remove(&e3);
                refresh_later(&c, "Startup apps");
            });
            hb.append(&x);
        }
        rows.push(row(&e.name, Some(e.exec.as_str()), &hb));
    }
    if rows.is_empty() {
        rows.push(note("Nothing starts at login yet."));
    }

    // add an installed app
    let mut apps: Vec<gio::AppInfo> = gio::AppInfo::all()
        .into_iter()
        .filter(|a| a.should_show())
        .collect();
    apps.sort_by_key(|a| a.display_name().to_lowercase());
    let names: Vec<String> = apps.iter().map(|a| a.display_name().to_string()).collect();
    let (dd, chosen) = picker("Add an app", names, None, |_| {});
    dd.set_hexpand(true);
    let add_app = gtk::Button::with_label("Add");
    let c = ctx.clone();
    add_app.connect_clicked(move |_| {
        if let Some(id) = apps.get(chosen.get()).and_then(|a| a.id()) {
            if let Err(e) = autostart::add_app(&id) {
                eprintln!("rusttouch-settings: {e}");
            }
            refresh_later(&c, "Startup apps");
        }
    });
    let app_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    app_row.add_css_class("row");
    app_row.append(&dd);
    app_row.append(&add_app);

    // add a command
    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("Name (e.g. rustsync)"));
    let cmd = gtk::Entry::new();
    cmd.set_placeholder_text(Some("Command (e.g. rustsync start)"));
    let add_cmd = gtk::Button::with_label("Add command");
    let status = gtk::Label::new(None);
    status.add_css_class("row-sub");
    let (c, n2, c2, st) = (ctx.clone(), name.clone(), cmd.clone(), status.clone());
    add_cmd.connect_clicked(
        move |_| match autostart::add_command(&n2.text(), &c2.text()) {
            Ok(()) => refresh_later(&c, "Startup apps"),
            Err(e) => st.set_text(&e),
        },
    );
    let cmd_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    cmd_box.add_css_class("row");
    cmd_box.append(&name);
    cmd_box.append(&cmd);
    cmd_box.append(&add_cmd);
    cmd_box.append(&status);

    page(&[
        group("RustTouch", &[mode_row]),
        group("Starts at login", &rows),
        group("Add an app", &[app_row.upcast()]),
        group("Add a command", &[cmd_box.upcast()]),
        note(
            "These are the standard ~/.config/autostart entries that GNOME uses. \
             \"Everything GNOME starts\" runs the same set in RustTouch; \"Only the ones I pick\" \
             lets you tick a subset for RustTouch without changing GNOME. \
             Entries meant only for other desktops (GNOME's own helpers) are hidden here.",
        ),
    ])
}

/// Rebuild the page called `name` shortly after something on it changed
/// (not from inside the widget's own signal, and after rusttouch applied it).
fn refresh_later(ctx: &Rc<Ctx>, name: &'static str) {
    let c = ctx.clone();
    let _ = glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
        if let Some(i) = PAGES.iter().position(|p| p.0 == name) {
            open_page(&c, i);
        }
    });
}

// ------------------------------------------------------------ clock & bars

const TOP_SIZES: &[(&str, &str)] = &[
    ("24", "Tiny (24 px)"),
    ("28", "Small (28 px)"),
    ("32", "Normal (32 px)"),
    ("36", "36 px"),
    ("40", "Large (40 px)"),
    ("44", "44 px"),
    ("48", "Huge (48 px)"),
];

const NAV_SIZES: &[(&str, &str)] = &[
    ("32", "Tiny (32 px)"),
    ("36", "36 px"),
    ("40", "Small (40 px)"),
    ("44", "44 px"),
    ("48", "48 px"),
    ("52", "52 px"),
    ("56", "Normal (56 px)"),
    ("64", "Large (64 px)"),
    ("72", "Huge (72 px)"),
];

const BAR_CLOCK: &[(&str, &str)] = &[
    ("time", "Time  (2:05 PM)"),
    ("day_time", "Day and time  (Thu 2:05 PM)"),
    ("date_time", "Date and time  (Thu, Nov 5 2:05 PM)"),
];

const DATE_FORMATS: &[(&str, &str)] = &[
    ("full", "Thursday November 5, 2026"),
    ("long", "Thursday, November 5"),
    ("short", "Thu, Nov 5"),
    ("numeric", "11/05/2026"),
    ("iso", "2026-11-05"),
];

fn clock_bars(_: &Rc<Ctx>) -> gtk::Widget {
    let cfg = Config::load();
    page(&[
        group(
            "Clock",
            &[
                switch_row(
                    &cfg,
                    "time_24h",
                    "24-hour time",
                    Some("14:05 instead of 2:05 PM"),
                ),
                choice_row(&cfg, "bar_clock", "Top bar shows", None, BAR_CLOCK),
                choice_row(
                    &cfg,
                    "date_format",
                    "Date",
                    Some("Pull-down menu and lock screen"),
                    DATE_FORMATS,
                ),
            ],
        ),
        group(
            "Bars",
            &[
                choice_row(&cfg, "top_bar_size", "Top bar height", None, TOP_SIZES),
                choice_row(
                    &cfg,
                    "nav_bar_size",
                    "Button bar height",
                    Some("The bar with ◀ ● ■"),
                    NAV_SIZES,
                ),
                switch_row(
                    &cfg,
                    "bars_autohide",
                    "Auto-hide the bars",
                    Some("Over apps both bars hide; tap or swipe the top or bottom edge to bring them back"),
                ),
                choice_row(
                    &cfg,
                    "bars_autohide_delay",
                    "Hide them after",
                    Some("Touching a bar starts the wait over"),
                    &[
                        ("3", "3 seconds"),
                        ("6", "6 seconds"),
                        ("10", "10 seconds"),
                        ("20", "20 seconds"),
                    ],
                ),
            ],
        ),
        note("Changes apply right away."),
    ])
}

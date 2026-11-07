//! ~/.config/rusttouch/rusttouch.conf - one simple `key = value` file shared by
//! rusttouch and rusttouch-settings. Either program (or you, in a text editor) can
//! change it; rusttouch watches it and applies changes live.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

pub struct Key {
    pub name: &'static str,
    pub default: &'static str,
    pub help: &'static str,
}

pub const KEYS: &[Key] = &[
    Key {
        name: "theme",
        default: "dark",
        help: "dark | light | amoled | nord | dracula | gruvbox | solarized-dark | solarized-light | ocean | sepia | contrast",
    },
    Key {
        name: "accent",
        default: "blue",
        help: "blue | green | purple | orange | pink | red",
    },
    Key {
        name: "background",
        default: "aurora",
        help: "aurora | ocean | sunset | forest | graphite - or the full path of an image",
    },
    Key {
        name: "background_fit",
        default: "fill",
        help: "for pictures: fill | fit | stretch | center | tile | zoom150 | zoom200",
    },
    Key {
        name: "lock_background",
        default: "same",
        help: "lock screen background: same (as home) | a preset name | path of a picture",
    },
    Key { name: "lock_background_fit", default: "fill", help: "like background_fit, for the lock screen" },
    Key { name: "lock_message", default: "", help: "text shown on the lock screen (e.g. contact info)" },
    Key {
        name: "wifi",
        default: "on",
        help: "on | off  (remembered, restored at login)",
    },
    Key {
        name: "bluetooth",
        default: "on",
        help: "on | off  (remembered, restored at login)",
    },
    Key {
        name: "auto_rotate",
        default: "on",
        help: "on | off  (follow the accelerometer)",
    },
    Key {
        name: "keyboard_auto",
        default: "on",
        help: "on | off  (pop up the on-screen keyboard when a text field is tapped)",
    },
    Key {
        name: "keyboard_resize_apps",
        default: "on",
        help: "on | off  (apps shrink to the space above the keyboard, so nothing is hidden behind it)",
    },
    Key {
        name: "keyboard_height",
        default: "auto",
        help: "on-screen keyboard height in portrait (px), or auto",
    },
    Key {
        name: "keyboard_height_landscape",
        default: "auto",
        help: "on-screen keyboard height in landscape (px), or auto",
    },
    Key { name: "keyboard_font_size", default: "auto", help: "key label size (px), or auto" },
    Key { name: "keyboard_rounding", default: "auto", help: "key corner rounding (px), or auto" },
    Key {
        name: "keyboard_landscape_layout",
        default: "wide",
        help: "wide (every key on one page, like a laptop) | compact (like portrait: letters + a symbols page)",
    },
    Key {
        name: "keyboard_style",
        default: "theme",
        help: "theme (match RustTouch colors) | wvkbd (classic dark keys)",
    },
    Key {
        name: "screen_off_after",
        default: "120",
        help: "seconds without input before the screen turns off, 0 = never",
    },
    Key {
        name: "lock_on_screen_off",
        default: "on",
        help: "on | off  (lock when the screen turns off)",
    },
    Key {
        name: "sleep_after",
        default: "600",
        help: "seconds without input before suspending, 0 = never",
    },
    Key {
        name: "power_button",
        default: "sleep",
        help: "short press: sleep | lock | screen_off | menu | poweroff | nothing",
    },
    Key {
        name: "power_button_hold",
        default: "menu",
        help: "press and hold (~0.7s): same choices",
    },
    Key {
        name: "power_button_double",
        default: "nothing",
        help: "double press: same choices (anything but nothing delays the short press by 0.3s)",
    },
    Key {
        name: "show_cursor",
        default: "auto",
        help: "auto | on | off  (mouse pointer over rusttouch; auto = while a mouse is used)",
    },
    Key {
        name: "window_mode",
        default: "fullscreen",
        help: "fullscreen | tile  (sway: each new app gets the whole screen, or opens next to the ones showing)",
    },
    Key {
        name: "split_direction",
        default: "auto",
        help: "auto | side | stacked  (how Split places two apps; auto = side by side on a wide screen)",
    },
    Key {
        name: "cursor_blink",
        default: "standard",
        help: "standard | slow | fast | off  (text cursor blinking, in RustTouch and in apps)",
    },
    Key {
        name: "compositor",
        default: "auto",
        help: "auto | sway | phoc | labwc  (next login; auto = sway if installed - split screen needs sway)",
    },
    Key {
        name: "top_bar_size",
        default: "32",
        help: "top bar height in pixels (24 - 48)",
    },
    Key {
        name: "nav_bar_size",
        default: "56",
        help: "button bar height in pixels (32 - 72)",
    },
    Key {
        name: "bars_autohide_delay",
        default: "3",
        help: "seconds the auto-hiding bars stay after you bring them back or touch them",
    },
    Key {
        name: "display_scale",
        default: "",
        help: "how big everything is on the screen, e.g. 1.5 (empty = leave it as it is); one scale for every screen, since other screens show a copy",
    },
    Key {
        name: "battery_warn_level",
        default: "15",
        help: "percent: warn when the battery gets this low (while not charging)",
    },
    Key {
        name: "battery_warn_mode",
        default: "both",
        help: "both | popup | off  (the low battery warning: sound and popup, popup only, or none)",
    },
    Key {
        name: "bars_autohide",
        default: "off",
        help: "on | off  (hide both bars over apps; tap or swipe a screen edge to bring them back)",
    },
    Key {
        name: "time_24h",
        default: "off",
        help: "on | off  (24-hour clock)",
    },
    Key {
        name: "bar_clock",
        default: "time",
        help: "top bar clock: time | day_time | date_time",
    },
    Key {
        name: "date_format",
        default: "full",
        help: "full (Thursday November 5, 2026) | long | short | numeric | iso",
    },
    Key {
        name: "shell_output",
        default: "auto",
        help:
            "monitor for rusttouch's bars and home: auto (built-in screen) or a name like HDMI-A-1",
    },
    Key {
        name: "autostart_mode",
        default: "all",
        help: "all = start what GNOME starts (~/.config/autostart) | chosen = only autostart_apps",
    },
    Key {
        name: "autostart_apps",
        default: "",
        help: "with autostart_mode = chosen: the entries RustTouch starts (file names, comma separated)",
    },
    Key {
        name: "home_apps",
        default: "",
        help: "home screen icons in portrait: id@column:row, ... - long-press to arrange",
    },
    Key {
        name: "home_apps_landscape",
        default: "",
        help: "home screen icons in landscape (starts as a copy of portrait)",
    },
];

pub const POWER_ACTIONS: &[(&str, &str)] = &[
    ("sleep", "Sleep"),
    ("lock", "Lock & turn screen off"),
    ("screen_off", "Turn screen off"),
    ("menu", "Show power options"),
    ("poweroff", "Power off"),
    ("nothing", "Do nothing"),
];

pub const ACCENTS: &[(&str, &str)] = &[
    ("blue", "#8ab4f8"),
    ("green", "#81c995"),
    ("purple", "#c58af9"),
    ("orange", "#fcad70"),
    ("pink", "#ff8bcb"),
    ("red", "#f28b82"),
];

pub const BACKGROUNDS: &[(&str, &str)] = &[
    (
        "aurora",
        "linear-gradient(160deg, #1b2a49 0%, #3a1c5c 55%, #0f3b3a 100%)",
    ),
    (
        "ocean",
        "linear-gradient(165deg, #0b3954 0%, #087e8b 60%, #0b2545 100%)",
    ),
    (
        "sunset",
        "linear-gradient(170deg, #3d1e6d 0%, #c04b6b 55%, #f2a65a 100%)",
    ),
    (
        "forest",
        "linear-gradient(160deg, #0f2a1d 0%, #2d6a4f 55%, #1b4332 100%)",
    ),
    (
        "graphite",
        "linear-gradient(160deg, #1f1f24 0%, #3a3a42 60%, #18181c 100%)",
    ),
    (
        "midnight",
        "radial-gradient(circle at 30% 20%, #2b3a67 0%, #0d1326 55%, #05070f 100%)",
    ),
    (
        "ember",
        "radial-gradient(circle at 70% 85%, #ff7a3d 0%, #a3263b 40%, #2a0b1f 100%)",
    ),
    (
        "glacier",
        "linear-gradient(180deg, #c9e8f5 0%, #6fb1d8 45%, #1f4e79 100%)",
    ),
    (
        "lavender",
        "linear-gradient(160deg, #e6d9ff 0%, #a98fe0 50%, #5b4a9b 100%)",
    ),
    (
        "mint",
        "linear-gradient(165deg, #d8f5e3 0%, #6cc9a1 50%, #1f6f5c 100%)",
    ),
    (
        "rose",
        "linear-gradient(170deg, #ffd6e0 0%, #f08ca8 50%, #8c2f5a 100%)",
    ),
    (
        "dune",
        "linear-gradient(175deg, #f3dfb7 0%, #d9a066 45%, #7a4b2a 100%)",
    ),
    (
        "deep-sea",
        "radial-gradient(circle at 50% 110%, #1aa3a3 0%, #0b4f6c 40%, #01161e 100%)",
    ),
    (
        "neon",
        "linear-gradient(135deg, #12002b 0%, #3a0ca3 40%, #f72585 75%, #4cc9f0 100%)",
    ),
    (
        "dusk",
        "linear-gradient(180deg, #ffb88c 0%, #de6262 40%, #3b1d4a 100%)",
    ),
    (
        "slate",
        "linear-gradient(160deg, #3c4a5c 0%, #232b36 60%, #11161c 100%)",
    ),
    ("amoled", "#000000"),
];

/// A full color theme. Strings are CSS colors.
pub struct Palette {
    pub label: &'static str,
    /// tells GTK apps / the settings app to use their dark style
    pub dark: bool,
    pub fg: &'static str,
    pub bar_bg: &'static str,
    pub bar_fg: &'static str,
    pub nav_bg: &'static str,
    pub nav_fg: &'static str,
    pub panel_bg: &'static str,
    pub tile_bg: &'static str,
    pub card_bg: &'static str,
    /// keyboard (wvkbd): background, key, special key, text - rrggbb
    pub kb: [&'static str; 4],
}

pub const THEMES: &[(&str, Palette)] = &[
    (
        "dark",
        Palette {
            label: "Dark",
            dark: true,
            fg: "#ffffff",
            bar_bg: "rgba(0,0,0,0.55)",
            bar_fg: "#ffffff",
            nav_bg: "#000000",
            nav_fg: "#e8e8e8",
            panel_bg: "rgba(20,22,30,0.97)",
            tile_bg: "rgba(255,255,255,0.10)",
            card_bg: "#1c1c24",
            kb: ["15151b", "2c2c36", "22222a", "ffffff"],
        },
    ),
    (
        "light",
        Palette {
            label: "Light",
            dark: false,
            fg: "#1b1b1f",
            bar_bg: "rgba(248,248,250,0.88)",
            bar_fg: "#1b1b1f",
            nav_bg: "#f4f4f6",
            nav_fg: "#2a2a2e",
            panel_bg: "rgba(250,250,252,0.97)",
            tile_bg: "rgba(0,0,0,0.07)",
            card_bg: "#e8e8ee",
            kb: ["e4e4ea", "ffffff", "cfcfd8", "1b1b1f"],
        },
    ),
    (
        "amoled",
        Palette {
            label: "Black (AMOLED)",
            dark: true,
            fg: "#ffffff",
            bar_bg: "#000000",
            bar_fg: "#ffffff",
            nav_bg: "#000000",
            nav_fg: "#dddddd",
            panel_bg: "rgba(0,0,0,0.98)",
            tile_bg: "rgba(255,255,255,0.08)",
            card_bg: "#0b0b0b",
            kb: ["000000", "161616", "0c0c0c", "ffffff"],
        },
    ),
    (
        "nord",
        Palette {
            label: "Nord",
            dark: true,
            fg: "#eceff4",
            bar_bg: "rgba(46,52,64,0.85)",
            bar_fg: "#eceff4",
            nav_bg: "#2e3440",
            nav_fg: "#d8dee9",
            panel_bg: "rgba(46,52,64,0.97)",
            tile_bg: "rgba(136,192,208,0.14)",
            card_bg: "#3b4252",
            kb: ["2e3440", "3b4252", "434c5e", "eceff4"],
        },
    ),
    (
        "dracula",
        Palette {
            label: "Dracula",
            dark: true,
            fg: "#f8f8f2",
            bar_bg: "rgba(40,42,54,0.85)",
            bar_fg: "#f8f8f2",
            nav_bg: "#21222c",
            nav_fg: "#f8f8f2",
            panel_bg: "rgba(40,42,54,0.97)",
            tile_bg: "rgba(189,147,249,0.16)",
            card_bg: "#44475a",
            kb: ["21222c", "44475a", "343746", "f8f8f2"],
        },
    ),
    (
        "gruvbox",
        Palette {
            label: "Gruvbox",
            dark: true,
            fg: "#ebdbb2",
            bar_bg: "rgba(29,32,33,0.85)",
            bar_fg: "#ebdbb2",
            nav_bg: "#1d2021",
            nav_fg: "#ebdbb2",
            panel_bg: "rgba(40,40,40,0.97)",
            tile_bg: "rgba(250,189,47,0.14)",
            card_bg: "#3c3836",
            kb: ["1d2021", "3c3836", "282828", "ebdbb2"],
        },
    ),
    (
        "solarized-dark",
        Palette {
            label: "Solarized dark",
            dark: true,
            fg: "#eee8d5",
            bar_bg: "rgba(0,43,54,0.85)",
            bar_fg: "#eee8d5",
            nav_bg: "#002b36",
            nav_fg: "#93a1a1",
            panel_bg: "rgba(0,43,54,0.97)",
            tile_bg: "rgba(42,161,152,0.16)",
            card_bg: "#073642",
            kb: ["002b36", "073642", "0a4252", "eee8d5"],
        },
    ),
    (
        "solarized-light",
        Palette {
            label: "Solarized light",
            dark: false,
            fg: "#073642",
            bar_bg: "rgba(253,246,227,0.9)",
            bar_fg: "#073642",
            nav_bg: "#eee8d5",
            nav_fg: "#586e75",
            panel_bg: "rgba(253,246,227,0.97)",
            tile_bg: "rgba(38,139,210,0.12)",
            card_bg: "#eee8d5",
            kb: ["eee8d5", "fdf6e3", "e2dcc6", "073642"],
        },
    ),
    (
        "ocean",
        Palette {
            label: "Ocean",
            dark: true,
            fg: "#e6f4f1",
            bar_bg: "rgba(4,32,46,0.85)",
            bar_fg: "#e6f4f1",
            nav_bg: "#03141d",
            nav_fg: "#cde8e5",
            panel_bg: "rgba(6,38,54,0.97)",
            tile_bg: "rgba(72,202,228,0.15)",
            card_bg: "#0b3a52",
            kb: ["03141d", "0b3a52", "082c3e", "e6f4f1"],
        },
    ),
    (
        "sepia",
        Palette {
            label: "Paper (sepia)",
            dark: false,
            fg: "#3b2f22",
            bar_bg: "rgba(244,236,216,0.9)",
            bar_fg: "#3b2f22",
            nav_bg: "#eadfc4",
            nav_fg: "#4a3b2a",
            panel_bg: "rgba(247,240,224,0.97)",
            tile_bg: "rgba(120,80,30,0.10)",
            card_bg: "#efe4c9",
            kb: ["e6dbbf", "f7f0e0", "dccfae", "3b2f22"],
        },
    ),
    (
        "contrast",
        Palette {
            label: "High contrast",
            dark: true,
            fg: "#ffffff",
            bar_bg: "#000000",
            bar_fg: "#ffffff",
            nav_bg: "#000000",
            nav_fg: "#ffffff",
            panel_bg: "#000000",
            tile_bg: "rgba(255,255,255,0.22)",
            card_bg: "#111111",
            kb: ["000000", "333333", "1a1a1a", "ffffff"],
        },
    ),
];

/// The theme in the config (unknown names fall back to Dark).
pub fn palette(cfg: &Config) -> &'static Palette {
    let name = cfg.get("theme");
    THEMES
        .iter()
        .find(|t| t.0 == name)
        .map(|t| &t.1)
        .unwrap_or(&THEMES[0].1)
}

pub const BG_FITS: &[(&str, &str)] = &[
    ("fill", "Fill (crop to the screen)"),
    ("fit", "Fit (whole picture)"),
    ("stretch", "Stretch"),
    ("center", "Center (actual size)"),
    ("tile", "Tile"),
    ("zoom150", "Zoom 150%"),
    ("zoom200", "Zoom 200%"),
];

/// CSS `background` value for a wallpaper setting (`bg_key` = preset or
/// picture path, `fit_key` = how a picture fits).
pub fn wallpaper_css(cfg: &Config, bg_key: &str, fit_key: &str) -> String {
    let bg = cfg.get(bg_key);
    if let Some((_, css)) = BACKGROUNDS.iter().find(|b| b.0 == bg) {
        return (*css).to_string();
    }
    if !std::path::Path::new(&bg).is_file() {
        return BACKGROUNDS[0].1.to_string();
    }
    let url = format!(
        "url(\"file://{}\")",
        bg.replace('"', "%22").replace(' ', "%20")
    );
    let how = match cfg.get(fit_key).as_str() {
        "fit" => "center / contain no-repeat",
        "stretch" => "center / 100% 100% no-repeat",
        "center" => "center no-repeat",
        "tile" => "top left repeat",
        "zoom150" => "center / 150% auto no-repeat",
        "zoom200" => "center / 200% auto no-repeat",
        _ => "center / cover no-repeat",
    };
    format!("{url} {how}, #000")
}

/// Home screen wallpaper.
pub fn background_css(cfg: &Config) -> String {
    wallpaper_css(cfg, "background", "background_fit")
}

/// Lock screen wallpaper ("same" = the home one).
pub fn lock_background_css(cfg: &Config) -> String {
    let lb = cfg.get("lock_background");
    if lb.is_empty() || lb == "same" {
        background_css(cfg)
    } else {
        wallpaper_css(cfg, "lock_background", "lock_background_fit")
    }
}

/// wvkbd colors matching the theme: (background, key, special key, pressed, text).
pub fn keyboard_colors(cfg: &Config) -> (String, String, String, String, String) {
    let accent = accent_hex(&cfg.get("accent"))
        .trim_start_matches('#')
        .to_string();
    let k = palette(cfg).kb;
    (k[0].into(), k[1].into(), k[2].into(), accent, k[3].into())
}

/// "#rrggbb" for an accent setting: a named accent, or a custom "#rrggbb".
pub fn accent_hex(name: &str) -> String {
    let custom = name.len() == 7
        && name.starts_with('#')
        && name[1..].chars().all(|c| c.is_ascii_hexdigit());
    if custom {
        return name.to_lowercase();
    }
    ACCENTS
        .iter()
        .find(|a| a.0 == name)
        .map(|a| a.1)
        .unwrap_or(ACCENTS[0].1)
        .to_string()
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn dir() -> PathBuf {
    static MIGRATE: std::sync::Once = std::sync::Once::new();
    let new = config_home().join("rusttouch");
    MIGRATE.call_once(|| migrate_from_rdesk(&new));
    new
}

/// RustTouch used to be called "rdesk": copy ~/.config/rdesk once.
fn migrate_from_rdesk(new: &std::path::Path) {
    let old = config_home().join("rdesk");
    if new.exists() || !old.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(&old) else {
        return;
    };
    if fs::create_dir_all(new).is_err() {
        return;
    }
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let target = if name == "rdesk.conf" {
            "rusttouch.conf".to_string()
        } else {
            name
        };
        if e.path().is_file() {
            let _ = fs::copy(e.path(), new.join(target));
        }
    }
}

pub fn path() -> PathBuf {
    dir().join("rusttouch.conf")
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Config {
    values: BTreeMap<String, String>,
}

impl Config {
    pub fn load() -> Config {
        let mut values = BTreeMap::new();
        if let Ok(s) = fs::read_to_string(path()) {
            for line in s.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    values.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
        Config { values }
    }

    pub fn get(&self, key: &str) -> String {
        if let Some(v) = self.values.get(key) {
            return v.clone();
        }
        KEYS.iter()
            .find(|k| k.name == key)
            .map(|k| k.default.to_string())
            .unwrap_or_default()
    }

    pub fn on(&self, key: &str) -> bool {
        matches!(
            self.get(key).to_lowercase().as_str(),
            "on" | "true" | "yes" | "1"
        )
    }

    pub fn num(&self, key: &str) -> u32 {
        self.get(key).parse().unwrap_or(0)
    }

    /// All (key, value) pairs whose key starts with `prefix`.
    pub fn prefixed(&self, prefix: &str) -> Vec<(String, String)> {
        self.values
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn insert(&mut self, key: &str, value: &str) {
        self.values.insert(key.to_string(), value.to_string());
    }

    /// Write every known key (with its help text) plus any unknown ones.
    /// Written to a temp file and renamed, so readers never see half a file.
    pub fn save(&self) -> std::io::Result<()> {
        let mut out = String::from(
            "# rusttouch settings - edit here or in the Settings app; rusttouch applies changes live.\n\n",
        );
        for k in KEYS {
            out.push_str(&format!(
                "# {}\n{} = {}\n\n",
                k.help,
                k.name,
                self.get(k.name)
            ));
        }
        for (k, v) in &self.values {
            if !KEYS.iter().any(|x| x.name == k) {
                out.push_str(&format!("{k} = {v}\n"));
            }
        }
        fs::create_dir_all(dir())?;
        let tmp = dir().join(".rusttouch.conf.tmp");
        fs::write(&tmp, out)?;
        fs::rename(tmp, path())
    }
}

/// Read-modify-write one key, so two programs never clobber each other.
pub fn set(key: &str, value: &str) {
    let mut c = Config::load();
    if c.values.get(key).map(String::as_str) == Some(value) && path().exists() {
        return;
    }
    c.insert(key, value);
    if let Err(e) = c.save() {
        eprintln!("rusttouch: could not save {}: {e}", path().display());
    }
}

pub fn set_on(key: &str, on: bool) {
    set(key, if on { "on" } else { "off" });
}

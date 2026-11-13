//! RustTV's settings: ~/.config/rusttv/rusttv.json. Launchers (web services
//! opened in a browser), pinned apps and a few display options. Written
//! whenever something changes; a broken or missing file means the defaults.

use serde_json::{json, Value};
use std::path::PathBuf;

/// A web service opened full screen in a browser (its own profile, so each
/// keeps its own sign-in).
#[derive(Clone, Debug, PartialEq)]
pub struct Launcher {
    /// letters, digits and '-' only (names its browser profile and window)
    pub id: String,
    pub name: String,
    pub url: String,
    /// "firefox", "waterfox", "chromium", "google-chrome"
    pub browser: String,
    /// card colour, "#rrggbb"
    pub color: String,
    /// show the service's own icon on its card (when one is found)
    pub picture: bool,
    /// tell the site it's on a TV (its TV layout, where it has one)
    pub tv: bool,
    /// arrow keys jump between the page's buttons and links (Chromium's
    /// "spatial navigation": opens in Chromium or Chrome)
    pub spatial: bool,
    /// trust the certificate the site has now, even if it's home-made (a
    /// camera or server at home): no "not private" warning each time
    pub trust_cert: bool,
    /// this app's own remote buttons (Edit > Remote buttons)
    pub keys: Vec<RemoteKey>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub launchers: Vec<Launcher>,
    /// desktop file ids of pinned apps, in order
    pub apps: Vec<String>,
    /// "1", "1.5", "2", ... (sway output scale); "" = leave as is
    pub scale: String,
    /// the browser new launchers use
    pub browser: String,
    /// how many apps may be open at once: opening one more closes the one
    /// opened longest ago (0: no limit; 1: one at a time, like a TV)
    pub max_apps: u32,
    /// suspend after this many minutes with nothing going on (0: never;
    /// playing video keeps it awake)
    pub auto_suspend: u32,
    /// your remote's own buttons, given jobs (see remote.rs)
    pub keys: Vec<RemoteKey>,
    /// launchers open with your browser's own profile (your history and
    /// sign-ins) instead of one each
    pub own_profile: bool,
    /// the colour of the focus outline and highlights ("#rrggbb")
    pub accent: String,
    /// "dark" (the gradient), "black", "glow" (accent-tinted), "picture"
    pub background: String,
    /// the picture for background "picture"
    pub wallpaper: String,
    /// show the battery at the top (when there is one)
    pub battery_show: bool,
    /// warn when the battery is at or below this % (0: never)
    pub battery_warn: u32,
    /// the screen RustTV is on ("" = automatic); the others show a copy
    pub main_output: String,
    /// how the other screens show the copy: "fit", "fill", "exact"
    pub mirror_fit: String,
    /// picked resolutions: (screen, "1920x1080@60.000Hz")
    pub resolutions: Vec<(String, String)>,
    /// the on-screen keyboard shows in apps' text boxes
    pub app_keyboard: bool,
    /// going home pauses what's playing
    pub pause_on_home: bool,
    /// AirPlay (UxPlay) is on: iPhones, iPads and Macs can show their screen
    pub airplay: bool,
    /// the name the TV has in AirPlay
    pub airplay_name: String,
    /// when a device types a code: "first", "always", "never"
    pub airplay_code: String,
}

/// Accent colours: (colour, name). White is the starting one.
pub const ACCENTS: &[(&str, &str)] = &[
    ("#ffffff", "White"),
    ("#3584e4", "Blue"),
    ("#2190a4", "Teal"),
    ("#3a944a", "Green"),
    ("#c88800", "Yellow"),
    ("#ed5b00", "Orange"),
    ("#e62d42", "Red"),
    ("#d56199", "Pink"),
    ("#9141ac", "Purple"),
    ("#6f8396", "Slate"),
];

pub const BACKGROUNDS: &[(&str, &str)] = &[
    ("dark", "Dark"),
    ("black", "Black"),
    ("glow", "Accent glow"),
    ("picture", "Picture"),
];

/// A remote button given a job: `action` (remote::ACTIONS) for the key
/// `keyval` (its X key name: `name`).
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteKey {
    pub action: String,
    pub keyval: u32,
    pub name: String,
    /// its evdev key code (0: not known)
    pub code: u32,
}

/// Remote buttons from the settings file.
fn parse_keys(v: &Value) -> Vec<RemoteKey> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|k| {
            let name = k["name"].as_str()?.to_string();
            // (it ends up in sway's config: key names only)
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            Some(RemoteKey {
                action: k["action"].as_str()?.to_string(),
                keyval: k["keyval"].as_u64()? as u32,
                name,
                code: k["code"].as_u64().unwrap_or(0) as u32,
            })
        })
        .collect()
}

fn keys_json(keys: &[RemoteKey]) -> Value {
    keys.iter()
        .map(|k| json!({"action": k.action, "keyval": k.keyval, "name": k.name, "code": k.code}))
        .collect::<Vec<_>>()
        .into()
}

/// A ready-made launcher: (name, address, colour). An empty address means
/// "your own server" (asked for when added).
pub struct Preset {
    pub name: &'static str,
    pub url: &'static str,
    pub color: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Plex",
        url: "https://app.plex.tv/desktop",
        color: "#e5a00d",
    },
    Preset {
        name: "Jellyfin",
        url: "",
        color: "#7b3fb0",
    },
    Preset {
        name: "Emby",
        url: "",
        color: "#52b54b",
    },
    // (your own Frigate NVR: cameras - asks for its address)
    Preset {
        name: "Frigate",
        url: "",
        color: "#1f6feb",
    },
    // (your own Home Assistant - asks for its address)
    Preset {
        name: "Home Assistant",
        url: "",
        color: "#18bcf2",
    },
    Preset {
        name: "Netflix",
        url: "https://www.netflix.com",
        color: "#e50914",
    },
    Preset {
        name: "Hulu",
        url: "https://www.hulu.com",
        color: "#1ce783",
    },
    Preset {
        name: "HBO Max",
        url: "https://play.hbomax.com",
        color: "#002be7",
    },
    Preset {
        name: "Disney+",
        url: "https://www.disneyplus.com",
        color: "#113ccf",
    },
    Preset {
        name: "Prime Video",
        url: "https://www.primevideo.com",
        color: "#00a8e1",
    },
    Preset {
        name: "YouTube",
        url: "https://www.youtube.com/tv",
        color: "#ff0000",
    },
    Preset {
        name: "Paramount+",
        url: "https://www.paramountplus.com",
        color: "#0064ff",
    },
    Preset {
        name: "Peacock",
        url: "https://www.peacocktv.com",
        color: "#222222",
    },
    Preset {
        name: "Crunchyroll",
        url: "https://www.crunchyroll.com",
        color: "#f47521",
    },
    Preset {
        name: "Tubi",
        url: "https://tubitv.com",
        color: "#7408ff",
    },
    Preset {
        name: "Pluto TV",
        url: "https://pluto.tv",
        color: "#fff200",
    },
    Preset {
        name: "Twitch",
        url: "https://www.twitch.tv",
        color: "#9146ff",
    },
    Preset {
        name: "Spotify",
        url: "https://open.spotify.com",
        color: "#1db954",
    },
];

/// Colours offered for a card.
pub const COLORS: &[(&str, &str)] = &[
    ("#e50914", "Red"),
    ("#f47521", "Orange"),
    ("#e5a00d", "Gold"),
    ("#fff200", "Yellow"),
    ("#1ce783", "Green"),
    ("#1db954", "Dark green"),
    ("#00a8e1", "Light blue"),
    ("#0064ff", "Blue"),
    ("#002be7", "Royal blue"),
    ("#7b3fb0", "Purple"),
    ("#9146ff", "Violet"),
    ("#e91e63", "Pink"),
    ("#607d8b", "Slate"),
    ("#222222", "Black"),
];

pub const BROWSERS: &[(&str, &str)] = &[
    ("firefox", "Firefox"),
    ("waterfox", "Waterfox"),
    ("chromium", "Chromium"),
    ("google-chrome", "Google Chrome"),
];

pub const SCALES: &[(&str, &str)] = &[
    ("", "Automatic"),
    ("1", "100%"),
    ("1.25", "125%"),
    ("1.5", "150%"),
    ("1.75", "175%"),
    ("2", "200% (4K TVs)"),
    ("2.5", "250%"),
    ("3", "300%"),
];

pub fn dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"));
    base.join("rusttv")
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn path() -> PathBuf {
    dir().join("rusttv.json")
}

/// A safe id from a name: "HBO Max" -> "hbo-max" (made unique by the caller).
pub fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() {
        "app".into()
    } else {
        s
    }
}

fn str_of(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

/// Is this a usable card colour? (it ends up in CSS)
pub fn valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|h| h.is_ascii_hexdigit())
}

/// The browser new launchers use to start with: Chromium (or Chrome) - it
/// can say it's a TV and move between buttons with the arrows - else Firefox.
pub fn default_browser() -> String {
    ["chromium", "google-chrome"]
        .into_iter()
        .find(|b| crate::launch::installed(b))
        .unwrap_or("chromium")
        .to_string()
}

/// A new launcher, set up for a remote: it says it's a TV and the arrows
/// jump between buttons - except on YouTube's TV page, which is already
/// made for the arrows (jumping would fight it).
pub fn new_launcher(id: String, name: &str, url: &str, color: &str, browser: String) -> Launcher {
    Launcher {
        id,
        name: name.into(),
        url: url.into(),
        browser,
        color: color.into(),
        picture: true,
        tv: true,
        spatial: !url.contains("youtube.com"),
        trust_cert: false,
        keys: Vec::new(),
    }
}

impl Default for Config {
    fn default() -> Self {
        let browser = default_browser();
        let pick = |name: &str| {
            PRESETS
                .iter()
                .find(|p| p.name == name)
                .map(|p| new_launcher(slug(p.name), p.name, p.url, p.color, browser.clone()))
        };
        Config {
            launchers: ["Plex", "YouTube", "Netflix"]
                .iter()
                .filter_map(|n| pick(n))
                .collect(),
            apps: Vec::new(),
            scale: String::new(),
            browser: browser.clone(),
            max_apps: 1,
            auto_suspend: 0,
            keys: Vec::new(),
            own_profile: false,
            accent: "#ffffff".into(),
            background: "dark".into(),
            wallpaper: String::new(),
            battery_show: true,
            battery_warn: 15,
            main_output: String::new(),
            mirror_fit: "fit".into(),
            resolutions: Vec::new(),
            app_keyboard: true,
            pause_on_home: true,
            airplay: true,
            airplay_name: "RustTV".into(),
            airplay_code: "first".into(),
        }
    }
}

impl Config {
    pub fn load() -> Config {
        let Ok(text) = std::fs::read_to_string(path()) else {
            return Config::default();
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            eprintln!(
                "rusttv: {} is not valid - using the defaults",
                path().display()
            );
            return Config::default();
        };
        let mut launchers: Vec<Launcher> = Vec::new();
        for l in v["launchers"].as_array().cloned().unwrap_or_default() {
            let mut id = slug(&str_of(&l, "id"));
            while launchers.iter().any(|o| o.id == id) {
                id.push('2');
            }
            let color = str_of(&l, "color");
            let browser = str_of(&l, "browser");
            launchers.push(Launcher {
                id,
                name: str_of(&l, "name"),
                url: str_of(&l, "url"),
                browser: if BROWSERS.iter().any(|b| b.0 == browser) {
                    browser
                } else {
                    "firefox".into()
                },
                color: if valid_color(&color) {
                    color
                } else {
                    "#607d8b".into()
                },
                picture: l["picture"].as_bool().unwrap_or(true),
                tv: l["tv"].as_bool().unwrap_or(false),
                spatial: l["spatial"].as_bool().unwrap_or(false),
                trust_cert: l["trust_cert"].as_bool().unwrap_or(false),
                keys: parse_keys(&l["keys"]),
            });
        }
        // (the browser for new launchers: Chromium to start with, from 0.7.5)
        let browser = str_of(&v, "new_browser");
        Config {
            launchers,
            apps: v["apps"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|a| a.as_str().map(str::to_string))
                .collect(),
            scale: {
                let s = str_of(&v, "scale");
                if SCALES.iter().any(|x| x.0 == s) {
                    s
                } else {
                    String::new()
                }
            },
            browser: if BROWSERS.iter().any(|b| b.0 == browser) {
                browser
            } else {
                default_browser()
            },
            max_apps: v["max_apps"].as_u64().map(|n| n.min(9) as u32).unwrap_or(1),
            auto_suspend: v["auto_suspend"]
                .as_u64()
                .map(|n| n.min(24 * 60) as u32)
                .unwrap_or(0),
            // ("separate history" is on to start with)
            own_profile: !v["separate_history"].as_bool().unwrap_or(true),
            accent: Some(str_of(&v, "accent"))
                .filter(|c| valid_color(c))
                .unwrap_or_else(|| "#ffffff".into()),
            background: Some(str_of(&v, "background"))
                .filter(|b| BACKGROUNDS.iter().any(|x| x.0 == b))
                .unwrap_or_else(|| "dark".into()),
            wallpaper: str_of(&v, "wallpaper"),
            battery_show: v["battery_show"].as_bool().unwrap_or(true),
            main_output: str_of(&v, "main_output"),
            app_keyboard: v["app_keyboard"].as_bool().unwrap_or(true),
            pause_on_home: v["pause_on_home"].as_bool().unwrap_or(true),
            airplay: v["airplay"].as_bool().unwrap_or(true),
            airplay_name: Some(str_of(&v, "airplay_name"))
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| "RustTV".into()),
            airplay_code: Some(str_of(&v, "airplay_code"))
                .filter(|c| crate::airplay::CODES.iter().any(|x| x.0 == c))
                .unwrap_or_else(|| "first".into()),
            mirror_fit: Some(str_of(&v, "mirror_fit"))
                .filter(|f| ["fit", "fill", "exact"].contains(&f.as_str()))
                .unwrap_or_else(|| "fit".into()),
            resolutions: v["resolutions"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                        .collect()
                })
                .unwrap_or_default(),
            battery_warn: v["battery_warn"]
                .as_u64()
                .map(|n| n.min(50) as u32)
                .unwrap_or(15),
            keys: parse_keys(&v["keys"]),
        }
    }

    pub fn save(&self) {
        let v = json!({
            "launchers": self.launchers.iter().map(|l| json!({
                "id": l.id, "name": l.name, "url": l.url,
                "browser": l.browser, "color": l.color,
                "picture": l.picture, "tv": l.tv, "spatial": l.spatial,
                "trust_cert": l.trust_cert,
                "keys": keys_json(&l.keys),
            })).collect::<Vec<_>>(),
            "max_apps": self.max_apps,
            "auto_suspend": self.auto_suspend,
            "separate_history": !self.own_profile,
            "accent": self.accent,
            "background": self.background,
            "wallpaper": self.wallpaper,
            "battery_show": self.battery_show,
            "battery_warn": self.battery_warn,
            "main_output": self.main_output,
            "app_keyboard": self.app_keyboard,
            "pause_on_home": self.pause_on_home,
            "airplay": self.airplay,
            "airplay_name": self.airplay_name,
            "airplay_code": self.airplay_code,
            "mirror_fit": self.mirror_fit,
            "resolutions": self
                .resolutions
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect::<serde_json::Map<String, Value>>(),
            "keys": keys_json(&self.keys),
            "apps": self.apps,
            "scale": self.scale,
            "new_browser": self.browser,
        });
        let _ = std::fs::create_dir_all(dir());
        let tmp = path().with_extension("json.new");
        let text = serde_json::to_string_pretty(&v).unwrap_or_default();
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path());
        }
    }

    /// An id no launcher has yet, from a name.
    pub fn new_id(&self, name: &str) -> String {
        let base = slug(name);
        let mut id = base.clone();
        let mut n = 2;
        while self.launchers.iter().any(|l| l.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }
}

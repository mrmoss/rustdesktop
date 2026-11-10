//! A launcher's picture: the web service's own icon, found on its page (the
//! big "touch" icon, the icons in its web app manifest, or its favicon) and
//! kept in ~/.cache/rusttv/icons. When none big enough turns up, the card
//! keeps its colour and name.
//!
//! Downloads use curl (or wget), on a worker thread.

use crate::config::Launcher;
use gtk::gdk_pixbuf::Pixbuf;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

/// Smaller than this looks blurry on a TV card.
const MIN_SIZE: i32 = 96;
/// The size it's kept at.
const KEEP_SIZE: i32 = 256;
/// After not finding one, try again after this long.
const RETRY: Duration = Duration::from_secs(24 * 3600);
/// Browser-like, so sites hand over their normal page.
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";

/// Icons known to be good, for services that run on your own server (whose
/// page may be behind a sign-in or a home-made certificate): from the
/// project's own source. (launcher name, picture address)
const KNOWN: &[(&str, &str)] = &[
    (
        "frigate",
        "https://raw.githubusercontent.com/blakeblackshear/frigate/dev/web/public/images/android-chrome-512x512.png",
    ),
    (
        "home assistant",
        "https://raw.githubusercontent.com/home-assistant/frontend/dev/public/static/icons/favicon-512x512.png",
    ),
];

fn known(l: &Launcher) -> Option<&'static str> {
    let name = l.name.to_ascii_lowercase();
    KNOWN.iter().find(|k| name.contains(k.0)).map(|k| k.1)
}

fn dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| crate::config::home().join(".cache"));
    base.join("rusttv").join("icons")
}

fn png(l: &Launcher) -> PathBuf {
    dir().join(format!("{}.png", l.id))
}

/// (which address the picture is for; "" = none was found)
fn note(l: &Launcher) -> PathBuf {
    dir().join(format!("{}.from", l.id))
}

/// The picture to show for a launcher, if there is one for its address.
pub fn picture(l: &Launcher) -> Option<PathBuf> {
    if !l.picture {
        return None;
    }
    let from = std::fs::read_to_string(note(l)).ok()?;
    (from.trim() == l.url.trim() && png(l).is_file()).then(|| png(l))
}

/// Should we go and look for one? (none yet for this address, and not
/// tried and failed in the last day)
pub fn wanted(l: &Launcher) -> bool {
    if !l.picture || l.url.trim().is_empty() {
        return false;
    }
    let Ok(from) = std::fs::read_to_string(note(l)) else {
        return true;
    };
    if from.trim() != l.url.trim() {
        return true;
    }
    if png(l).is_file() {
        return false;
    }
    let age = std::fs::metadata(note(l))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok());
    // (one with a known icon: again after ten minutes)
    if known(l).is_some() && age.is_none_or(|a| a > Duration::from_secs(600)) {
        return true;
    }
    // tried: failed - again after a day
    std::fs::metadata(note(l))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_none_or(|age| age > RETRY)
}

/// Forget it (so it's looked for again).
pub fn forget(l: &Launcher) {
    let _ = std::fs::remove_file(png(l));
    let _ = std::fs::remove_file(note(l));
}

/// Download `url` to `to`. Returns the address it ended up at (after
/// redirects) when known.
fn download(url: &str, to: &Path, max: &str) -> Option<String> {
    if crate::launch::find("curl").is_some() {
        let out = Command::new("curl")
            .args(["-fsSL", "--max-time", "20", "--max-filesize", max, "-A", UA])
            .args(["-H", "Accept-Language: en", "-w", "%{url_effective}", "-o"])
            .arg(to)
            .arg(url)
            .output()
            .ok()?;
        return out
            .status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    let ok = Command::new("wget")
        .args(["-q", "--timeout=20", "-U", UA, "-O"])
        .arg(to)
        .arg(url)
        .status()
        .ok()?
        .success();
    ok.then(|| url.to_string())
}

/// "https://a.b/c/d?e" -> ("https", "https://a.b", "https://a.b/c/")
fn parts(url: &str) -> Option<(String, String, String)> {
    let (scheme, rest) = url.split_once("://")?;
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let origin = format!("{scheme}://{}", &rest[..host_end]);
    let path = &rest[host_end..];
    let path = path.split(['?', '#']).next().unwrap_or("");
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "/",
    };
    Some((scheme.to_string(), origin.clone(), format!("{origin}{dir}")))
}

/// An address on a page, made whole.
fn join(base: &str, href: &str) -> Option<String> {
    let href = href.trim().replace("&amp;", "&");
    if href.is_empty() || href.starts_with("data:") {
        return None;
    }
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href);
    }
    let (scheme, origin, dir) = parts(base)?;
    Some(if let Some(r) = href.strip_prefix("//") {
        format!("{scheme}://{r}")
    } else if href.starts_with('/') {
        format!("{origin}{href}")
    } else {
        format!("{dir}{href}")
    })
}

/// The attributes of an HTML tag's inside (`rel="icon" href=...`).
fn attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = tag.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let s = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'>' {
            i += 1;
        }
        let name = tag[s..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let s = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = tag[s..i.min(b.len())].to_string();
                i += 1;
            } else {
                let s = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                value = tag[s..i].to_string();
            }
        }
        if !name.is_empty() {
            out.push((name, value));
        }
        if s == i {
            i += 1; // (never stuck)
        }
    }
    out
}

/// "192x192 512x512" -> 512 ("any" - a drawing - counts as big).
fn size_of(sizes: &str) -> i32 {
    sizes
        .split_whitespace()
        .map(|s| {
            if s.eq_ignore_ascii_case("any") {
                512
            } else {
                s.split(['x', 'X'])
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0)
            }
        })
        .max()
        .unwrap_or(0)
}

/// The icons a page offers: (address, size it says it is).
fn page_icons(html: &str, base: &str) -> (Vec<(String, i32)>, Option<String>) {
    let mut found = Vec::new();
    let mut manifest = None;
    let lower = html.to_ascii_lowercase();
    let mut at = 0;
    while let Some(i) = lower[at..].find("<link") {
        let s = at + i + 5;
        let e = lower[s..].find('>').map(|e| s + e).unwrap_or(lower.len());
        at = e;
        // (lower and html have the same byte positions: ASCII lower-casing)
        let a = attrs(&html[s..e]);
        let get = |k: &str| {
            a.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let rel = get("rel").to_ascii_lowercase();
        let Some(href) = join(base, &get("href")) else {
            continue;
        };
        if rel.split_whitespace().any(|r| r == "manifest") {
            manifest = Some(href);
        } else if rel.contains("apple-touch-icon") {
            let s = size_of(&get("sizes"));
            found.push((href, if s > 0 { s } else { 180 }));
        } else if rel.split_whitespace().any(|r| r == "icon") {
            let svg = get("type").contains("svg") || href.to_ascii_lowercase().ends_with(".svg");
            let s = size_of(&get("sizes"));
            found.push((
                href,
                if svg {
                    512
                } else if s > 0 {
                    s
                } else {
                    32
                },
            ));
        }
    }
    (found, manifest)
}

fn manifest_icons(text: &str, base: &str) -> Vec<(String, i32)> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    v["icons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| !i["purpose"].as_str().unwrap_or("").contains("monochrome"))
        .filter_map(|i| {
            let src = join(base, i["src"].as_str()?)?;
            Some((src, size_of(i["sizes"].as_str().unwrap_or(""))))
        })
        .collect()
}

/// Download one picture and keep it if it's big enough.
fn try_picture(url: &str, tmp: &Path, keep: &Path) -> bool {
    if download(url, tmp, "4M").is_none() {
        return false;
    }
    let Ok(pb) = Pixbuf::from_file_at_scale(tmp, KEEP_SIZE, KEEP_SIZE, true) else {
        return false;
    };
    // (a drawing (svg) comes out at the size asked for: always big enough)
    let real = Pixbuf::file_info(tmp)
        .map(|(_, w, h)| w.min(h))
        .unwrap_or(0);
    let is_svg = url.to_ascii_lowercase().contains(".svg");
    if !is_svg && real < MIN_SIZE {
        return false;
    }
    on_light_if_dark(&pb).savev(keep, "png", &[]).is_ok()
}

/// A dark logo on a see-through background (Frigate's black bird) would
/// vanish on the card's dark colour: put it on white.
fn on_light_if_dark(pb: &Pixbuf) -> Pixbuf {
    if !pb.has_alpha() || pb.n_channels() != 4 {
        return pb.clone();
    }
    let (w, h, stride) = (
        pb.width() as usize,
        pb.height() as usize,
        pb.rowstride() as usize,
    );
    let bytes = pb.read_pixel_bytes();
    let px: &[u8] = &bytes;
    let (mut clear, mut solid, mut lum) = (0usize, 0usize, 0f64);
    for y in 0..h {
        for x in 0..w {
            let i = y * stride + x * 4;
            let Some(p) = px.get(i..i + 4) else { continue };
            if p[3] < 64 {
                clear += 1;
            } else if p[3] > 128 {
                solid += 1;
                lum += 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64;
            }
        }
    }
    let total = (w * h).max(1);
    let dark = solid > 0 && lum / (solid as f64) < 90.0;
    if clear * 100 / total < 15 || !dark {
        return pb.clone();
    }
    let Some(bg) = Pixbuf::new(
        gtk::gdk_pixbuf::Colorspace::Rgb,
        true,
        8,
        w as i32,
        h as i32,
    ) else {
        return pb.clone();
    };
    bg.fill(0xffff_ffff);
    pb.composite(
        &bg,
        0,
        0,
        w as i32,
        h as i32,
        0.0,
        0.0,
        1.0,
        1.0,
        gtk::gdk_pixbuf::InterpType::Bilinear,
        255,
    );
    bg
}

/// Look for a launcher's picture (blocking: call it from a worker thread).
/// True when one was found.
pub fn fetch(l: &Launcher) -> bool {
    let d = dir();
    if std::fs::create_dir_all(&d).is_err() {
        return false;
    }
    let tmp = d.join(format!("{}.download", l.id));
    let keep = png(l);
    // a known-good icon first
    if let Some(url) = known(l) {
        if try_picture(url, &tmp, &keep) {
            let _ = std::fs::remove_file(&tmp);
            let _ = std::fs::write(note(l), &l.url);
            return true;
        }
    }
    let mut cands: Vec<(String, i32)> = Vec::new();
    let page = d.join(format!("{}.page", l.id));
    if let Some(base) = download(&l.url, &page, "3M") {
        let base = if base.contains("://") {
            base
        } else {
            l.url.clone()
        };
        let html = std::fs::read(&page)
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default();
        let (icons, manifest) = page_icons(&html, &base);
        cands.extend(icons);
        if let Some(m) = manifest {
            if download(&m, &page, "1M").is_some() {
                let text = std::fs::read_to_string(&page).unwrap_or_default();
                cands.extend(manifest_icons(&text, &m));
            }
        }
        if let Some((_, origin, _)) = parts(&base) {
            cands.push((format!("{origin}/apple-touch-icon.png"), 180));
        }
    }
    let _ = std::fs::remove_file(&page);
    // the biggest first
    cands.sort_by(|a, b| b.1.cmp(&a.1));
    cands.dedup_by(|a, b| a.0 == b.0);
    let mut ok = false;
    for (url, size) in cands.iter().take(8) {
        if *size > 0 && *size < MIN_SIZE {
            continue;
        }
        if try_picture(url, &tmp, &keep) {
            ok = true;
            break;
        }
    }
    // last try: a well-known icon service
    if !ok {
        if let Some((_, origin, _)) = parts(&l.url) {
            let host = origin.split("://").nth(1).unwrap_or("");
            let url = format!("https://www.google.com/s2/favicons?domain={host}&sz=256");
            ok = try_picture(&url, &tmp, &keep);
        }
    }
    let _ = std::fs::remove_file(&tmp);
    if !ok {
        let _ = std::fs::remove_file(&keep);
    }
    // (what it was for: found, or tried and not found)
    let _ = std::fs::write(note(l), &l.url);
    ok
}

/// Look for the pictures the launchers still need, then redraw.
/// (launchers whose picture is being looked for right now)
static BUSY: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub fn fetch_missing(launchers: &[Launcher]) {
    let todo: Vec<Launcher> = {
        let Ok(mut busy) = BUSY.lock() else { return };
        let todo: Vec<Launcher> = launchers
            .iter()
            .filter(|l| wanted(l) && !busy.contains(&l.id))
            .cloned()
            .collect();
        busy.extend(todo.iter().map(|l| l.id.clone()));
        todo
    };
    if todo.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let mut any = false;
        for l in &todo {
            any |= fetch(l);
            if let Ok(mut busy) = BUSY.lock() {
                busy.retain(|b| *b != l.id);
            }
        }
        if any {
            crate::on_main(|| {
                crate::with_tv(|t| {
                    if let Some(h) = t.home.borrow().as_ref() {
                        crate::home::rebuild(h);
                    }
                })
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_icons() {
        let html = r#"<html><head>
            <link rel="icon" href="/fav.ico">
            <LINK REL="apple-touch-icon" sizes="180x180" href="https://cdn.x.com/touch.png">
            <link rel=icon type="image/svg+xml" href=logo.svg>
            <link rel="manifest" href="/site.webmanifest">
        </head></html>"#;
        let (icons, manifest) = page_icons(html, "https://www.x.com/app/home");
        assert_eq!(
            manifest.as_deref(),
            Some("https://www.x.com/site.webmanifest")
        );
        assert!(icons.contains(&("https://www.x.com/fav.ico".into(), 32)));
        assert!(icons.contains(&("https://cdn.x.com/touch.png".into(), 180)));
        assert!(icons.contains(&("https://www.x.com/app/logo.svg".into(), 512)));
        let m = r#"{"icons":[{"src":"i/512.png","sizes":"512x512"},{"src":"m.png","sizes":"96x96","purpose":"monochrome"}]}"#;
        assert_eq!(
            manifest_icons(m, "https://www.x.com/site.webmanifest"),
            vec![("https://www.x.com/i/512.png".to_string(), 512)]
        );
        assert_eq!(
            join("https://a.b", "//c.d/e.png").as_deref(),
            Some("https://c.d/e.png")
        );
    }
}

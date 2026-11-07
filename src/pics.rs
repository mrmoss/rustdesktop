//! Window screenshots for recents cards and the group swipe.
//!
//! Screenshots are raw PPM files. A worker thread reads them right after
//! they're taken (plus a half-size copy for small cards); the GTK thread only
//! wraps the pixels as GPU textures, and the GPU does all the scaling. Nothing
//! is decoded or resized on the GTK thread, so cards and swipes stay smooth
//! on slower CPUs.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

/// RGB pixels.
struct Raw {
    w: i32,
    h: i32,
    data: Vec<u8>,
}

impl Raw {
    fn texture(self) -> gdk::Texture {
        let stride = (self.w * 3) as usize;
        gdk::MemoryTexture::new(
            self.w,
            self.h,
            gdk::MemoryFormat::R8g8b8,
            &glib::Bytes::from_owned(self.data),
            stride,
        )
        .upcast()
    }

    /// Half size, each pixel the average of 2x2 (a clean downscale).
    fn half(&self) -> Raw {
        let (w, h) = ((self.w / 2).max(1), (self.h / 2).max(1));
        let sw = self.w as usize * 3;
        let mut out = vec![0u8; (w * h * 3) as usize];
        for y in 0..h as usize {
            let r0 = (y * 2) * sw;
            let r1 = ((y * 2 + 1).min(self.h as usize - 1)) * sw;
            for x in 0..w as usize {
                let c0 = x * 2 * 3;
                let c1 = ((x * 2 + 1).min(self.w as usize - 1)) * 3;
                for k in 0..3 {
                    let s = u16::from(self.data[r0 + c0 + k])
                        + u16::from(self.data[r0 + c1 + k])
                        + u16::from(self.data[r1 + c0 + k])
                        + u16::from(self.data[r1 + c1 + k]);
                    out[(y * w as usize + x) * 3 + k] = (s / 4) as u8;
                }
            }
        }
        Raw { w, h, data: out }
    }
}

fn read_ppm(path: &Path) -> Option<Raw> {
    let bytes = std::fs::read(path).ok()?;
    if !bytes.starts_with(b"P6") {
        return None;
    }
    // header: P6 <width> <height> <maxval>, then one whitespace byte
    let mut fields = Vec::new();
    let mut i = 2;
    while fields.len() < 3 && i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        fields.push(
            std::str::from_utf8(&bytes[start..i])
                .ok()?
                .parse::<i32>()
                .ok()?,
        );
    }
    let (w, h, max) = (*fields.first()?, *fields.get(1)?, *fields.get(2)?);
    if max != 255 || w <= 0 || h <= 0 {
        return None;
    }
    let start = i + 1;
    let len = (w as usize) * (h as usize) * 3;
    let data = bytes.get(start..start + len)?.to_vec();
    Some(Raw { w, h, data })
}

struct Entry {
    path: PathBuf,
    full: gdk::Texture,
    half: gdk::Texture,
}

thread_local! {
    static CACHE: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

const KEEP: usize = 16;

fn remember(path: PathBuf, full: Raw, half: Raw) {
    let e = Entry {
        path,
        full: full.texture(),
        half: half.texture(),
    };
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.retain(|x| x.path != e.path && x.path.exists());
        if c.len() >= KEEP {
            c.remove(0);
        }
        c.push(e);
    });
}

/// (full, half) textures, reading the file now if the worker hasn't yet.
fn textures(path: &Path) -> Option<(gdk::Texture, gdk::Texture)> {
    let hit = CACHE.with(|c| {
        c.borrow()
            .iter()
            .find(|e| e.path == path)
            .map(|e| (e.full.clone(), e.half.clone()))
    });
    if hit.is_some() {
        return hit;
    }
    let raw = read_ppm(path)?;
    let half = raw.half();
    remember(path.to_path_buf(), raw, half);
    textures(path)
}

/// Read new screenshots on a worker thread, so showing them is instant.
pub fn prefetch(paths: Vec<PathBuf>) {
    let todo: Vec<PathBuf> = paths
        .into_iter()
        .filter(|p| !CACHE.with(|c| c.borrow().iter().any(|e| &e.path == p)))
        .collect();
    if todo.is_empty() {
        return;
    }
    glib::spawn_future_local(async move {
        let loaded = gio::spawn_blocking(move || {
            todo.into_iter()
                .filter_map(|p| {
                    let raw = read_ppm(&p)?;
                    let half = raw.half();
                    Some((p, raw, half))
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        for (p, raw, half) in loaded {
            remember(p, raw, half);
        }
    });
}

/// A w x h picture of the screenshot, filled edge to edge (cropping a little
/// if its shape differs). Scaled by the GPU.
pub fn widget(path: Option<PathBuf>, w: i32, h: i32) -> Option<gtk::Widget> {
    let (full, half) = textures(&path?)?;
    let scale = crate::shell::shell_monitor()
        .map(|m| m.scale_factor())
        .unwrap_or(1)
        .max(1);
    // small cards use the half-size copy: less shrinking = smoother edges
    let tex = if w * scale <= half.width() && h * scale <= half.height() {
        half
    } else {
        full
    };
    let pic = gtk::Picture::for_paintable(&tex);
    pic.set_content_fit(gtk::ContentFit::Cover);
    pic.set_can_shrink(true);
    pic.set_can_target(false);
    // A Picture asks for its texture's full pixel size, which made the cards
    // around it far too tall. This frame asks for exactly w x h (and hands
    // the Picture exactly that).
    let holder = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(w)
        .min_content_height(h)
        .max_content_width(w)
        .max_content_height(h)
        .propagate_natural_width(false)
        .propagate_natural_height(false)
        .child(&pic)
        .build();
    holder.set_size_request(w, h);
    holder.set_overflow(gtk::Overflow::Hidden);
    // (its scroll bars never show; the theme's sizes for them only made GTK
    // complain in the log: "GtkGizmo (slider) reported min width -2")
    holder.add_css_class("pic-holder");
    holder.set_can_target(false);
    Some(holder.upcast())
}

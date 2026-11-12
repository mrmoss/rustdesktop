//! The volume pop-up: shown over everything (the app on screen too) for a
//! moment when the volume changes - how loud, or muted.
//!
//! Over other apps it needs gtk4-layer-shell (RustTV built with the `layer`
//! feature, which install-rusttv.sh does when the library is there).
//! Without it the volume shows as a message on RustTV's own screen.

use crate::Tv;
use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

pub struct Osd {
    win: gtk::Window,
    icon: gtk::Label,
    bar: gtk::LevelBar,
    pct: gtk::Label,
    timer: RefCell<Option<glib::SourceId>>,
}

/// The pop-up window, if it can float over other apps here.
pub fn build(app: &gtk::Application) -> Option<Rc<Osd>> {
    #[cfg(feature = "layer")]
    {
        use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
        if !gtk4_layer_shell::is_supported() {
            return None;
        }
        let win = gtk::Window::builder()
            .application(app)
            .title("RustTV volume")
            .build();
        win.add_css_class("osd-window");
        win.init_layer_shell();
        win.set_namespace(Some("rusttv-osd"));
        win.set_layer(Layer::Overlay);
        // (at the top: subtitles are at the bottom)
        win.set_anchor(Edge::Top, true);
        win.set_margin(Edge::Top, 40);
        win.set_keyboard_mode(KeyboardMode::None);
        win.set_exclusive_zone(-1);
        win.set_can_target(false);

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 18);
        row.add_css_class("osd");
        let icon = gtk::Label::new(Some("🔊"));
        icon.add_css_class("osd-icon");
        let bar = gtk::LevelBar::for_interval(0.0, 1.0);
        bar.set_mode(gtk::LevelBarMode::Continuous);
        bar.set_size_request(360, 14);
        bar.set_valign(gtk::Align::Center);
        // (no colour change near full)
        bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_LOW));
        bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_HIGH));
        bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_FULL));
        let pct = gtk::Label::new(Some("0%"));
        pct.add_css_class("osd-pct");
        pct.set_width_chars(5);
        pct.set_xalign(1.0);
        row.append(&icon);
        row.append(&bar);
        row.append(&pct);
        win.set_child(Some(&row));
        return Some(Rc::new(Osd {
            win,
            icon,
            bar,
            pct,
            timer: RefCell::new(None),
        }));
    }
    #[allow(unreachable_code)]
    {
        let _ = app;
        None
    }
}

impl Osd {
    /// A short message instead of the volume (a low battery).
    fn text(self: &Rc<Self>, icon: &str, text: &str) {
        self.icon.set_text(icon);
        self.bar.set_visible(false);
        self.pct.set_width_chars(-1);
        self.pct.set_text(text);
        self.win.remove_css_class("muted");
        self.pop(4000);
    }

    fn pop(self: &Rc<Self>, ms: u64) {
        self.win.set_visible(true);
        if let Some(t) = self.timer.borrow_mut().take() {
            t.remove();
        }
        let me = self.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(ms), move || {
            me.timer.borrow_mut().take();
            me.win.set_visible(false);
        });
        *self.timer.borrow_mut() = Some(id);
    }

    fn show(self: &Rc<Self>, level: f64, muted: bool) {
        self.bar.set_visible(true);
        self.pct.set_width_chars(5);
        self.icon.set_text(if muted {
            "🔇"
        } else if level < 0.01 {
            "🔈"
        } else if level < 0.5 {
            "🔉"
        } else {
            "🔊"
        });
        // (up to 150% is allowed: the bar shows 0..100, the number the rest)
        self.bar.set_value(if muted { 0.0 } else { level.min(1.0) });
        self.pct.set_text(&if muted {
            "Muted".to_string()
        } else {
            format!("{:.0}%", level * 100.0)
        });
        if muted {
            self.win.add_css_class("muted");
        } else {
            self.win.remove_css_class("muted");
        }
        self.pop(1600);
    }
}

/// "Volume: 0.45 [MUTED]" -> (0.45, true)
fn parse(out: &str) -> Option<(f64, bool)> {
    let rest = out.trim().strip_prefix("Volume:")?.trim();
    let level: f64 = rest.split_whitespace().next()?.parse().ok()?;
    Some((level, rest.contains("MUTED")))
}

/// Change the volume ("volume-up", "volume-down", "mute") and show it.
pub fn volume(action: &str) {
    let args: Vec<&str> = match action {
        "volume-up" => vec!["set-volume", "-l", "1.5", "@DEFAULT_AUDIO_SINK@", "5%+"],
        "volume-down" => vec!["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"],
        _ => vec!["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"],
    };
    let args: Vec<String> = args.into_iter().map(str::to_string).collect();
    let unmute = action != "mute";
    std::thread::spawn(move || {
        let _ = Command::new("wpctl").args(&args).status();
        // (turning it up or down while muted: unmute, like a TV)
        if unmute {
            let _ = Command::new("wpctl")
                .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "0"])
                .status();
        }
        let now = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
            .ok()
            .and_then(|o| parse(&String::from_utf8_lossy(&o.stdout)));
        let Some((level, muted)) = now else { return };
        crate::on_main(move || crate::with_tv(|t| shown(t, level, muted)));
    });
}

/// A message over everything for a moment (on RustTV's own screen when it
/// can't float over apps).
pub fn message(t: &Tv, icon: &str, text: &str) {
    match t.osd.as_ref() {
        Some(o) => o.text(icon, text),
        None => t.toast(&format!("{icon} {text}")),
    }
}

fn shown(t: &Tv, level: f64, muted: bool) {
    // (the Sound settings, if open, show it too)
    crate::netui::volume_changed(level, muted);
    match t.osd.as_ref() {
        Some(o) => o.show(level, muted),
        None => t.toast(&if muted {
            "Volume: muted".to_string()
        } else {
            format!("Volume: {:.0}%", level * 100.0)
        }),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_wpctl() {
        assert_eq!(super::parse("Volume: 0.45\n"), Some((0.45, false)));
        assert_eq!(super::parse("Volume: 1.00 [MUTED]\n"), Some((1.0, true)));
        assert_eq!(super::parse("nope"), None);
    }
}

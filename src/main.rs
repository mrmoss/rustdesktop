//! rusttouch - a deliberately tiny Android-style shell.
//!
//! Surfaces (wlroots compositors with layer-shell; phoc is the default):
//!   * home      - Top layer, shown/hidden over apps (like phosh)
//!   * top bar   - Top layer, clock + status icons, swipe down => quick settings
//!   * nav bar   - Top layer, back / home / recents, swipe up => app drawer
//!   * sheets    - Overlay layer: app drawer, quick settings, recents; follow your finger
//!   * lock      - Overlay layer, exclusive keyboard, built-in keyboard + PAM
//!
//! Without layer-shell (GNOME/mutter, X11, or RUSTTOUCH_WINDOWED=1) everything runs
//! inside one phone-sized window so you can still poke at it.

mod anim;
mod apps;
mod audio;
mod autostart;
mod bars;
mod clock;
mod config;
mod display;
mod drawer;
mod gestures;
mod home;
mod icons;
mod layer;
mod lock;
mod netlist;
mod osd;
mod pam;
mod power;
mod quick;
mod recents;
mod screens;
mod sensors;
mod sheet;
mod shell;
mod slide;
mod sway;
mod sys;
mod theme;
mod tiling;
mod tray;
mod wl;

use gtk::glib;
use gtk::prelude::*;

/// `rusttouch --do <action>`: ask the running shell to do something (used by
/// touchpad gesture bindings; handy for your own key bindings or scripts too).
fn send_action(action: &str) -> glib::ExitCode {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = shell::ctl_path();
    // non-blocking: fails straight away if rusttouch isn't running
    match std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(0o4000) // O_NONBLOCK
        .open(&path)
    {
        Ok(mut f) => {
            let _ = writeln!(f, "{action}");
            glib::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("rusttouch isn't running ({}: {e})", path.display());
            glib::ExitCode::FAILURE
        }
    }
}

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--do") {
        return send_action(args.get(2).map(String::as_str).unwrap_or(""));
    }
    let app = gtk::Application::builder()
        .application_id("dev.rusttouch.Shell")
        .build();

    app.connect_activate(|app| {
        // A second launch just re-activates the first instance; don't build twice.
        if !app.windows().is_empty() {
            return;
        }
        let cfg = config::Config::load();
        theme::apply(&cfg);
        clock::set_prefs(&cfg);
        let shell = shell::build(app, cfg);
        // Keep the shell alive for the life of the process.
        std::mem::forget(shell);
    });
    // mirrored screens are separate programs: stop them with us
    app.connect_shutdown(|_| screens::stop());

    app.run()
}

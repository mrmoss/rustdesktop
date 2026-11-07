//! .desktop app discovery + launching.

use gtk::prelude::*;
use gtk::{gdk, gio};

use crate::shell::Shell;

pub fn all_apps() -> Vec<gio::AppInfo> {
    let mut v: Vec<gio::AppInfo> = gio::AppInfo::all()
        .into_iter()
        .filter(|a| a.should_show())
        .collect();
    v.sort_by_key(|a| a.display_name().to_lowercase());
    v
}

/// Icon for a running window's app_id (Wayland app_id is usually the
/// desktop file name without ".desktop").
pub fn icon_for_app_id(app_id: &str) -> Option<gio::Icon> {
    if app_id.is_empty() {
        return None;
    }
    for id in [
        format!("{app_id}.desktop"),
        format!("{}.desktop", app_id.to_lowercase()),
    ] {
        if let Some(info) = gio::DesktopAppInfo::new(&id) {
            if let Some(i) = info.icon() {
                return Some(i);
            }
        }
    }
    let names = [app_id, "application-x-executable"];
    Some(gio::ThemedIcon::from_names(&names).upcast())
}

pub fn icon_image(icon: Option<&gio::Icon>, px: i32) -> gtk::Image {
    let img = match icon {
        Some(i) => gtk::Image::from_gicon(i),
        None => gtk::Image::from_icon_name("application-x-executable"),
    };
    img.set_pixel_size(px);
    img
}

/// Icon + label button used in the drawer and the dock.
pub fn tile(info: &gio::AppInfo, icon_px: i32) -> gtk::Button {
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let icon = info.icon();
    vbox.append(&icon_image(icon.as_ref(), icon_px));

    let label = gtk::Label::new(Some(info.display_name().as_str()));
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(10);
    label.set_justify(gtk::Justification::Center);
    vbox.append(&label);

    let b = gtk::Button::builder().child(&vbox).build();
    b.add_css_class("tile");
    b
}

pub fn launch(shell: &Shell, info: &gio::AppInfo) {
    let ctx = gdk::Display::default().map(|d| d.app_launch_context());
    if let Some(c) = &ctx {
        // these are only for rusttouch itself (see session/rusttouch-shell)
        c.unsetenv("GDK_DEBUG");
        c.unsetenv("GTK_A11Y");
        c.unsetenv("GTK_USE_PORTAL");
    }
    eprintln!(
        "rusttouch: [{:.1}s] launching {}",
        crate::shell::uptime(),
        info.display_name()
    );
    shell.before_launch();
    match info.launch(&[], ctx.as_ref()) {
        Ok(()) => shell.launching(),
        Err(e) => eprintln!("rusttouch: failed to launch {}: {e}", info.display_name()),
    }
}

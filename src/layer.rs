//! Thin wrapper around gtk4-layer-shell so the rest of the code doesn't care.
//! With the `layer` cargo feature off, everything here is a no-op and rusttouch
//! always runs in windowed (phone emulator) mode.

// Without the feature, setup() ignores its arguments, so their fields look unused.
#![cfg_attr(not(feature = "layer"), allow(dead_code))]

#[derive(Clone, Copy)]
pub enum Layer {
    /// below app windows (sway home screen: shows on an empty workspace)
    Bottom,
    Top,
    Overlay,
}

#[derive(Clone, Copy)]
pub enum KeyboardMode {
    None,
    /// Grab the keyboard (lock screen).
    Exclusive,
}

#[derive(Clone, Copy)]
pub struct Anchors {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl Anchors {
    pub const ALL: Anchors = Anchors {
        top: true,
        bottom: true,
        left: true,
        right: true,
    };
    pub const TOP: Anchors = Anchors {
        top: true,
        bottom: false,
        left: true,
        right: true,
    };
    pub const BOTTOM: Anchors = Anchors {
        top: false,
        bottom: true,
        left: true,
        right: true,
    };
}

pub enum Zone {
    /// Reserve exactly as much space as the surface is tall (bars).
    Auto,
    /// 0 = stay out of the bars' way, -1 = cover everything (lock screen).
    Fixed(i32),
}

/// True if we should use layer-shell surfaces (wlroots-style compositor).
/// Set RUSTTOUCH_WINDOWED=1 to force the phone-emulator window instead.
#[cfg(feature = "layer")]
pub fn supported() -> bool {
    if std::env::var_os("RUSTTOUCH_WINDOWED").is_some() {
        return false;
    }
    gtk4_layer_shell::is_supported()
}

#[cfg(not(feature = "layer"))]
pub fn supported() -> bool {
    false
}

#[cfg(feature = "layer")]
pub fn setup(win: &gtk::ApplicationWindow, layer: Layer, a: Anchors, zone: Zone, kb: KeyboardMode) {
    use gtk4_layer_shell::{Edge, LayerShell};

    win.init_layer_shell();
    win.set_layer(match layer {
        Layer::Bottom => gtk4_layer_shell::Layer::Bottom,
        Layer::Top => gtk4_layer_shell::Layer::Top,
        Layer::Overlay => gtk4_layer_shell::Layer::Overlay,
    });
    win.set_anchor(Edge::Top, a.top);
    win.set_anchor(Edge::Bottom, a.bottom);
    win.set_anchor(Edge::Left, a.left);
    win.set_anchor(Edge::Right, a.right);
    match zone {
        Zone::Auto => win.auto_exclusive_zone_enable(),
        Zone::Fixed(z) => win.set_exclusive_zone(z),
    }
    win.set_keyboard_mode(match kb {
        KeyboardMode::None => gtk4_layer_shell::KeyboardMode::None,
        KeyboardMode::Exclusive => gtk4_layer_shell::KeyboardMode::Exclusive,
    });
}

/// Move a surface to another layer after creation.
#[cfg(feature = "layer")]
pub fn set_layer(win: &gtk::ApplicationWindow, layer: Layer) {
    use gtk4_layer_shell::LayerShell;
    LayerShell::set_layer(
        win,
        match layer {
            Layer::Bottom => gtk4_layer_shell::Layer::Bottom,
            Layer::Top => gtk4_layer_shell::Layer::Top,
            Layer::Overlay => gtk4_layer_shell::Layer::Overlay,
        },
    );
}

#[cfg(not(feature = "layer"))]
pub fn set_layer(_: &gtk::ApplicationWindow, _: Layer) {}

/// Distance from the edges (top, right, bottom, left) for small popups.
#[cfg(feature = "layer")]
pub fn set_margins(win: &gtk::ApplicationWindow, top: i32, right: i32, bottom: i32, left: i32) {
    use gtk4_layer_shell::{Edge, LayerShell};
    LayerShell::set_margin(win, Edge::Top, top);
    LayerShell::set_margin(win, Edge::Right, right);
    LayerShell::set_margin(win, Edge::Bottom, bottom);
    LayerShell::set_margin(win, Edge::Left, left);
}

/// Change the exclusive zone after creation (auto-hiding bars).
#[cfg(feature = "layer")]
pub fn set_zone(win: &gtk::ApplicationWindow, zone: Zone) {
    use gtk4_layer_shell::LayerShell;
    match zone {
        Zone::Auto => win.auto_exclusive_zone_enable(),
        Zone::Fixed(z) => win.set_exclusive_zone(z),
    }
}

#[cfg(not(feature = "layer"))]
pub fn set_zone(_: &gtk::ApplicationWindow, _: Zone) {}

/// Put the surface on a specific monitor (None = compositor's choice).
#[cfg(feature = "layer")]
pub fn set_monitor(win: &gtk::ApplicationWindow, monitor: Option<&gtk::gdk::Monitor>) {
    gtk4_layer_shell::LayerShell::set_monitor(win, monitor);
}

#[cfg(not(feature = "layer"))]
pub fn set_monitor(_: &gtk::ApplicationWindow, _: Option<&gtk::gdk::Monitor>) {}

#[cfg(not(feature = "layer"))]
pub fn set_margins(_: &gtk::ApplicationWindow, _: i32, _: i32, _: i32, _: i32) {}

#[cfg(not(feature = "layer"))]
pub fn setup(_: &gtk::ApplicationWindow, _: Layer, _: Anchors, _: Zone, _: KeyboardMode) {}

//! rusttouch's own Wayland connection (separate from GTK's), on a thread:
//!   * window list   - wlr-foreign-toplevel-management (recents, switch, close)
//!   * screen on/off - wlr-output-power-management (screen blanking)
//!   * text focus    - input-method-v2 (auto-show the on-screen keyboard)
//! Events go to the GTK thread over a channel; requests are sent straight
//! from the GTK thread (wayland-client proxies are thread-safe).

use std::os::fd::{AsFd, FromRawFd};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_output, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_manager_v2 as im_manager, zwp_input_method_v2 as im,
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1 as vk_manager, zwp_virtual_keyboard_v1 as vk,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1 as handle, zwlr_foreign_toplevel_manager_v1 as manager,
};
use wayland_protocols_wlr::output_power_management::v1::client::{
    zwlr_output_power_manager_v1 as power_manager, zwlr_output_power_v1 as power,
};

use crate::shell::Event;

#[derive(Clone, Debug)]
pub struct Toplevel {
    /// Stable id for the life of the window (Wayland object id).
    pub key: u32,
    pub handle: handle::ZwlrForeignToplevelHandleV1,
    pub title: String,
    pub app_id: String,
    pub activated: bool,
    pub minimized: bool,
    pub fullscreen: bool,
}

pub struct Client {
    conn: Connection,
    qh: QueueHandle<State>,
    seat: Option<wl_seat::WlSeat>,
    pub has_toplevels: bool,
    power: Option<power_manager::ZwlrOutputPowerManagerV1>,
    outputs: Vec<wl_output::WlOutput>,
    pub has_input_method: bool,
    /// the RustTouch keyboard types through these
    vkbd: Option<vk::ZwpVirtualKeyboardV1>,
    im: Option<im::ZwpInputMethodV2>,
    im_serial: Arc<AtomicU32>,
    _keymap: Option<std::fs::File>,
}

impl Client {
    pub fn activate(&self, t: &Toplevel) {
        if t.minimized {
            t.handle.unset_minimized();
        }
        if let Some(seat) = &self.seat {
            t.handle.activate(seat);
        }
        let _ = self.conn.flush();
    }

    /// Re-maximize a window so it fits the screen space that's free now (the
    /// keyboard took or gave back room; phoc doesn't refit windows by itself).
    pub fn refit(&self, t: &Toplevel) {
        t.handle.unset_maximized();
        t.handle.set_maximized();
        let _ = self.conn.flush();
    }

    pub fn close(&self, t: &Toplevel) {
        t.handle.close();
        let _ = self.conn.flush();
    }

    /// Press and release a key (evdev code) with modifiers held (xkb mask).
    pub fn vk_key(&self, code: u32, mods: u32) {
        let Some(k) = &self.vkbd else { return };
        let t = (glib_ms() & 0xffff_ffff) as u32;
        if mods != 0 {
            k.modifiers(mods, 0, 0, 0);
        }
        k.key(t, code, 1);
        k.key(t.wrapping_add(1), code, 0);
        if mods != 0 {
            k.modifiers(0, 0, 0, 0);
        }
        let _ = self.conn.flush();
    }

    /// Type text into the focused text field (emoji and other characters
    /// that aren't on the keyboard map).
    pub fn commit_text(&self, text: &str) {
        let Some(m) = &self.im else { return };
        m.commit_string(text.to_string());
        m.commit(self.im_serial.load(Ordering::SeqCst));
        let _ = self.conn.flush();
    }

    /// Turn all screens on/off. false if the compositor can't.
    pub fn set_screens(&self, on: bool) -> bool {
        let Some(mgr) = &self.power else {
            return false;
        };
        let mode = if on {
            power::Mode::On
        } else {
            power::Mode::Off
        };
        for o in &self.outputs {
            let p = mgr.get_output_power(o, &self.qh, ());
            p.set_mode(mode);
            p.destroy();
        }
        let _ = self.conn.flush();
        !self.outputs.is_empty()
    }
}

pub struct State {
    im_serial: Arc<AtomicU32>,
    items: Vec<Toplevel>,
    tx: Sender<Event>,
    im_pending: bool,
    im_active: bool,
    /// an activate arrived in this batch (also when already active: GTK
    /// re-sends it when you tap a focused field, meaning "show the keyboard")
    im_activated: bool,
}

impl State {
    fn publish(&self) {
        let _ = self.tx.send(Event::Toplevels(self.items.clone()));
        crate::shell::wake();
    }
}

/// `want_im`: bind input-method-v2 (only when *we* drive the keyboard; an
/// OSK like squeekboard binds it itself and there can only be one).
fn glib_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A US keyboard map for the virtual keyboard (the compositor fills in the
/// details from the system's keyboard data).
const KEYMAP: &str = "xkb_keymap {\n\
  xkb_keycodes { include \"evdev+aliases(qwerty)\" };\n\
  xkb_types { include \"complete\" };\n\
  xkb_compat { include \"complete\" };\n\
  xkb_symbols { include \"pc+us+inet(evdev)\" };\n\
};\n";

extern "C" {
    fn memfd_create(name: *const std::ffi::c_char, flags: u32) -> i32;
}

fn keymap_file() -> Option<std::fs::File> {
    use std::io::Write;
    let fd = unsafe { memfd_create(c"rusttouch-keymap".as_ptr(), 0) };
    if fd < 0 {
        return None;
    }
    let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
    f.write_all(KEYMAP.as_bytes()).ok()?;
    f.write_all(&[0]).ok()?;
    Some(f)
}

/// `want_kbd`: also create a virtual keyboard (the RustTouch keyboard).
pub fn start(tx: Sender<Event>, want_im: bool, want_kbd: bool) -> Option<Client> {
    let conn = Connection::connect_to_env().ok()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn).ok()?;
    let qh = queue.handle();

    let seat: Option<wl_seat::WlSeat> = globals.bind(&qh, 1..=1, ()).ok();
    let mgr: Option<manager::ZwlrForeignToplevelManagerV1> = globals.bind(&qh, 1..=3, ()).ok();
    let power: Option<power_manager::ZwlrOutputPowerManagerV1> = globals.bind(&qh, 1..=1, ()).ok();

    let outputs: Vec<wl_output::WlOutput> = globals
        .contents()
        .clone_list()
        .into_iter()
        .filter(|g| g.interface == "wl_output")
        .map(|g| {
            globals
                .registry()
                .bind::<wl_output::WlOutput, _, _>(g.name, g.version.min(3), &qh, ())
        })
        .collect();

    let mut input_method = None;
    if want_im {
        let im_mgr: Option<im_manager::ZwpInputMethodManagerV2> = globals.bind(&qh, 1..=1, ()).ok();
        if let (Some(m), Some(s)) = (&im_mgr, &seat) {
            input_method = Some(m.get_input_method(s, &qh, ()));
        }
    }

    let mut vkbd = None;
    let mut keymap = None;
    if want_kbd {
        let vm: Option<vk_manager::ZwpVirtualKeyboardManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
        if let (Some(m), Some(s)) = (&vm, &seat) {
            let k = m.create_virtual_keyboard(s, &qh, ());
            if let Some(f) = keymap_file() {
                k.keymap(1, f.as_fd(), (KEYMAP.len() + 1) as u32); // 1 = XKB v1
                keymap = Some(f);
            }
            vkbd = Some(k);
        } else {
            eprintln!("rusttouch: the compositor has no virtual-keyboard support - the keyboard can't type");
        }
    }
    let im_serial = Arc::new(AtomicU32::new(0));

    let client = Client {
        conn: conn.clone(),
        qh: qh.clone(),
        seat,
        has_toplevels: mgr.is_some(),
        power,
        outputs,
        has_input_method: input_method.is_some(),
        vkbd,
        im: input_method.clone(),
        im_serial: im_serial.clone(),
        _keymap: keymap,
    };
    let mut state = State {
        im_serial,
        items: Vec::new(),
        tx,
        im_pending: false,
        im_active: false,
        im_activated: false,
    };
    std::thread::Builder::new()
        .name("rusttouch-wayland".into())
        .spawn(move || {
            let _keep = (mgr, input_method);
            while queue.blocking_dispatch(&mut state).is_ok() {}
            eprintln!("rusttouch: wayland connection closed");
        })
        .ok()?;
    Some(client)
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_output::WlOutput,
        _: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<power_manager::ZwlrOutputPowerManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &power_manager::ZwlrOutputPowerManagerV1,
        _: power_manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<power::ZwlrOutputPowerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &power::ZwlrOutputPowerV1,
        _: power::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<im_manager::ZwpInputMethodManagerV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &im_manager::ZwpInputMethodManagerV2,
        _: im_manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<vk_manager::ZwpVirtualKeyboardManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &vk_manager::ZwpVirtualKeyboardManagerV1,
        _: vk_manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<vk::ZwpVirtualKeyboardV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &vk::ZwpVirtualKeyboardV1,
        _: vk::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<im::ZwpInputMethodV2, ()> for State {
    fn event(
        this: &mut Self,
        _: &im::ZwpInputMethodV2,
        event: im::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            im::Event::Activate => {
                this.im_pending = true;
                this.im_activated = true;
            }
            im::Event::Deactivate => this.im_pending = false,
            im::Event::Done => {
                this.im_serial.fetch_add(1, Ordering::SeqCst);
                let reshow = this.im_activated && this.im_pending;
                this.im_activated = false;
                if this.im_pending != this.im_active || reshow {
                    this.im_active = this.im_pending;
                    let _ = this.tx.send(Event::TextInput(this.im_active));
                    crate::shell::wake();
                }
            }
            im::Event::Unavailable => {
                eprintln!(
                    "rusttouch: another input method is running - keyboard auto-show disabled"
                );
            }
            _ => {}
        }
    }
}

impl Dispatch<manager::ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        this: &mut Self,
        _: &manager::ZwlrForeignToplevelManagerV1,
        event: manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let manager::Event::Toplevel { toplevel } = event {
            this.items.push(Toplevel {
                key: toplevel.id().protocol_id(),
                handle: toplevel,
                title: String::new(),
                app_id: String::new(),
                activated: false,
                minimized: false,
                fullscreen: false,
            });
        }
    }

    wayland_client::event_created_child!(State, manager::ZwlrForeignToplevelManagerV1, [
        manager::EVT_TOPLEVEL_OPCODE => (handle::ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<handle::ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(
        this: &mut Self,
        proxy: &handle::ZwlrForeignToplevelHandleV1,
        event: handle::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = proxy.id();
        let Some(i) = this.items.iter().position(|t| t.handle.id() == id) else {
            return;
        };
        match event {
            handle::Event::Title { title } => this.items[i].title = title,
            handle::Event::AppId { app_id } => this.items[i].app_id = app_id,
            handle::Event::State { state } => {
                let states: Vec<u32> = state
                    .chunks_exact(4)
                    .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                // 1 = minimized, 2 = activated, 3 = fullscreen (protocol XML)
                this.items[i].minimized = states.contains(&1);
                this.items[i].activated = states.contains(&2);
                this.items[i].fullscreen = states.contains(&3);
            }
            handle::Event::Done => this.publish(),
            handle::Event::Closed => {
                let t = this.items.remove(i);
                t.handle.destroy();
                this.publish();
            }
            _ => {}
        }
    }
}

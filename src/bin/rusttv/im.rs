//! Typing into apps (a sign-in box on a web page): RustTV is the system's
//! input method (input-method-v2), so it hears when a text box gets the
//! focus. While its keyboard shows, it holds the keyboard (a keyboard grab):
//! the remote's arrows and OK work its keys instead of the page - and keys
//! from a real keyboard are passed on to the app (through a virtual
//! keyboard with the same key map), putting the on-screen one away.
//!
//! Its own Wayland connection, on a thread; events to the GTK thread over a
//! channel. (Like RustTouch's wl.rs.)

use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_keyboard_grab_v2 as grab, zwp_input_method_manager_v2 as im_manager,
    zwp_input_method_v2 as im,
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1 as vk_manager, zwp_virtual_keyboard_v1 as vk,
};

pub enum Event {
    /// a text box got the focus (true) or lost it (false)
    Text(bool),
    /// what's in the text box now (when the app says)
    Surrounding(String),
    /// a key pressed (true) or let go, while the keyboard is held: (evdev
    /// code, time)
    Key(u32, bool, u32),
}

pub struct Client {
    conn: Connection,
    qh: QueueHandle<State>,
    im: im::ZwpInputMethodV2,
    vkbd: Option<vk::ZwpVirtualKeyboardV1>,
    serial: Arc<AtomicU32>,
    grab: Arc<Mutex<Option<grab::ZwpInputMethodKeyboardGrabV2>>>,
    _keymap: Option<std::fs::File>,
}

/// A US key map for the virtual keyboard until the real keyboard's arrives
/// (the compositor fills in the details).
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
    let fd = unsafe { memfd_create(c"rusttv-keymap".as_ptr(), 0) };
    if fd < 0 {
        return None;
    }
    let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
    f.write_all(KEYMAP.as_bytes()).ok()?;
    f.write_all(&[0]).ok()?;
    Some(f)
}

impl Client {
    fn flush(&self) {
        let _ = self.conn.flush();
    }

    /// Hold the keyboard (its keys come to us) - or let it go.
    pub fn hold_keyboard(&self, on: bool) {
        let mut g = self.grab.lock().unwrap_or_else(|e| e.into_inner());
        match (on, g.is_some()) {
            (true, false) => *g = Some(self.im.grab_keyboard(&self.qh, ())),
            (false, true) => {
                if let Some(x) = g.take() {
                    x.release();
                }
            }
            _ => {}
        }
        drop(g);
        self.flush();
    }

    /// Type text into the text box.
    pub fn type_text(&self, text: &str) {
        self.im.commit_string(text.to_string());
        self.im.commit(self.serial.load(Ordering::SeqCst));
        self.flush();
    }

    /// Delete the character before the cursor.
    pub fn backspace(&self) {
        self.im.delete_surrounding_text(1, 0);
        self.im.commit(self.serial.load(Ordering::SeqCst));
        self.flush();
    }

    /// Press and let go of a key (evdev code) in the app.
    pub fn tap(&self, code: u32) {
        let Some(k) = &self.vkbd else { return };
        let t = now_ms();
        k.key(t, code, 1);
        k.key(t.wrapping_add(1), code, 0);
        self.flush();
    }

    /// Pass a real keyboard's key on to the app.
    pub fn forward(&self, code: u32, pressed: bool, time: u32) {
        let Some(k) = &self.vkbd else { return };
        k.key(time, code, if pressed { 1 } else { 0 });
        self.flush();
    }
}

fn now_ms() -> u32 {
    (gtk::glib::monotonic_time() / 1000) as u32
}

pub struct State {
    tx: Sender<Event>,
    serial: Arc<AtomicU32>,
    vkbd: Option<vk::ZwpVirtualKeyboardV1>,
    pending: bool,
    active: bool,
    activated: bool,
    surrounding: Option<String>,
}

/// Connect (None: the compositor has no input-method support).
pub fn start(tx: Sender<Event>) -> Option<Client> {
    let conn = Connection::connect_to_env().ok()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn).ok()?;
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).ok()?;
    let mgr: im_manager::ZwpInputMethodManagerV2 = globals.bind(&qh, 1..=1, ()).ok()?;
    let input = mgr.get_input_method(&seat, &qh, ());
    let mut keymap = None;
    let vkbd = globals
        .bind::<vk_manager::ZwpVirtualKeyboardManagerV1, _, _>(&qh, 1..=1, ())
        .ok()
        .map(|m| {
            let k = m.create_virtual_keyboard(&seat, &qh, ());
            if let Some(f) = keymap_file() {
                k.keymap(1, f.as_fd(), (KEYMAP.len() + 1) as u32);
                keymap = Some(f);
            }
            k
        });
    let serial = Arc::new(AtomicU32::new(0));
    let client = Client {
        conn: conn.clone(),
        qh: qh.clone(),
        im: input.clone(),
        vkbd: vkbd.clone(),
        serial: serial.clone(),
        grab: Arc::new(Mutex::new(None)),
        _keymap: keymap,
    };
    let mut state = State {
        tx,
        serial,
        vkbd,
        pending: false,
        active: false,
        activated: false,
        surrounding: None,
    };
    std::thread::Builder::new()
        .name("rusttv-im".into())
        .spawn(move || {
            let _keep = (mgr, input, seat);
            while queue.blocking_dispatch(&mut state).is_ok() {}
            eprintln!("rusttv: the input method's connection closed");
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

macro_rules! no_events {
    ($t:ty) => {
        impl Dispatch<$t, ()> for State {
            fn event(
                _: &mut Self,
                _: &$t,
                _: <$t as wayland_client::Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    };
}
no_events!(wl_seat::WlSeat);
no_events!(im_manager::ZwpInputMethodManagerV2);
no_events!(vk_manager::ZwpVirtualKeyboardManagerV1);
no_events!(vk::ZwpVirtualKeyboardV1);

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
                this.pending = true;
                this.activated = true;
            }
            im::Event::Deactivate => this.pending = false,
            im::Event::SurroundingText { text, .. } => this.surrounding = Some(text),
            im::Event::Done => {
                this.serial.fetch_add(1, Ordering::SeqCst);
                if let Some(t) = this.surrounding.take() {
                    let _ = this.tx.send(Event::Surrounding(t));
                }
                // (a new text box, or the same one tapped again: show it)
                let again = this.activated && this.pending;
                this.activated = false;
                if this.pending != this.active || again {
                    this.active = this.pending;
                    let _ = this.tx.send(Event::Text(this.active));
                }
            }
            im::Event::Unavailable => {
                eprintln!(
                    "rusttv: another input method is running - no on-screen keyboard for apps"
                );
            }
            _ => {}
        }
    }
}

impl Dispatch<grab::ZwpInputMethodKeyboardGrabV2, ()> for State {
    fn event(
        this: &mut Self,
        _: &grab::ZwpInputMethodKeyboardGrabV2,
        event: grab::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            // the real keyboard's key map: the virtual keyboard uses it too,
            // so keys passed on mean the same
            grab::Event::Keymap { format, fd, size } => {
                if let (Some(k), WEnum::Value(f)) = (&this.vkbd, format) {
                    let fd: OwnedFd = fd;
                    k.keymap(f as u32, fd.as_fd(), size);
                }
            }
            grab::Event::Key {
                time, key, state, ..
            } => {
                let pressed = matches!(
                    state,
                    WEnum::Value(wayland_client::protocol::wl_keyboard::KeyState::Pressed)
                );
                let _ = this.tx.send(Event::Key(key, pressed, time));
            }
            grab::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                // (Shift and the like, for the keys passed on)
                if let Some(k) = &this.vkbd {
                    k.modifiers(mods_depressed, mods_latched, mods_locked, group);
                }
            }
            _ => {}
        }
    }
}

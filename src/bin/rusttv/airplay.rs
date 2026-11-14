//! AirPlay: an iPhone, iPad or Mac shows its screen (or plays its sound) on
//! the TV - Screen Mirroring in its Control Center, then this TV's name.
//!
//! The AirPlay part is UxPlay (`sudo apt install uxplay`), which RustTV runs
//! in the background while AirPlay is on (Settings > AirPlay), and starts
//! again if it stops. Its mirroring window is put on a screen of its own,
//! full screen, and shows in "Open now" like an app. The first time a device
//! connects (or every time, as set) the TV shows a 4-digit code to type on
//! it - over whatever is on screen.

use crate::config::{self, Config};
use crate::{keyboard, sway, ui, Tv};
use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// When a device has to type the code: (setting, what it's called).
pub const CODES: &[(&str, &str)] = &[
    ("first", "First time only"),
    ("always", "Every time"),
    ("never", "Never"),
];

/// the running UxPlay's process (0: none) - its window is the AirPlay one
static PID: AtomicI64 = AtomicI64::new(0);
/// bumped to stop the UxPlay that's running (each one has its number)
static GEN: AtomicU64 = AtomicU64::new(0);
/// start UxPlay again with a new key (a device finished: "every time")
static RENEW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// start UxPlay again now (it's streaming, but no window came)
static RESTART: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// the AirPlay windows already put on screen (by sway's window id)
static HANDLED: Mutex<Vec<i64>> = Mutex::new(Vec::new());
/// a device is mirroring (its window is open)
static MIRRORING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// what's going on, for the settings page
static STATUS: Mutex<Status> = Mutex::new(Status::Off);
/// what the running one was started with (to restart only when it changed)
static RUNNING: Mutex<Option<(String, String, bool)>> = Mutex::new(None);
/// the name the running UxPlay has (its X11 window has it as its title)
static NAME: Mutex<String> = Mutex::new(String::new());
/// whether the running UxPlay is sandboxed (None: not running)
static SANDBOXED: Mutex<Option<bool>> = Mutex::new(None);
/// a device has just typed the code (to check UxPlay kept it)
static JUST_PAIRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Clone, PartialEq)]
enum Status {
    Off,
    Missing,
    Starting,
    Ready,
    Failing,
    /// GStreamer's plugins for it aren't installed
    NoPlugins,
    /// the sandbox is on, but bubblewrap isn't installed
    NoBwrap,
    /// the sandbox is on, but bubblewrap doesn't work here
    BwrapBroken,
}

/// What UxPlay needs besides itself (Ubuntu's uxplay doesn't bring them).
pub const PLUGINS: &str =
    "gstreamer1.0-plugins-bad gstreamer1.0-plugins-good gstreamer1.0-libav gstreamer1.0-gl gstreamer1.0-x";

pub fn code_name(code: &str) -> &'static str {
    CODES
        .iter()
        .find(|c| c.0 == code)
        .map(|c| c.1)
        .unwrap_or("First time only")
}

/// UxPlay, if it's installed.
fn uxplay() -> Option<PathBuf> {
    find("uxplay")
}

/// A program on the PATH.
fn find(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(name))
            .find(|f| f.is_file())
    })
}

pub fn installed() -> bool {
    uxplay().is_some()
}

/// Devices that typed the code once (the "first time only" setting).
fn register() -> PathBuf {
    airplay_dir().join("register")
}

/// UxPlay's own folder: its key and register (the only part of your home
/// it sees, in its sandbox).
fn airplay_dir() -> PathBuf {
    config::dir().join("airplay")
}

/// (from before 0.9.17: the files were next to rusttv.json)
fn move_old_files() {
    let _ = std::fs::create_dir_all(airplay_dir());
    for (old, new) in [
        ("airplay.pem", key_file()),
        ("airplay.id", id_file()),
        ("airplay.register", register()),
    ] {
        let old = config::dir().join(old);
        if old.exists() && !new.exists() {
            let _ = std::fs::rename(&old, &new);
        }
        let _ = std::fs::remove_file(&old);
    }
}

/// The TV's AirPlay key: kept, so devices know it's the same TV next time.
fn key_file() -> PathBuf {
    airplay_dir().join("key.pem")
}

/// The TV's AirPlay device ID (what devices know it by), kept in a file;
/// `fresh`: a new one (not kept).
fn device_id(fresh: bool) -> String {
    let saved = std::fs::read_to_string(id_file())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| s.len() == 17);
    match saved {
        Some(id) if !fresh => id,
        _ => {
            let mut b = [0u8; 6];
            let _ = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b));
            if b == [0; 6] {
                let n = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(1);
                b.copy_from_slice(&n.to_le_bytes()[..6]);
            }
            // (a "locally made" address, not a real network card's)
            b[0] = (b[0] | 0x02) & 0xfe;
            let id = b
                .iter()
                .map(|x| format!("{x:02x}"))
                .collect::<Vec<_>>()
                .join(":");
            if !fresh {
                let _ = std::fs::write(id_file(), &id);
            }
            id
        }
    }
}

fn id_file() -> PathBuf {
    airplay_dir().join("id")
}

/// Is `w` the AirPlay window (UxPlay's, while a device mirrors)?
pub fn is_airplay(w: &sway::Window) -> bool {
    let pid = PID.load(Ordering::SeqCst);
    // (its X11 window - shown through XWayland - has the TV's AirPlay name as
    // its title, and maybe not UxPlay's process)
    let name = NAME.lock().unwrap().clone();
    (pid > 0 && w.pid == pid)
        || w.app_id.eq_ignore_ascii_case("uxplay")
        // (GStreamer's video window, which UxPlay shows the screen in)
        || w.app_id.eq_ignore_ascii_case("gstreamer")
        || (!name.is_empty() && w.title == name)
}

/// Start, restart or stop UxPlay to match the settings.
pub fn apply(cfg: &Config) {
    let want = (cfg.airplay && installed()).then(|| {
        let name = if cfg.airplay_name.trim().is_empty() {
            "RustTV".to_string()
        } else {
            cfg.airplay_name.trim().to_string()
        };
        (name, cfg.airplay_code.clone(), cfg.airplay_sandbox)
    });
    let mut running = RUNNING.lock().unwrap();
    if *running == want {
        return;
    }
    // stop the one running (its thread sees the number change)
    let g = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    *running = want.clone();
    drop(running);
    let Some((name, code, sandbox)) = want else {
        *STATUS.lock().unwrap() = if cfg.airplay {
            Status::Missing
        } else {
            Status::Off
        };
        crate::on_main(|| hide_code_for("AirPlay is off"));
        return;
    };
    std::thread::spawn(move || supervise(g, name, code, sandbox));
}

/// UxPlay's sandbox (bubblewrap): the system's programs and settings (read
/// only), the TV's picture (X11) and sound sockets, the system bus (to show
/// up on the network, through Avahi) - and of your home folder only its own
/// AirPlay folder. Its own process list; the network is shared (it's a
/// network service).
fn sandbox_args() -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = Vec::new();
    let mut add = |xs: &[&str]| a.extend(xs.iter().map(std::ffi::OsString::from));
    add(&["--ro-bind", "/usr", "/usr"]);
    for (link, to) in [
        ("/bin", "usr/bin"),
        ("/sbin", "usr/sbin"),
        ("/lib", "usr/lib"),
        ("/lib64", "usr/lib64"),
        ("/lib32", "usr/lib32"),
    ] {
        // (merged /usr: these are links to it)
        if std::fs::read_link(link).is_ok() {
            add(&["--symlink", to, link]);
        } else if std::path::Path::new(link).is_dir() {
            add(&["--ro-bind", link, link]);
        }
    }
    add(&[
        "--ro-bind",
        "/etc",
        "/etc",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
        "--ro-bind-try",
        "/tmp/.X11-unix",
        "/tmp/.X11-unix",
        "--tmpfs",
        "/run",
        "--ro-bind-try",
        "/run/dbus",
        "/run/dbus",
        "--ro-bind-try",
        "/run/avahi-daemon",
        "/run/avahi-daemon",
        "--ro-bind-try",
        "/run/systemd/resolve",
        "/run/systemd/resolve",
        "--ro-bind-try",
        "/var/lib/dbus",
        "/var/lib/dbus",
        "--unshare-pid",
        "--unshare-uts",
        "--unshare-cgroup-try",
        "--die-with-parent",
        "--new-session",
    ]);
    let rt = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    if !rt.is_empty() {
        add(&["--dir", &rt]);
        // (the sound: PulseAudio's socket only - PipeWire's own isn't
        // needed. It can still record, like any sound client: known.)
        let p = format!("{rt}/pulse");
        add(&["--ro-bind-try", &p, &p]);
    }
    let home = config::home().to_string_lossy().to_string();
    let ap = airplay_dir().to_string_lossy().to_string();
    let reg = airplay_dir()
        .join("gst-registry.bin")
        .to_string_lossy()
        .to_string();
    add(&[
        "--tmpfs",
        &home,
        "--bind",
        &ap,
        &ap,
        "--setenv",
        "HOME",
        &home,
        // (GStreamer's list of its plugins, kept between starts)
        "--setenv",
        "GST_REGISTRY",
        &reg,
        "--chdir",
        &ap,
    ]);
    // (X11's key, if there is one - after the home folder's hidden: it may
    // be in it)
    if let Ok(xa) = std::env::var("XAUTHORITY") {
        add(&["--ro-bind-try", &xa, &xa]);
    }
    a
}

/// Stop UxPlay left over from before (RustTV started again).
pub fn clean_up() {
    use std::os::unix::fs::MetadataExt;
    if let Ok(m) = std::fs::metadata("/proc/self") {
        let _ = Command::new("pkill")
            .args(["-U", &m.uid().to_string(), "-x", "uxplay"])
            .stderr(Stdio::null())
            .status();
    }
}

/// Run UxPlay, and again whenever it stops, until `g` isn't current.
fn supervise(g: u64, name: String, code: String, sandbox: bool) {
    let current = || GEN.load(Ordering::SeqCst) == g;
    let mut quick_stops = 0u32;
    let mut renewed = false;
    let _ = std::fs::create_dir_all(config::dir());
    move_old_files();
    while current() {
        let Some(exe) = uxplay() else {
            *STATUS.lock().unwrap() = Status::Missing;
            return;
        };
        let ask = code != "never";
        // (its messages line by line, as they come: piped, they'd otherwise
        // wait in a buffer - and the code would show long after it's asked)
        let mut args: Vec<std::ffi::OsString> = Vec::new();
        if let Some(sb) = find("stdbuf") {
            args.extend([sb.into(), "-oL".into(), "-eL".into()]);
        }
        args.push(exe.into());
        // its name (without "@hostname"); a new device takes over from the
        // one mirroring, like on a TV.
        // A device that typed the code once knows this TV by its device ID
        // (and key), and doesn't ask again while they're the same: "every
        // time" gets new ones each time (UxPlay starts again when mirroring
        // ends); the others keep them ("Unpair all" makes new ones).
        *NAME.lock().unwrap() = name.clone();
        let mut ux: Vec<std::ffi::OsString> = Vec::new();
        ux.extend(
            [
                "-n",
                &name,
                "-nh",
                "-nohold",
                // the video decoded by the processor, shown in a plain X11 video
                // window: what came up most reliably (tested by hand)
                "-avdec",
                "-vs",
                "xvimagesink",
                "-m",
                &device_id(code == "always"),
            ]
            .map(std::ffi::OsString::from),
        );
        if code != "always" {
            ux.push("-key".into());
            ux.push(key_file().into());
        }
        if ask {
            // a new code for each device that asks (UxPlay picks it and says
            // it: shown on the TV) - no trying all 9,000 against one code
            ux.push("-pin".into());
            // the devices that typed it: only they're let back in without
            // one (without a register, any device saying it had paired
            // would be). It has to be there for UxPlay to use it; "every
            // time" starts with it empty.
            let reg = register();
            if code == "always" || !reg.exists() {
                let _ = std::fs::write(&reg, "");
            }
            ux.push("-reg".into());
            ux.push(reg.into());
        }
        args.extend(ux);
        // The sandbox is never left out by itself (something on the
        // network crashing UxPlay mustn't be able to switch it off): without
        // bubblewrap, or where it doesn't work, AirPlay waits for you - to
        // fix it, or to turn Sandbox off yourself.
        let sandboxed = sandbox;
        if sandboxed && find("bwrap").is_none() {
            eprintln!("rusttv: airplay: bubblewrap isn't installed - not starting UxPlay without its sandbox");
            *STATUS.lock().unwrap() = Status::NoBwrap;
            return;
        }
        let mut cmd = if sandboxed {
            let mut c = Command::new("bwrap");
            c.args(sandbox_args()).arg("--").args(&args);
            c
        } else {
            let mut c = Command::new(&args[0]);
            c.args(&args[1..]);
            c
        };
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        eprintln!(
            "rusttv: airplay: starting UxPlay{}",
            if sandboxed {
                " (sandboxed)"
            } else {
                " (NOT sandboxed)"
            }
        );
        *SANDBOXED.lock().unwrap() = Some(sandboxed);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("rusttv: can't start uxplay: {e}");
                *STATUS.lock().unwrap() = Status::Failing;
                return;
            }
        };
        PID.store(child.id() as i64, Ordering::SeqCst);
        {
            let mut st = STATUS.lock().unwrap();
            if !matches!(*st, Status::Failing | Status::NoPlugins) {
                *st = Status::Starting;
            }
        }
        let started = Instant::now();
        // its messages go to RustTV's log; asking a device for the code
        // shows the code on the TV
        RENEW.store(false, Ordering::SeqCst);
        let remember = code == "first";
        let saw_bwrap_error = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watch = |r: Box<dyn Read + Send>| {
            let bw = saw_bwrap_error.clone();
            // (not on a device going: pairing itself connects twice - only
            // a finished mirroring, below, gets a new key)
            std::thread::spawn(move || {
                for line in BufReader::new(r).lines().map_while(Result::ok) {
                    // (the code itself stays out of the log)
                    if line.contains("MUST NOW ENTER PIN") {
                        eprintln!("uxplay: *** CLIENT MUST NOW ENTER PIN = \"****\"");
                    } else {
                        eprintln!("uxplay: {line}");
                    }
                    if line.starts_with("bwrap:") {
                        bw.store(true, Ordering::SeqCst);
                    }
                    let low = line.to_ascii_lowercase();
                    if low.contains("gstreamer plugin") && low.contains("not found") {
                        *STATUS.lock().unwrap() = Status::NoPlugins;
                    }
                    // (a device going doesn't put the code away: pairing
                    // itself connects, goes, and connects again once the
                    // code's typed)
                    // a device asks for the code:
                    //   *** CLIENT MUST NOW ENTER PIN = "7199" AS AIRPLAY PASSWORD
                    if line.contains("MUST NOW ENTER PIN") {
                        let shown = line
                            .split('"')
                            .nth(1)
                            .filter(|c| c.len() == 4 && c.chars().all(|c| c.is_ascii_digit()))
                            .map(str::to_string);
                        if let Some(c) = shown {
                            JUST_PAIRED.store(true, Ordering::SeqCst);
                            eprintln!("rusttv: airplay: showing the code");
                            crate::on_main(move || show_code(&c));
                        }
                    }
                    // paired (or a paired one back), connecting:
                    //   connection request from NAME (MODEL) with deviceID = ID
                    // the screen's coming: its window should be on screen
                    // soon - check, in a few seconds
                    if line.contains("Begin streaming") {
                        crate::on_main(|| {
                            glib::timeout_add_local_once(Duration::from_secs(6), check_window);
                        });
                    }
                    if let Some(d) = connecting(&line) {
                        crate::on_main(|| hide_code_for("the device typed it - it's connecting"));
                        if remember {
                            remember_device(&d);
                        }
                        // (it typed the code: UxPlay should have kept it in
                        // its register - otherwise it'll be asked again)
                        if JUST_PAIRED.swap(false, Ordering::SeqCst) {
                            std::thread::spawn(|| {
                                std::thread::sleep(Duration::from_secs(3));
                                let n = std::fs::metadata(register()).map(|m| m.len()).unwrap_or(0);
                                if n == 0 {
                                    eprintln!(
                                        "rusttv: airplay: UxPlay didn't keep the device that typed the code \
                                         in its register - it'll be asked again next time"
                                    );
                                }
                            });
                        }
                    }
                }
            });
        };
        if let Some(o) = child.stdout.take() {
            watch(Box::new(o));
        }
        if let Some(e) = child.stderr.take() {
            watch(Box::new(e));
        }
        loop {
            std::thread::sleep(Duration::from_millis(300));
            if !current() {
                stop(&mut child);
                PID.store(0, Ordering::SeqCst);
                return;
            }
            if let Ok(Some(st)) = child.try_wait() {
                eprintln!("rusttv: uxplay stopped ({st})");
                // the sandbox itself doesn't work here: stop, and say so
                if sandboxed
                    && started.elapsed() < Duration::from_secs(5)
                    && saw_bwrap_error.load(Ordering::SeqCst)
                {
                    eprintln!("rusttv: airplay: bubblewrap doesn't work here - not starting UxPlay without its sandbox");
                    *STATUS.lock().unwrap() = Status::BwrapBroken;
                    PID.store(0, Ordering::SeqCst);
                    crate::on_main(|| hide_code_for("UxPlay stopped"));
                    return;
                }
                break;
            }
            // streaming, but no window came: start again (the device drops,
            // and connecting again starts clean)
            if RESTART.swap(false, Ordering::SeqCst) {
                eprintln!("rusttv: airplay: no window came - starting UxPlay again");
                stop(&mut child);
                renewed = true;
                break;
            }
            // mirroring ended: UxPlay starts again, fresh - one that's had a
            // device sometimes gives the next one no picture (a fresh one
            // always does). "Every time": with a new identity, so the next
            // device types a code.
            if !MIRRORING.load(Ordering::SeqCst) && RENEW.swap(false, Ordering::SeqCst) {
                eprintln!("rusttv: airplay: mirroring ended - starting UxPlay again, fresh");
                stop(&mut child);
                renewed = true;
                break;
            }
            // still going after a few seconds: it's up
            if started.elapsed() > Duration::from_secs(3) {
                let mut st = STATUS.lock().unwrap();
                if *st != Status::Ready {
                    *st = Status::Ready;
                }
            }
        }
        PID.store(0, Ordering::SeqCst);
        crate::on_main(|| hide_code_for("UxPlay stopped"));
        if std::mem::take(&mut renewed) {
            quick_stops = 0;
            continue;
        }
        // (stopping straight after starting, again and again: something's
        // wrong - wait longer between tries, up to a minute)
        if started.elapsed() < Duration::from_secs(10) {
            quick_stops += 1;
        } else {
            quick_stops = 0;
        }
        // (stopping again and again stays sandboxed: "keeps stopping")
        if quick_stops >= 3 {
            let mut st = STATUS.lock().unwrap();
            if *st != Status::NoPlugins {
                *st = Status::Failing;
            }
        }
        let wait = Duration::from_secs(1u64 << quick_stops.min(6));
        let until = Instant::now() + wait;
        while Instant::now() < until {
            if !current() {
                return;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
}

/// Stop UxPlay politely (it takes its name off the network), and force it
/// after a second - quick, so it's back before a device connects again.
fn stop(child: &mut std::process::Child) {
    let _ = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .stderr(Stdio::null())
        .status();
    let until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < until {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The AirPlay window opened (a device started mirroring): on a screen of
/// its own, full screen, shown.
/// A few seconds after a device started streaming: is its window on
/// screen? If not, any window that may be UxPlay's goes on screen; with none
/// at all, UxPlay starts again.
fn check_window() {
    if !sway::available() || PID.load(Ordering::SeqCst) == 0 {
        return;
    }
    let wins = sway::windows();
    if wins.iter().any(|w| is_airplay(w) && handled(w.id)) {
        return;
    }
    eprintln!("rusttv: airplay: streaming, but its window isn't on screen. The windows open:");
    for w in &wins {
        eprintln!(
            "rusttv: airplay:   id {} pid {} app_id {:?} title {:?} workspace {:?}",
            w.id, w.pid, w.app_id, w.title, w.workspace
        );
    }
    let name = NAME.lock().unwrap().clone();
    let maybe = wins.iter().find(|w| {
        !handled(w.id)
            && (is_airplay(w)
                || (!name.is_empty() && w.title.contains(&name))
                || w.app_id.to_ascii_lowercase().contains("gst"))
    });
    match maybe {
        Some(w) => window_opened(w),
        None => RESTART.store(true, Ordering::SeqCst),
    }
}

/// Has `id` (an AirPlay window) been put on screen already?
pub fn handled(id: i64) -> bool {
    HANDLED.lock().unwrap().contains(&id)
}

pub fn window_opened(w: &sway::Window) {
    HANDLED.lock().unwrap().push(w.id);
    eprintln!(
        "rusttv: airplay: its window opened (id {}, app_id {:?}, title {:?}): showing it",
        w.id, w.app_id, w.title
    );
    hide_code_for("mirroring started");
    MIRRORING.store(true, Ordering::SeqCst);
    let ws = sway::free_workspace();
    sway::run(&format!(
        "[con_id={id}] move container to workspace number {ws}; workspace number {ws}; \
         [con_id={id}] fullscreen enable",
        id = w.id
    ));
}

/// The windows changed: when the AirPlay one has gone, the device
/// finished (UxPlay starts again, fresh).
pub fn windows_changed(wins: &[sway::Window]) {
    HANDLED
        .lock()
        .unwrap()
        .retain(|id| wins.iter().any(|w| w.id == *id));
    if MIRRORING.load(Ordering::SeqCst) && !wins.iter().any(is_airplay) {
        eprintln!("rusttv: airplay: its window closed");
        MIRRORING.store(false, Ordering::SeqCst);
        RENEW.store(true, Ordering::SeqCst);
    }
}

// ------------------------------------------------------------ the code

struct CodeView {
    win: Option<gtk::Window>,
    digits: gtk::Label,
    timer: Option<glib::SourceId>,
}

thread_local! {
    static CODE: RefCell<Option<CodeView>> = const { RefCell::new(None) };
    /// when the code was shown (a key in the first moment doesn't close it)
    static SHOWN_AT: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// Back (or a click) on the code: put it away - unless it's only just
/// shown (a key already on its way).
#[cfg_attr(not(feature = "layer"), allow(dead_code))]
fn cancel_code() {
    let fresh = SHOWN_AT.with(|s| {
        s.get()
            .is_some_and(|t| t.elapsed() < Duration::from_millis(800))
    });
    if !fresh {
        hide_code_for("Back (or a button) on the TV");
    }
}

fn code_box(digits: &gtk::Label) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 14);
    b.add_css_class("airplay-box");
    let t = gtk::Label::new(Some("AirPlay"));
    t.add_css_class("airplay-title");
    digits.add_css_class("airplay-code");
    let n = gtk::Label::new(Some("Type this code on your iPhone, iPad or Mac"));
    n.add_css_class("airplay-note");
    b.append(&t);
    b.append(digits);
    b.append(&n);
    let h = ui::hints(&[("Back", "Cancel")]);
    h.set_halign(gtk::Align::Center);
    h.set_margin_top(10);
    b.append(&h);
    b
}

/// Show `code` big, over everything, for a minute (or until mirroring
/// starts).
pub fn show_code(code: &str) {
    let spaced: String = code
        .chars()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    CODE.with(|cv| {
        let mut cv = cv.borrow_mut();
        if cv.is_none() {
            *cv = Some(build_code_view());
        }
        let v = cv.as_mut().unwrap();
        v.digits.set_text(&spaced);
        match &v.win {
            Some(w) => {
                if !w.is_visible() {
                    SHOWN_AT.with(|s| s.set(Some(Instant::now())));
                }
                w.set_visible(true)
            }
            None => {
                // (no layer-shell: on RustTV's own screen, brought to the front)
                crate::with_tv(|tv| {
                    if sway::available() {
                        sway::run(&format!("workspace {}", sway::HOME));
                    }
                    let digits = gtk::Label::new(Some(&spaced));
                    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    root.add_css_class("page");
                    let card = code_box(&digits);
                    card.set_halign(gtk::Align::Center);
                    card.set_valign(gtk::Align::Center);
                    card.set_vexpand(true);
                    root.append(&card);
                    root.append(&ui::hints(&[("Back", "Close")]));
                    tv.push(&root, None);
                    tv.win.present();
                });
            }
        }
        if let Some(t) = v.timer.take() {
            t.remove();
        }
        v.timer = Some(glib::timeout_add_local_once(
            Duration::from_secs(60),
            || {
                CODE.with(|cv| {
                    if let Some(v) = cv.borrow_mut().as_mut() {
                        v.timer = None;
                        if let Some(w) = &v.win {
                            if w.is_visible() {
                                eprintln!("rusttv: airplay: code put away (a minute passed)");
                            }
                            w.set_visible(false);
                        }
                    }
                });
            },
        ));
    });
}

fn hide_code_for(why: &str) {
    CODE.with(|cv| {
        if let Some(v) = cv.borrow_mut().as_mut() {
            if let Some(t) = v.timer.take() {
                t.remove();
            }
            if let Some(w) = &v.win {
                if w.is_visible() {
                    eprintln!("rusttv: airplay: code put away ({why})");
                }
                w.set_visible(false);
            }
        }
    });
}

fn build_code_view() -> CodeView {
    let digits = gtk::Label::new(None);
    #[cfg(feature = "layer")]
    {
        use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
        let app = crate::with_tv_ret(|t| t.app.clone());
        if let Some(app) = app.filter(|_| gtk4_layer_shell::is_supported()) {
            let win = gtk::Window::builder()
                .application(&app)
                .title("RustTV AirPlay code")
                .build();
            win.add_css_class("osd-window");
            win.init_layer_shell();
            win.set_namespace(Some("rusttv-airplay"));
            win.set_layer(Layer::Overlay);
            // (no edges: in the middle of the screen)
            // While it shows it has the keys (the app under it doesn't):
            // Back - or any button - puts it away. (Home and the volume
            // still work: they're sway's.)
            win.set_keyboard_mode(KeyboardMode::Exclusive);
            win.set_exclusive_zone(-1);
            win.set_child(Some(&code_box(&digits)));
            let k = gtk::EventControllerKey::new();
            k.connect_key_pressed(|_, _, _, _| {
                cancel_code();
                glib::Propagation::Stop
            });
            win.add_controller(k);
            // (a mouse: a click puts it away too)
            let c = gtk::GestureClick::new();
            c.connect_released(|_, _, _, _| cancel_code());
            win.add_controller(c);
            return CodeView {
                win: Some(win),
                digits,
                timer: None,
            };
        }
    }
    CodeView {
        win: None,
        digits,
        timer: None,
    }
}

// ------------------------------------------------------------ devices

/// A device that connected with "first time only" (it typed the code once,
/// then keeps coming back without): its ID, name and model, from UxPlay's
/// "connection request from NAME (MODEL) with deviceID = ID".
#[derive(Clone)]
struct Device {
    id: String,
    name: String,
    model: String,
}

fn devices_file() -> PathBuf {
    config::dir().join("airplay.devices")
}

/// The device in a UxPlay "connection request" line.
fn connecting(line: &str) -> Option<Device> {
    let rest = line.split("connection request from ").nth(1)?;
    let (who, id) = rest.rsplit_once(" with deviceID = ")?;
    let (name, model) = match who.rsplit_once(" (") {
        Some((n, m)) => (n.to_string(), m.trim_end_matches(')').to_string()),
        None => (who.to_string(), String::new()),
    };
    Some(Device {
        id: id.trim().to_string(),
        name: name.trim().to_string(),
        model,
    })
}

/// The devices that connected (one a line: ID, name, model - tab apart).
fn devices() -> Vec<Device> {
    std::fs::read_to_string(devices_file())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let id = f.next()?.trim().to_string();
            (!id.is_empty()).then(|| Device {
                id,
                name: f.next().unwrap_or("A device").to_string(),
                model: f.next().unwrap_or_default().to_string(),
            })
        })
        .collect()
}

fn save_devices(list: &[Device]) {
    let text: String = list
        .iter()
        .map(|d| format!("{}\t{}\t{}\n", d.id, d.name, d.model))
        .collect();
    let _ = std::fs::create_dir_all(config::dir());
    let _ = std::fs::write(devices_file(), text);
}

/// Keep `d` on the list (its name updated if it changed).
fn remember_device(d: &Device) {
    let mut list = devices();
    list.retain(|x| x.id != d.id);
    list.push(d.clone());
    save_devices(&list);
}

/// Start UxPlay again (it reads what it knows at the start).
fn restart(tv: &Tv) {
    *RUNNING.lock().unwrap() = None;
    apply(&tv.cfg());
}

/// A new device ID and key: a new TV to every device, so each types a code
/// again. (The pairing is kept by the device, not the TV: there's no
/// sending just one away.)
fn new_identity() {
    let _ = std::fs::remove_file(key_file());
    let _ = std::fs::remove_file(id_file());
    let _ = std::fs::remove_file(register());
}

/// Unpair one: it types a code again (and the others do, once).
fn unpair(tv: &Tv, d: &Device) {
    let mut list = devices();
    list.retain(|x| x.id != d.id);
    // (the others aren't forgotten: they're back on the list once they've
    // typed the new code)
    save_devices(&list);
    new_identity();
    restart(tv);
    // (the pages showing devices, again)
    tv.change(|_| {});
    tv.toast(&format!("Unpaired {}", d.name));
}

fn unpair_all(tv: &Tv) {
    save_devices(&[]);
    new_identity();
    restart(tv);
    tv.change(|_| {});
    tv.toast("Unpaired every device: each types a code again");
}

/// Settings > AirPlay > Devices: the devices that typed the code; OK on
/// one unpairs it.
fn devices_page(tv: &Tv) {
    let body = ui::list();
    let list = devices();
    let ask = |tv: &Tv, d: &Device| {
        let d = d.clone();
        let others = devices().len() > 1;
        let q = if others {
            format!(
                "Unpair {}? It types a code next time - and so do your other devices, once.",
                d.name
            )
        } else {
            format!("Unpair {}? It types a code next time.", d.name)
        };
        ui::confirm(tv, &q, "Unpair", move |t| {
            unpair(t, &d);
            // (the list again, without it)
            t.back();
            devices_page(t);
        });
    };
    let sub = |d: &Device| {
        if d.model.is_empty() {
            d.id.clone()
        } else {
            d.model.clone()
        }
    };
    if !list.is_empty() {
        let t = tv.clone();
        let found_list = list.clone();
        let (search, _) = ui::row("Search", "", move || {
            let found = found_list
                .iter()
                .map(|d| {
                    let d = d.clone();
                    keyboard::Found {
                        label: d.name.clone(),
                        sub: sub(&d),
                        pick: Rc::new(move |t: &Tv| ask(t, &d)),
                    }
                })
                .collect();
            keyboard::search(&t, "Search: Devices", found);
        });
        search.add_css_class("search-row");
        body.append(&search);
    }
    for d in list {
        let t = tv.clone();
        let (name, model) = (d.name.clone(), sub(&d));
        let (b, _) = ui::row(&name, &model, move || ask(&t, &d));
        body.append(&b);
    }
    let t = tv.clone();
    let (all, _) = ui::row("Unpair all", "", move || {
        ui::confirm(
            &t,
            "Unpair every device? Each types a code again.",
            "Unpair all",
            |t| {
                unpair_all(t);
                // (the list again, empty)
                t.back();
                devices_page(t);
            },
        );
    });
    body.append(&all);
    let note = gtk::Label::new(Some(if tv.cfg().airplay_code == "first" {
        "Devices that typed the code (they connect straight in after that). \
         A device keeps its pairing itself, so unpairing one gives the TV a new \
         identity: your other devices type a code once too."
    } else {
        "Devices are only kept with Ask for a code: First time only."
    }));
    note.add_css_class("note");
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.set_max_width_chars(70);
    body.append(&note);
    let pg = ui::page("AirPlay devices", &body);
    tv.push(&pg, None);
}

// ------------------------------------------------------------ settings

fn status_text(cfg: &Config) -> String {
    if !installed() {
        return "Needs UxPlay: sudo apt install uxplay".into();
    }
    let st = STATUS.lock().unwrap().clone();
    match st {
        _ if !cfg.airplay => "Off: this TV doesn't show up for AirPlay".into(),
        Status::Starting => "Starting\u{2026}".into(),
        Status::NoPlugins => format!("Needs GStreamer's plugins: sudo apt install {PLUGINS}"),
        Status::Off | Status::Ready => {
            let mut t = format!(
                "Ready: \u{201c}{}\u{201d} in Screen Mirroring",
                shown_name(cfg)
            );
            if *SANDBOXED.lock().unwrap() == Some(false) {
                t.push_str(" (not sandboxed)");
            }
            t
        }
        Status::Missing => "Needs UxPlay: sudo apt install uxplay".into(),
        Status::NoBwrap => {
            "Needs bubblewrap, UxPlay's sandbox: sudo apt install bubblewrap (or turn Sandbox off)"
                .into()
        }
        Status::BwrapBroken => "UxPlay's sandbox (bubblewrap) doesn't work here - the log says \
                                why. Fix it, or turn Sandbox off"
            .into(),
        Status::Failing => {
            "UxPlay keeps stopping - the log says why (is avahi-daemon running?)".into()
        }
    }
}

fn shown_name(cfg: &Config) -> String {
    if cfg.airplay_name.trim().is_empty() {
        "RustTV".into()
    } else {
        cfg.airplay_name.trim().into()
    }
}

/// For the Settings row: On, Off, or that it needs installing.
pub fn summary(cfg: &Config) -> &'static str {
    if !installed() {
        "Not installed"
    } else if cfg.airplay {
        "On"
    } else {
        "Off"
    }
}

/// Settings > AirPlay.
pub fn page(tv: &Tv) {
    let cfg = tv.cfg();
    let body = ui::list();
    let on_off = |b: bool| if b { "On" } else { "Off" };

    let status = gtk::Label::new(Some(&status_text(&cfg)));
    status.add_css_class("note");
    status.set_halign(gtk::Align::Start);
    status.set_wrap(true);
    status.set_xalign(0.0);

    let t = tv.clone();
    let st = status.clone();
    let on_v: Rc<RefCell<Option<gtk::Label>>> = Rc::new(RefCell::new(None));
    let ov = on_v.clone();
    let (on, on_label) = ui::row("AirPlay", on_off(cfg.airplay), move || {
        t.change(|c| c.airplay = !c.airplay);
        let cfg = t.cfg();
        apply(&cfg);
        if let Some(l) = ov.borrow().as_ref() {
            l.set_text(on_off(cfg.airplay));
        }
        st.set_text(&status_text(&cfg));
    });
    *on_v.borrow_mut() = Some(on_label);
    body.append(&on);

    let t = tv.clone();
    let st = status.clone();
    let name_v: Rc<RefCell<Option<gtk::Label>>> = Rc::new(RefCell::new(None));
    let nv = name_v.clone();
    let (name, name_label) = ui::row("Name", &shown_name(&cfg), move || {
        let (st, nv) = (st.clone(), nv.clone());
        keyboard::ask(&t, "AirPlay name", &shown_name(&t.cfg()), move |t, s| {
            let s = s.trim().to_string();
            if s.is_empty() {
                return;
            }
            t.change(|c| c.airplay_name = s.clone());
            let cfg = t.cfg();
            apply(&cfg);
            if let Some(l) = nv.borrow().as_ref() {
                l.set_text(&s);
            }
            st.set_text(&status_text(&cfg));
        });
    });
    *name_v.borrow_mut() = Some(name_label);
    body.append(&name);

    let t = tv.clone();
    let code_v: Rc<RefCell<Option<gtk::Label>>> = Rc::new(RefCell::new(None));
    let cv = code_v.clone();
    let (code, code_label) = ui::row("Ask for a code", code_name(&cfg.airplay_code), move || {
        let opts = CODES
            .iter()
            .map(|c| (c.0.to_string(), c.1.to_string()))
            .collect();
        let cv = cv.clone();
        ui::choose(
            &t,
            "Ask for a code",
            opts,
            &t.cfg().airplay_code,
            move |t, v| {
                t.change(|c| c.airplay_code = v.clone());
                apply(&t.cfg());
                if let Some(l) = cv.borrow().as_ref() {
                    l.set_text(code_name(&v));
                }
            },
        );
    });
    *code_v.borrow_mut() = Some(code_label);
    body.append(&code);

    // the sandbox: on unless you turn it off (with a warning)
    let t = tv.clone();
    let sb_v: Rc<RefCell<Option<gtk::Label>>> = Rc::new(RefCell::new(None));
    let sv = sb_v.clone();
    let (sb, sb_label) = ui::row("Sandbox", on_off(cfg.airplay_sandbox), move || {
        let sv = sv.clone();
        let set = move |t: &Tv, on: bool| {
            t.change(|c| c.airplay_sandbox = on);
            apply(&t.cfg());
            if let Some(l) = sv.borrow().as_ref() {
                l.set_text(if on { "On" } else { "Off" });
            }
        };
        if t.cfg().airplay_sandbox {
            ui::confirm(
                &t,
                "Run UxPlay without its sandbox? A bug in it could then reach your whole account.",
                "Turn it off",
                move |t| set(t, false),
            );
        } else {
            set(&t, true);
        }
    });
    *sb_v.borrow_mut() = Some(sb_label);
    body.append(&sb);

    let t = tv.clone();
    let n = devices().len();
    let (devs, devs_v) = ui::row("Devices", &n.to_string(), move || devices_page(&t));
    body.append(&devs);

    // (what restarting RustTV did, when a device won't show its screen)
    let t = tv.clone();
    let (again, _) = ui::row("Restart AirPlay", "", move || {
        if !t.cfg().airplay {
            t.toast("AirPlay is off");
            return;
        }
        eprintln!("rusttv: airplay: restarted from Settings");
        restart(&t);
        t.toast("AirPlay is starting again - connect in a moment");
    });
    body.append(&again);

    body.append(&status);
    let how = gtk::Label::new(Some(
        "On an iPhone or iPad: open Control Center, pick Screen Mirroring, then this TV. \
         On a Mac: Control Center, Screen Mirroring. Music and videos can be sent \
         with the AirPlay button in the app playing them.",
    ));
    how.add_css_class("note");
    how.set_wrap(true);
    how.set_xalign(0.0);
    how.set_max_width_chars(70);
    body.append(&how);

    // (the status line follows UxPlay while the page shows)
    let (t, st) = (tv.clone(), status.clone());
    glib::timeout_add_local(Duration::from_secs(1), move || {
        if st.root().is_none() {
            return glib::ControlFlow::Break;
        }
        st.set_text(&status_text(&t.cfg()));
        glib::ControlFlow::Continue
    });

    let pg = ui::page("AirPlay", &body);
    // (how many devices, kept up to date - unpairing changes it)
    tv.push_page(
        &pg,
        None,
        Some(Rc::new(move || {
            devs_v.set_text(&devices().len().to_string())
        })),
    );
}

//! Suspending by itself after a while with nothing going on (Settings >
//! Auto suspend). swayidle does the waiting and says when the time's up
//! ("idle") or someone's back ("active"); RustTV then suspends - unless
//! something's playing or asked to keep the TV on (see inhibit.rs: a video in
//! a browser - MPRIS -, Chromium's org.freedesktop.ScreenSaver.Inhibit,
//! systemd's idle inhibitors; and AirPlay mirroring). Then it looks again
//! every minute, while nobody's pressed anything.

use gtk::glib;
use std::cell::Cell;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

thread_local! {
    static CHILD: std::cell::RefCell<Option<Child>> = const { std::cell::RefCell::new(None) };
    /// the time's up and nobody's pressed anything since
    static IDLE: Cell<bool> = const { Cell::new(false) };
    /// which swayidle the news is from (an old one's is ignored)
    static GEN: Cell<u32> = const { Cell::new(0) };
}

/// Should the TV stay on now? (something playing, asked to, or mirroring)
async fn busy() -> bool {
    crate::airplay::mirroring() || crate::inhibit::busy().await
}

/// The time's up: suspend, unless something's going on - then look again in
/// a minute (as long as nobody presses anything).
fn idle(generation: u32) {
    if GEN.with(|g| g.get()) != generation || !IDLE.with(|i| i.get()) {
        return;
    }
    glib::MainContext::default().spawn_local(async move {
        if GEN.with(|g| g.get()) != generation || !IDLE.with(|i| i.get()) {
            return;
        }
        if busy().await {
            glib::timeout_add_local_once(Duration::from_secs(60), move || idle(generation));
            return;
        }
        eprintln!("rusttv: nothing going on - suspending");
        IDLE.with(|i| i.set(false));
        let _ = Command::new("systemctl").arg("suspend").spawn();
    });
}

/// (the swayidle started last time, in case RustTV was restarted)
fn pid_file() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::home().join(".cache"))
        .join("rusttv-idle.pid")
}

fn stop() {
    CHILD.with(|c| {
        if let Some(mut ch) = c.borrow_mut().take() {
            let _ = ch.kill();
            let _ = ch.wait();
        }
    });
    // one left from an earlier RustTV
    if let Ok(pid) = std::fs::read_to_string(pid_file()) {
        let pid = pid.trim();
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if comm.trim() == "swayidle" {
            let _ = Command::new("kill").arg(pid).status();
        }
    }
    let _ = std::fs::remove_file(pid_file());
}

/// Suspend after `minutes` with nothing going on (0: never). False when it
/// can't (swayidle isn't installed).
pub fn apply(minutes: u32) -> bool {
    stop();
    if minutes == 0 {
        return true;
    }
    if crate::launch::find("swayidle").is_none() {
        eprintln!("rusttv: auto suspend needs swayidle (sudo apt install swayidle)");
        return false;
    }
    let secs = (minutes as u64) * 60;
    // (swayidle only says when; RustTV decides - see the top)
    let child = Command::new("swayidle")
        .args([
            "-w",
            "timeout",
            &secs.to_string(),
            "echo idle",
            "resume",
            "echo active",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn();
    match child {
        Ok(mut c) => {
            let _ = std::fs::write(pid_file(), c.id().to_string());
            let generation = GEN.with(|g| {
                g.set(g.get().wrapping_add(1));
                g.get()
            });
            IDLE.with(|i| i.set(false));
            if let Some(out) = c.stdout.take() {
                std::thread::spawn(move || {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        let now_idle = match line.trim() {
                            "idle" => true,
                            "active" => false,
                            _ => continue,
                        };
                        crate::on_main(move || {
                            if GEN.with(|g| g.get()) != generation {
                                return;
                            }
                            IDLE.with(|i| i.set(now_idle));
                            if now_idle {
                                idle(generation);
                            }
                        });
                    }
                });
            }
            CHILD.with(|x| *x.borrow_mut() = Some(c));
            true
        }
        Err(e) => {
            eprintln!("rusttv: couldn't start swayidle: {e}");
            false
        }
    }
}

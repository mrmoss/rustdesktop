//! Play, pause, skip and seek in whatever is playing - through MPRIS, the
//! D-Bus remote control browsers (Firefox, Chromium) and media players
//! offer for the video or music in them. Blocking: call from a worker
//! thread (`run` does).

use gtk::gio;
use gtk::glib::Variant;

const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

fn bus() -> Option<gio::DBusConnection> {
    gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()
}

fn call(
    bus: &gio::DBusConnection,
    name: &str,
    iface: &str,
    method: &str,
    args: Option<&Variant>,
) -> Option<Variant> {
    bus.call_sync(
        Some(name),
        "/org/mpris/MediaPlayer2",
        iface,
        method,
        args,
        None,
        gio::DBusCallFlags::NONE,
        2000,
        gio::Cancellable::NONE,
    )
    .ok()
}

/// The players there are, with whether each is playing now.
fn players(bus: &gio::DBusConnection) -> Vec<(String, String)> {
    let names = bus
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "ListNames",
            None,
            None,
            gio::DBusCallFlags::NONE,
            2000,
            gio::Cancellable::NONE,
        )
        .ok()
        .and_then(|v| v.child_value(0).get::<Vec<String>>())
        .unwrap_or_default();
    names
        .into_iter()
        .filter(|n| n.starts_with("org.mpris.MediaPlayer2."))
        .map(|n| {
            let status = call(
                bus,
                &n,
                "org.freedesktop.DBus.Properties",
                "Get",
                Some(&(PLAYER, "PlaybackStatus").into()),
            )
            .and_then(|v| v.child_value(0).as_variant())
            .and_then(|v| v.get::<String>())
            .unwrap_or_default();
            (n, status)
        })
        .collect()
}

/// The player to control: the one playing, else a paused one, else any.
fn target(ps: &[(String, String)]) -> Option<String> {
    ps.iter()
        .find(|p| p.1 == "Playing")
        .or_else(|| ps.iter().find(|p| p.1 == "Paused"))
        .or(ps.first())
        .map(|p| p.0.clone())
}

/// Do a media job: "play-pause", "play", "pause", "stop", "next",
/// "previous", "seek-forward", "seek-back" (10 seconds).
pub fn act(job: &str) {
    let Some(bus) = bus() else { return };
    let ps = players(&bus);
    let Some(p) = target(&ps) else {
        eprintln!("rusttv: nothing is playing (no MPRIS player) for {job}");
        return;
    };
    let (method, args): (&str, Option<Variant>) = match job {
        "play-pause" => ("PlayPause", None),
        "play" => ("Play", None),
        "pause" => ("Pause", None),
        "stop" => ("Stop", None),
        "next" => ("Next", None),
        "previous" => ("Previous", None),
        "seek-forward" => ("Seek", Some((10_000_000i64,).into())),
        "seek-back" => ("Seek", Some((-10_000_000i64,).into())),
        _ => return,
    };
    let _ = call(&bus, &p, PLAYER, method, args.as_ref());
}

/// Pause everything that's playing (going back to the home screen).
pub fn pause_all() {
    let Some(bus) = bus() else { return };
    for (p, status) in players(&bus) {
        if status == "Playing" {
            let _ = call(&bus, &p, PLAYER, "Pause", None);
        }
    }
}

/// Do it on a worker thread.
pub fn run(job: &str) {
    let job = job.to_string();
    std::thread::spawn(move || {
        if job == "pause-all" {
            pause_all();
        } else {
            act(&job);
        }
    });
}

//! Opening things: a launcher's web page in a browser of its own (kiosk mode:
//! full screen, no browser bars), or an installed app. Each opens on a
//! workspace of its own; Home goes back to RustTV.

use crate::config::Launcher;
use crate::sway;
use gtk::prelude::*;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The window name ("app id") a launcher's browser gets - how RustTV finds
/// it again among the open windows.
pub fn window_id(l: &Launcher) -> String {
    format!("rusttv-{}", l.id)
}

/// Where a browser program really is (None: not installed).
pub fn find(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(program))
        .find(|p| p.is_file())
}

/// How a browser is installed.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// a program (deb, tarball...): its path
    Program(PathBuf),
    /// a snap (Ubuntu's Firefox): may only use folders under ~/snap/<name>
    Snap(PathBuf),
    /// a Flatpak: its app id ("net.waterfox.waterfox")
    Flatpak(String),
}

/// The Flatpak app ids a browser comes as.
fn flatpak_ids(browser: &str) -> &'static [&'static str] {
    match browser {
        "firefox" => &["org.mozilla.firefox"],
        "waterfox" => &["net.waterfox.waterfox"],
        "chromium" => &[
            "org.chromium.Chromium",
            "io.github.ungoogled_software.ungoogled_chromium",
        ],
        "google-chrome" => &["com.google.Chrome"],
        _ => &[],
    }
}

/// Other places a browser's program can be (a download unpacked by hand).
fn other_paths(browser: &str) -> Vec<PathBuf> {
    let home = crate::config::home();
    match browser {
        "waterfox" => vec![
            PathBuf::from("/opt/waterfox/waterfox"),
            PathBuf::from("/usr/lib/waterfox/waterfox"),
            PathBuf::from("/usr/local/waterfox/waterfox"),
            home.join("waterfox/waterfox"),
            home.join(".local/share/waterfox/waterfox"),
            home.join("Applications/waterfox/waterfox"),
        ],
        "firefox" => vec![PathBuf::from("/opt/firefox/firefox")],
        "google-chrome" => vec![PathBuf::from("/opt/google/chrome/google-chrome")],
        _ => Vec::new(),
    }
}

fn flatpak_installed(id: &str) -> bool {
    let home = crate::config::home();
    [
        PathBuf::from("/var/lib/flatpak/app").join(id),
        home.join(".local/share/flatpak/app").join(id),
    ]
    .iter()
    .any(|d| d.join("current").exists())
}

/// Where (and how) a browser is installed: a program first, then a snap,
/// then a Flatpak.
pub fn install(browser: &str) -> Option<Install> {
    let names: &[&str] = match browser {
        "chromium" => &["chromium", "chromium-browser"],
        "google-chrome" => &["google-chrome", "google-chrome-stable"],
        "waterfox" => &["waterfox", "waterfox-g"],
        _ => &["firefox"],
    };
    let prog = names
        .iter()
        .find_map(|n| find(n))
        .or_else(|| other_paths(browser).into_iter().find(|p| p.is_file()));
    if let Some(p) = prog {
        let real = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
        let snap = real.starts_with("/snap")
            || std::path::Path::new(&format!("/snap/{browser}/current")).exists()
                && std::fs::read(&p).is_ok_and(|b| b.windows(5).any(|w| w == b"/snap"));
        return Some(if snap {
            Install::Snap(p)
        } else {
            Install::Program(p)
        });
    }
    if std::path::Path::new(&format!("/snap/bin/{browser}")).exists() {
        return Some(Install::Snap(PathBuf::from(format!("/snap/bin/{browser}"))));
    }
    if find("flatpak").is_some() {
        if let Some(id) = flatpak_ids(browser).iter().find(|id| flatpak_installed(id)) {
            return Some(Install::Flatpak(id.to_string()));
        }
    }
    None
}

pub fn installed(browser: &str) -> bool {
    install(browser).is_some()
}

/// The launcher's own browser profile: its sign-ins and settings, apart from
/// the others (and from your everyday browser). Snaps and Flatpaks may only
/// use their own folders, so it goes there for them.
fn profile_dir(l: &Launcher) -> PathBuf {
    let home = crate::config::home();
    match install(&l.browser) {
        Some(Install::Snap(_)) => home
            .join("snap")
            .join(&l.browser)
            .join("common")
            .join("rusttv")
            .join(&l.id),
        Some(Install::Flatpak(id)) => home.join(".var/app").join(id).join("rusttv").join(&l.id),
        _ => home.join(".local/share/rusttv/profiles").join(&l.id),
    }
}

/// What a TV's browser says it is: sites with a TV layout (YouTube's) show
/// it for this. (The one YouTube's TV page is known to take from a desktop.)
pub const TV_AGENT: &str = "Mozilla/5.0 (PS4; Leanback Shell) Gecko/20100101 Firefox/65.0 LeanbackShell/01.00.01.75 Sony PS4/ (PS4, , no, CH)";

/// Firefox settings for a TV: no first-run pages or default-browser
/// questions, protected (DRM) video on, hardware video decoding.
const USER_JS: &str = r#"// Written by RustTV each time it opens this launcher (these few settings
// come back if changed; anything else you change in about:config stays).
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("startup.homepage_welcome_url", "");
user_pref("startup.homepage_welcome_url.additional", "");
user_pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true);
user_pref("trailhead.firstrun.didSeeAboutWelcome", true);
user_pref("browser.tabs.warnOnClose", false);
user_pref("browser.sessionstore.resume_from_crash", false);
user_pref("media.eme.enabled", true);
user_pref("media.gmp-widevinecdm.visible", true);
user_pref("media.gmp-widevinecdm.enabled", true);
user_pref("media.autoplay.default", 0);
user_pref("media.ffmpeg.vaapi.enabled", true);
user_pref("full-screen-api.warning.timeout", 0);
user_pref("full-screen-api.transition-duration.enter", "0 0");
user_pref("full-screen-api.transition-duration.leave", "0 0");
user_pref("dom.disable_open_during_load", false);
"#;

/// Forget a launcher's sign-ins and settings (its browser profile).
pub fn forget_profile(l: &Launcher) {
    let l = &effective(l);
    let dir = profile_dir(l);
    // (only ever a folder RustTV made for it)
    if dir.ends_with(&l.id)
        && dir
            .components()
            .any(|c| c.as_os_str() == "rusttv" || c.as_os_str() == "profiles")
    {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

pub fn is_chromium(browser: &str) -> bool {
    matches!(browser, "chromium" | "google-chrome")
}

/// The launcher as it really opens: with "arrow keys jump between buttons"
/// on, in Chromium (or Chrome) - its own browser if that's one of them,
/// else whichever is installed.
pub fn effective(l: &Launcher) -> Launcher {
    let mut l = l.clone();
    if l.spatial && !is_chromium(&l.browser) {
        if let Some(b) = ["chromium", "google-chrome"]
            .into_iter()
            .find(|b| installed(b))
        {
            l.browser = b.to_string();
        } else {
            l.browser = "chromium".into(); // (not installed: says so)
        }
    }
    l
}

/// Chromium's usual profile folder (where it keeps your history).
fn default_chromium_dir(browser: &str, how: &Install) -> PathBuf {
    let home = crate::config::home();
    match how {
        Install::Snap(_) => home.join("snap").join(browser).join("common").join(browser),
        Install::Flatpak(app) => home.join(".var/app").join(app).join("config").join(browser),
        Install::Program(_) => home.join(".config").join(browser),
    }
}

/// The fingerprint of the key in the certificate `url`'s server shows now
/// (base64 SHA-256 of its public key - what Chromium's
/// --ignore-certificate-errors-spki-list takes). Uses openssl; None if the
/// site isn't https or can't be reached.
pub fn cert_fingerprint(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let hostport = rest.split(['/', '?', '#']).next()?;
    if hostport.is_empty()
        || !hostport
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c))
    {
        return None;
    }
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => (h, p),
        _ => (hostport, "443"),
    };
    let host = host.trim_matches(['[', ']']);
    // (checked above: letters, digits and . - : [ ] only - safe in a shell)
    let script = format!(
        "timeout 5 openssl s_client -connect '{hostport}' -servername '{host}' </dev/null 2>/dev/null \
         | openssl x509 -pubkey -noout | openssl pkey -pubin -outform der \
         | openssl dgst -sha256 -binary | openssl base64 -A",
        hostport = if port == "443" && !hostport.contains(':') {
            format!("{hostport}:443")
        } else {
            hostport.to_string()
        },
    );
    let out = std::process::Command::new("sh")
        .args(["-c", &script])
        .output()
        .ok()?;
    let fp = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // (the hash of nothing means it couldn't get a key)
    (fp.len() == 44 && fp != "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=").then_some(fp)
}

/// The command that opens a launcher (None: its browser isn't installed).
///
/// `shared`: open it with your browser's own profile (your history and
/// sign-ins, like opening the browser yourself) rather than one of its own.
/// "Say it's a TV" needs a profile of its own (it's a browser setting), so
/// such a launcher always gets one.
pub fn launcher_command(l: &Launcher, shared: bool) -> Option<Vec<String>> {
    let l = &effective(l);
    let how = install(&l.browser)?;
    // arrow keys jump between the page's buttons (Chromium only)
    let spatial = l.spatial && is_chromium(&l.browser);
    let mut cmd: Vec<String> = match &how {
        Install::Program(path) | Install::Snap(path) => vec![path.to_string_lossy().to_string()],
        Install::Flatpak(app) => vec!["flatpak".into(), "run".into(), app.clone()],
    };
    // the site's own certificate, trusted (Chromium: by its key's
    // fingerprint - that switch only works with a profile folder named, so
    // your usual one is named when sharing it)
    let trust = (l.trust_cert && is_chromium(&l.browser))
        .then(|| cert_fingerprint(&l.url))
        .flatten();
    if uses_own_profile(l, shared) {
        // (if the browser is open already, this opens a new window in it)
        match l.browser.as_str() {
            "chromium" | "google-chrome" => {
                if let Some(fp) = &trust {
                    cmd.push(format!(
                        "--user-data-dir={}",
                        default_chromium_dir(&l.browser, &how).to_string_lossy()
                    ));
                    cmd.push(format!("--ignore-certificate-errors-spki-list={fp}"));
                }
                cmd.extend([
                    "--ozone-platform-hint=auto".into(),
                    "--kiosk".into(),
                    "--new-window".into(),
                ]);
                if spatial {
                    cmd.push("--enable-spatial-navigation".into());
                }
                if l.tv {
                    cmd.push(format!("--user-agent={TV_AGENT}"));
                }
            }
            _ => cmd.extend(["--kiosk".into(), "--new-window".into()]),
        }
        cmd.push(l.url.clone());
        return Some(cmd);
    }
    let profile = profile_dir(l);
    let _ = std::fs::create_dir_all(&profile);
    let id = window_id(l);
    let p = profile.to_string_lossy().to_string();
    match l.browser.as_str() {
        "chromium" | "google-chrome" => {
            cmd.extend([
                format!("--user-data-dir={p}"),
                format!("--class={id}"),
                "--ozone-platform-hint=auto".into(),
                "--no-first-run".into(),
                "--no-default-browser-check".into(),
            ]);
            if let Some(fp) = &trust {
                cmd.push(format!("--ignore-certificate-errors-spki-list={fp}"));
            }
            if spatial {
                cmd.push("--enable-spatial-navigation".into());
            }
            cmd.extend(["--kiosk".into()]);
            if l.tv {
                cmd.push(format!("--user-agent={TV_AGENT}"));
            }
        }
        _ => {
            // (Firefox and Waterfox)
            let mut js = USER_JS.to_string();
            if l.tv {
                js.push_str(&format!(
                    "user_pref(\"general.useragent.override\", \"{TV_AGENT}\");\n"
                ));
            } else {
                // (a TV agent set before is taken off again: Firefox keeps
                // user.js settings in prefs.js)
                let prefs = profile.join("prefs.js");
                if let Ok(text) = std::fs::read_to_string(&prefs) {
                    if text.contains("general.useragent.override") {
                        let kept: String = text
                            .lines()
                            .filter(|l| !l.contains("\"general.useragent.override\""))
                            .map(|l| format!("{l}\n"))
                            .collect();
                        let _ = std::fs::write(&prefs, kept);
                    }
                }
            }
            let _ = std::fs::write(profile.join("user.js"), js);
            cmd.extend([
                "--no-remote".into(),
                "--profile".into(),
                p,
                "--name".into(),
                id,
                "--kiosk".into(),
            ]);
        }
    }
    cmd.push(l.url.clone());
    Some(cmd)
}

/// An installed app's command line, from its desktop file ("%U" and the
/// like dropped: nothing is being opened with it).
pub fn app_command(app: &gio_unix::DesktopAppInfo) -> Option<Vec<String>> {
    let line = app.commandline()?;
    let line = line.to_string_lossy();
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ' ' | '\t' if !quoted => {
                if any || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            _ => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        words.push(cur);
    }
    let words: Vec<String> = words
        .into_iter()
        .filter(|w| !(w.len() == 2 && w.starts_with('%')) && !w.starts_with("@@"))
        .map(|w| w.replace("%%", "%"))
        .collect();
    (!words.is_empty()).then_some(words)
}

thread_local! {
    /// when something was last opened (its window is shown when it appears)
    static OPENED_AT: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

/// Was something opened in the last half minute, with no window yet? (Then
/// a new window is it: show it.) Asking clears it.
pub fn take_just_opened() -> bool {
    OPENED_AT
        .with(|o| o.take())
        .is_some_and(|t| t.elapsed().as_secs() < 30)
}

/// Start `cmd`. Its window gets a workspace of its own when it appears (see
/// main::windows_changed); RustTV stays on screen until then.
pub fn open(cmd: &[String]) -> bool {
    let Some((prog, args)) = cmd.split_first() else {
        return false;
    };
    OPENED_AT.with(|o| o.set(Some(std::time::Instant::now())));
    let child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .env_remove("RUSTTV_CONTROL")
        .spawn();
    match child {
        Ok(mut c) => {
            // (collect it when it quits, so no zombie is left behind)
            std::thread::spawn(move || {
                let _ = c.wait();
            });
            true
        }
        Err(e) => {
            eprintln!("rusttv: couldn't start {prog}: {e}");
            OPENED_AT.with(|o| o.set(None));
            false
        }
    }
}

/// The process using a launcher's browser profile right now, from the lock
/// the browser leaves in it (Firefox: lock -> "host:+pid", Chromium:
/// SingletonLock -> "host-pid").
fn profile_pid(l: &Launcher) -> Option<i64> {
    let l = &effective(l);
    let dir = profile_dir(l);
    let target = std::fs::read_link(dir.join("lock"))
        .or_else(|_| std::fs::read_link(dir.join("SingletonLock")))
        .ok()?;
    let t = target.to_string_lossy();
    let pid: i64 = t.rsplit(['+', '-']).next()?.parse().ok()?;
    std::path::Path::new(&format!("/proc/{pid}"))
        .exists()
        .then_some(pid)
}

thread_local! {
    /// windows known to be a launcher's: (window, launcher id) - the
    /// window that appeared after it was opened
    static OWNERS: std::cell::RefCell<Vec<(i64, String)>> = const { std::cell::RefCell::new(Vec::new()) };
    /// the launcher opened last, waiting for its window
    static WAITING: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// one app at a time: close the others once the new one's window is
    /// here (closing first could catch a shared browser in the middle of
    /// quitting, and leaves a black screen while the new one starts)
    static CLOSE_FOR_NEW: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

thread_local! {
    /// how many apps may be open (0: no limit)
    static MAX_APPS: std::cell::Cell<u32> = const { std::cell::Cell::new(1) };
    /// app windows in the order they opened (the first is closed first)
    static ORDER: std::cell::RefCell<Vec<i64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// When the next new window appears (within a minute), close the apps
/// opened longest ago, so no more than `max` are open (0: no limit).
pub fn close_others_when_new(max: u32) {
    MAX_APPS.with(|m| m.set(max));
    if max > 0 {
        CLOSE_FOR_NEW.with(|c| c.set(Some(std::time::Instant::now())));
    }
}

/// Close the oldest apps so that, with `keep`, at most `max` are open.
fn trim_to(max: u32, keep: i64) {
    let wins = sway::windows();
    let mut order: Vec<i64> = ORDER.with(|o| o.borrow().clone());
    // (windows from before RustTV started: oldest)
    for w in &wins {
        if !order.contains(&w.id) {
            order.insert(0, w.id);
        }
    }
    let open: Vec<&sway::Window> = order
        .iter()
        .filter_map(|id| wins.iter().find(|w| w.id == *id))
        .collect();
    let extra = open.len().saturating_sub(max as usize);
    for w in open.into_iter().filter(|w| w.id != keep).take(extra) {
        close(w);
    }
}

/// A new window showed up: if a launcher was just opened, it's that one's.
pub fn new_window(w: &sway::Window) {
    if let Some(id) = WAITING.with(|x| x.borrow_mut().take()) {
        OWNERS.with(|o| o.borrow_mut().push((w.id, id)));
    }
    ORDER.with(|o| {
        let mut o = o.borrow_mut();
        o.retain(|id| *id != w.id);
        o.push(w.id);
    });
    if CLOSE_FOR_NEW
        .with(|c| c.take())
        .is_some_and(|t| t.elapsed().as_secs() < 60)
    {
        trim_to(MAX_APPS.with(|m| m.get()), w.id);
    }
}

/// Forget windows that have closed.
pub fn windows_left(wins: &[sway::Window]) {
    ORDER.with(|o| o.borrow_mut().retain(|id| wins.iter().any(|w| w.id == *id)));
    OWNERS.with(|o| {
        o.borrow_mut()
            .retain(|(id, _)| wins.iter().any(|w| w.id == *id))
    });
}

/// A launcher's open window: found by its window name, by the window that
/// came after opening it, or else by the browser that has its profile open.
pub fn launcher_window(l: &Launcher, wins: &[sway::Window]) -> Option<sway::Window> {
    let id = window_id(l);
    if let Some(w) = wins.iter().find(|w| w.app_id.eq_ignore_ascii_case(&id)) {
        return Some(w.clone());
    }
    let owned: Vec<i64> = OWNERS.with(|o| {
        o.borrow()
            .iter()
            .filter(|(_, lid)| *lid == l.id)
            .map(|(w, _)| *w)
            .collect()
    });
    if let Some(w) = wins.iter().find(|w| owned.contains(&w.id)) {
        return Some(w.clone());
    }
    let pid = profile_pid(l)?;
    wins.iter().find(|w| w.pid == pid).cloned()
}

/// Does this launcher open with your browser's own profile?
pub fn uses_own_profile(l: &Launcher, shared: bool) -> bool {
    // (Chromium says it's a TV with a command-line switch, not a profile
    // setting: it can share your profile)
    shared && (!l.tv || is_chromium(&effective(l).browser))
}

thread_local! {
    /// each open launcher as it was when it opened (to tell if it's been
    /// edited since)
    static OPENED_AS: std::cell::RefCell<Vec<Launcher>> = const { std::cell::RefCell::new(Vec::new()) };
    /// what the Chromium that's running with your profile was started with
    /// (says it's a TV, arrows jump): a new window in it gets the same
    static CHROMIUM_FLAGS: std::cell::Cell<Option<(bool, bool, bool)>> = const { std::cell::Cell::new(None) };
}

/// Has the launcher been edited (in a way that matters) since it opened?
pub fn changed_since_open(l: &Launcher) -> bool {
    OPENED_AS.with(|o| {
        o.borrow().iter().find(|x| x.id == l.id).is_some_and(|x| {
            x.url != l.url
                || x.browser != l.browser
                || x.tv != l.tv
                || x.spatial != l.spatial
                || x.trust_cert != l.trust_cert
        })
    })
}

fn chromium_window(w: &sway::Window) -> bool {
    let a = w.app_id.to_ascii_lowercase();
    a.contains("chromium") || a.contains("chrome")
}

/// The Chromium windows to close before opening `l`: Chromium (with your
/// profile) is open already, started for a launcher with different
/// switches - a new window would get those, not this one's.
pub fn chromium_in_the_way(l: &Launcher, shared: bool, wins: &[sway::Window]) -> Vec<sway::Window> {
    let real = effective(l);
    if !is_chromium(&real.browser) || !uses_own_profile(l, shared) {
        return Vec::new();
    }
    let want = (
        real.tv,
        real.spatial && is_chromium(&real.browser),
        real.trust_cert,
    );
    let open: Vec<sway::Window> = wins
        .iter()
        .filter(|w| chromium_window(w))
        .cloned()
        .collect();
    if open.is_empty() || CHROMIUM_FLAGS.with(|c| c.get()) == Some(want) {
        return Vec::new();
    }
    open
}

/// Open a launcher (its window is then known as its own).
pub fn open_launcher(l: &Launcher, shared: bool) -> bool {
    let Some(cmd) = launcher_command(l, shared) else {
        return false;
    };
    let real = effective(l);
    if is_chromium(&real.browser) && uses_own_profile(l, shared) {
        let first = !sway::windows().iter().any(chromium_window);
        if first {
            CHROMIUM_FLAGS.with(|c| c.set(Some((real.tv, real.spatial, real.trust_cert))));
        }
    }
    OPENED_AS.with(|o| {
        let mut o = o.borrow_mut();
        o.retain(|x| x.id != l.id);
        o.push(l.clone());
    });
    WAITING.with(|x| *x.borrow_mut() = Some(l.id.clone()));
    open(&cmd)
}

/// Close `wins`, then (once they're gone - or after 8 seconds) run `then`.
pub fn close_then(wins: Vec<sway::Window>, then: impl FnOnce() + 'static) {
    for w in &wins {
        close(w);
    }
    let ids: Vec<i64> = wins.iter().map(|w| w.id).collect();
    // (and their programs: a browser saves its sign-ins as it quits - one
    // started on the same profile before that would lose them)
    let pids: Vec<i64> = wins.iter().map(|w| w.pid).filter(|p| *p > 0).collect();
    let started = std::time::Instant::now();
    let then = std::cell::RefCell::new(Some(then));
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        let windows_left = sway::windows().iter().any(|w| ids.contains(&w.id));
        let programs_left = pids
            .iter()
            .any(|p| std::path::Path::new(&format!("/proc/{p}")).exists());
        if (windows_left || programs_left) && started.elapsed().as_secs() < 10 {
            return gtk::glib::ControlFlow::Continue;
        }
        if let Some(f) = then.borrow_mut().take() {
            let _ = gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(300), f);
        }
        gtk::glib::ControlFlow::Break
    });
}

/// Show a window (its workspace).
pub fn focus(w: &sway::Window) {
    sway::run(&format!("[con_id={}] focus", w.id));
}

pub fn close(w: &sway::Window) {
    sway::run(&format!("[con_id={}] kill", w.id));
}

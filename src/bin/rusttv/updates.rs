//! Checking for and installing updates: the system's packages (apt), snaps
//! and Flatpaks. Changing the system needs permission: the helper
//! /usr/local/libexec/rusttv-update has it (install-rusttv.sh puts it there
//! with a polkit rule for the person at the TV - it only ever updates what's
//! installed, from the sources already set up).
//!
//! Everything here blocks: call it from a worker thread.

use std::process::Command;

pub const HELPER: &str = "/usr/local/libexec/rusttv-update";

/// How many updates there are of each kind (None: that kind isn't
/// installed here, or couldn't be checked).
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub apt: Option<usize>,
    pub snap: Option<usize>,
    pub flatpak: Option<usize>,
}

impl Found {
    pub fn total(&self) -> usize {
        self.apt.unwrap_or(0) + self.snap.unwrap_or(0) + self.flatpak.unwrap_or(0)
    }
}

fn have(p: &str) -> bool {
    crate::launch::find(p).is_some()
}

fn out(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(cmd).args(args).output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).to_string())
}

fn helper(what: &str) -> Result<(), String> {
    if !std::path::Path::new(HELPER).exists() {
        return Err("the update helper isn't installed (run install-rusttv.sh)".into());
    }
    let o = Command::new("pkexec")
        .arg(HELPER)
        .arg(what)
        .output()
        .map_err(|e| format!("couldn't ask for permission: {e}"))?;
    if o.status.success() {
        Ok(())
    } else {
        let e = String::from_utf8_lossy(&o.stderr).trim().to_string();
        Err(e.lines().last().unwrap_or("it failed").to_string())
    }
}

/// Look for updates (refreshes the package lists first).
pub fn check() -> Result<Found, String> {
    // (never fails now - kept a Result for the page)
    let mut f = Found::default();
    if have("apt-get") {
        // (if the lists can't be fetched, what's known already is counted)
        if let Err(e) = helper("refresh") {
            eprintln!("rusttv: couldn't refresh the package lists: {e}");
        }
        f.apt = out("apt", &["list", "--upgradable"])
            .map(|t| t.lines().filter(|l| l.contains("[upgradable from")).count());
    }
    if have("snap") {
        // ("All snaps up to date." goes to stderr: no lines here)
        f.snap = out("snap", &["refresh", "--list"])
            .map(|t| t.lines().skip(1).filter(|l| !l.trim().is_empty()).count())
            .or(Some(0));
    }
    if have("flatpak") {
        f.flatpak = out(
            "flatpak",
            &["remote-ls", "--updates", "--columns=application"],
        )
        .map(|t| {
            t.lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with("Application"))
                .count()
        });
    }
    Ok(f)
}

/// Install them all. Ok(true): restart to finish.
pub fn install() -> Result<bool, String> {
    helper("upgrade")?;
    // your own (not system-wide) Flatpaks need no permission
    if have("flatpak") {
        let _ = Command::new("flatpak")
            .args(["update", "--user", "-y", "--noninteractive"])
            .status();
    }
    Ok(std::path::Path::new("/var/run/reboot-required").exists())
}

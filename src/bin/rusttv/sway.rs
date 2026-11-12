//! Talking to sway ($SWAYSOCK): RustTV lives on the workspace "home"; every
//! app opens full screen on a workspace of its own.
//!
//! Wire format: "i3-ipc" + payload length (u32 LE) + message type (u32 LE) + JSON.

use serde_json::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

const RUN_COMMAND: u32 = 0;
const GET_WORKSPACES: u32 = 1;
const SUBSCRIBE: u32 = 2;
const GET_TREE: u32 = 4;
const GET_OUTPUTS: u32 = 3;
const MAX_REPLY: usize = 16 * 1024 * 1024;

pub const HOME: &str = "home";

pub fn available() -> bool {
    std::env::var_os("SWAYSOCK").is_some()
}

fn connect() -> Option<UnixStream> {
    let s = UnixStream::connect(std::env::var_os("SWAYSOCK")?).ok()?;
    let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
    Some(s)
}

fn send(s: &mut UnixStream, kind: u32, payload: &str) -> Option<()> {
    let mut msg = Vec::with_capacity(14 + payload.len());
    msg.extend_from_slice(b"i3-ipc");
    msg.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    msg.extend_from_slice(&kind.to_le_bytes());
    msg.extend_from_slice(payload.as_bytes());
    s.write_all(&msg).ok()
}

fn recv(s: &mut UnixStream) -> Option<(u32, Value)> {
    let mut head = [0u8; 14];
    s.read_exact(&mut head).ok()?;
    if &head[..6] != b"i3-ipc" {
        return None;
    }
    let len = u32::from_le_bytes([head[6], head[7], head[8], head[9]]) as usize;
    let kind = u32::from_le_bytes([head[10], head[11], head[12], head[13]]);
    if len > MAX_REPLY {
        return None;
    }
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).ok()?;
    Some((kind, serde_json::from_slice(&body).ok()?))
}

fn request(kind: u32, payload: &str) -> Option<Value> {
    let mut s = connect()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    send(&mut s, kind, payload)?;
    recv(&mut s).map(|r| r.1)
}

/// Run sway commands (";"-separated). True if all of them worked.
pub fn run(cmd: &str) -> bool {
    let Some(v) = request(RUN_COMMAND, cmd) else {
        return false;
    };
    let ok = v
        .as_array()
        .is_some_and(|a| a.iter().all(|r| r["success"].as_bool().unwrap_or(false)));
    if !ok {
        eprintln!("rusttv: sway didn't do `{cmd}`: {v}");
    }
    ok
}

/// An app window.
#[derive(Clone, Debug)]
pub struct Window {
    pub id: i64,
    pub pid: i64,
    pub app_id: String,
    pub title: String,
    pub workspace: String,
}

fn collect(node: &Value, ws: &str, out: &mut Vec<Window>) {
    let ws = if node["type"] == "workspace" {
        node["name"].as_str().unwrap_or_default()
    } else {
        ws
    };
    let is_view = node["pid"].is_number() && node["nodes"].as_array().is_some_and(|n| n.is_empty());
    if is_view {
        let app_id = node["app_id"]
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                node["window_properties"]["class"]
                    .as_str()
                    .map(str::to_string)
            })
            .unwrap_or_default();
        out.push(Window {
            id: node["id"].as_i64().unwrap_or(0),
            pid: node["pid"].as_i64().unwrap_or(0),
            app_id,
            title: node["name"].as_str().unwrap_or_default().to_string(),
            workspace: ws.to_string(),
        });
    }
    for key in ["nodes", "floating_nodes"] {
        for n in node[key].as_array().into_iter().flatten() {
            collect(n, ws, out);
        }
    }
}

/// Every app window (not RustTV's own).
pub fn windows() -> Vec<Window> {
    let mut out = Vec::new();
    if let Some(t) = request(GET_TREE, "") {
        collect(&t, "", &mut out);
    }
    // (not RustTV itself, nor the copies on other screens)
    out.retain(|w| {
        w.app_id != crate::APP_ID
            && w.app_id != crate::screens::MIRROR_APP
            && w.workspace != "__i3_scratch"
    });
    out
}

/// The app window that has the focus (None: RustTV itself, or nothing).
pub fn focused_window() -> Option<Window> {
    fn find(node: &Value) -> Option<i64> {
        if node["focused"].as_bool() == Some(true) && node["pid"].is_number() {
            return node["id"].as_i64();
        }
        for key in ["nodes", "floating_nodes"] {
            for n in node[key].as_array().into_iter().flatten() {
                if let Some(id) = find(n) {
                    return Some(id);
                }
            }
        }
        None
    }
    let id = find(&request(GET_TREE, "")?)?;
    windows().into_iter().find(|w| w.id == id)
}

/// The screens, as sway describes them.
pub fn outputs_json() -> Vec<Value> {
    request(GET_OUTPUTS, "")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

/// The workspace on screen now.
pub fn current_workspace() -> String {
    request(GET_WORKSPACES, "")
        .and_then(|v| {
            v.as_array()?
                .iter()
                .find(|w| w["focused"].as_bool() == Some(true))
                .and_then(|w| w["name"].as_str().map(str::to_string))
        })
        .unwrap_or_default()
}

/// A numbered workspace nobody uses (for the next app).
pub fn free_workspace() -> String {
    let used: Vec<String> = request(GET_WORKSPACES, "")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|w| w["name"].as_str().map(str::to_string))
        .collect();
    (1..)
        .map(|n: u32| n.to_string())
        .find(|n| !used.contains(n))
        .unwrap_or_else(|| "99".into())
}

/// Calls `f` (on a thread of its own) for every window event: "new",
/// "close", "focus", ... Reconnects if sway restarts the socket.
pub fn watch_windows(f: impl Fn(String) + Send + 'static) {
    std::thread::spawn(move || loop {
        if let Some(mut s) = connect() {
            if send(&mut s, SUBSCRIBE, "[\"window\",\"workspace\",\"output\"]").is_some() {
                let _ = recv(&mut s); // {"success": true}
                while let Some((_, ev)) = recv(&mut s) {
                    if let Some(change) = ev["change"].as_str() {
                        f(change.to_string());
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    });
}

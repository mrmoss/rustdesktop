//! Talking to sway over its IPC socket ($SWAYSOCK). Only used when RustTouch runs
//! on sway: sway lets the shell arrange windows (split screen), which phoc and
//! labwc don't allow.
//!
//! Wire format: "i3-ipc" + payload length (u32 LE) + message type (u32 LE) + JSON.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde_json::Value;

const RUN_COMMAND: u32 = 0;
const GET_WORKSPACES: u32 = 1;
const SUBSCRIBE: u32 = 2;
const GET_TREE: u32 = 4;

pub fn available() -> bool {
    std::env::var_os("SWAYSOCK").is_some()
}

fn connect() -> Option<UnixStream> {
    let path = std::env::var_os("SWAYSOCK")?;
    let s = UnixStream::connect(path).ok()?;
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

/// (message type, JSON payload)
fn recv(s: &mut UnixStream) -> Option<(u32, Value)> {
    let mut head = [0u8; 14];
    s.read_exact(&mut head).ok()?;
    if &head[..6] != b"i3-ipc" {
        return None;
    }
    let len = u32::from_le_bytes(head[6..10].try_into().ok()?) as usize;
    let kind = u32::from_le_bytes(head[10..14].try_into().ok()?);
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
    let ok = request(RUN_COMMAND, cmd)
        .and_then(|v| v.as_array().cloned())
        .map(|a| a.iter().all(|r| r["success"].as_bool().unwrap_or(false)))
        .unwrap_or(false);
    if !ok {
        eprintln!("rusttouch: sway command failed: {cmd}");
    }
    ok
}

/// Lowest workspace number nobody is using (new apps get a workspace each).
pub fn free_workspace() -> i64 {
    let used: Vec<i64> = request(GET_WORKSPACES, "")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|w| w["num"].as_i64())
        .collect();
    (1..).find(|n| !used.contains(n)).unwrap_or(1)
}

/// Call `f(change, container)` for every window event, on a background thread.
pub fn watch_windows(f: impl Fn(&str, &Value) + Send + 'static) {
    std::thread::spawn(move || {
        let Some(mut s) = connect() else { return };
        if send(&mut s, SUBSCRIBE, r#"["window","workspace"]"#).is_none() {
            return;
        }
        let _ = recv(&mut s); // {"success": true}
        while let Some((kind, v)) = recv(&mut s) {
            match kind & 0x7fff_ffff {
                3 => f(v["change"].as_str().unwrap_or(""), &v["container"]),
                0 => f("workspace", &Value::Null),
                _ => {}
            }
        }
        eprintln!("rusttouch: lost the sway event connection");
    });
}

// ---------------------------------------------------------------- the tree

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn of(v: &Value) -> Rect {
        let n = |k: &str| v[k].as_i64().unwrap_or(0) as i32;
        Rect {
            x: n("x"),
            y: n("y"),
            w: n("width"),
            h: n("height"),
        }
    }
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= f64::from(self.x)
            && y >= f64::from(self.y)
            && x < f64::from(self.x + self.w)
            && y < f64::from(self.y + self.h)
    }
}

#[derive(Clone, Debug)]
pub struct Win {
    pub id: i64,
    pub rect: Rect,
    /// Wayland app_id, or the X11 class for XWayland apps
    pub app_id: String,
    pub title: String,
    /// an X11 app (draws no title bar of its own under sway)
    pub xwayland: bool,
}

#[derive(Clone, Debug)]
pub enum Node {
    Win(Win),
    Split {
        id: i64,
        /// true = side by side (splith), false = top and bottom (splitv)
        horizontal: bool,
        rect: Rect,
        children: Vec<Node>,
    },
}

impl Node {
    pub fn rect(&self) -> Rect {
        match self {
            Node::Win(w) => w.rect,
            Node::Split { rect, .. } => *rect,
        }
    }
    pub fn id(&self) -> i64 {
        match self {
            Node::Win(w) => w.id,
            Node::Split { id, .. } => *id,
        }
    }
    pub fn windows(&self) -> Vec<Win> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }
    fn collect(&self, out: &mut Vec<Win>) {
        match self {
            Node::Win(w) => out.push(w.clone()),
            Node::Split { children, .. } => children.iter().for_each(|c| c.collect(out)),
        }
    }
    /// How many containers sit between this node and `id` (None = not inside).
    pub fn depth_of(&self, id: i64) -> Option<usize> {
        match self {
            Node::Win(w) => (w.id == id).then_some(0),
            Node::Split {
                id: me, children, ..
            } => {
                if *me == id {
                    return Some(0);
                }
                children.iter().find_map(|c| c.depth_of(id).map(|d| d + 1))
            }
        }
    }

    /// The split that directly holds `id`.
    pub fn parent_of(&self, id: i64) -> Option<&Node> {
        let Node::Split { children, .. } = self else {
            return None;
        };
        if children.iter().any(|c| c.id() == id) {
            return Some(self);
        }
        children.iter().find_map(|c| c.parent_of(id))
    }
}

/// What's showing on one monitor.
#[derive(Clone, Debug)]
pub struct Screen {
    pub output: Rect,
    /// the visible workspace's number and area
    pub num: i64,
    pub area: Rect,
    /// the visible workspace's tiled windows (the workspace itself is the root split)
    pub root: Node,
}

fn node(v: &Value) -> Option<Node> {
    let kids: Vec<Node> = v["nodes"]
        .as_array()
        .map(|a| a.iter().filter_map(node).collect())
        .unwrap_or_default();
    let id = v["id"].as_i64()?;
    let rect = Rect::of(&v["rect"]);
    let is_window = v["pid"].as_i64().is_some() && kids.is_empty() && v["type"] == "con";
    if is_window {
        let app_id = v["app_id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| v["window_properties"]["class"].as_str())
            .unwrap_or("")
            .to_string();
        return Some(Node::Win(Win {
            id,
            rect,
            app_id,
            title: v["name"].as_str().unwrap_or("").to_string(),
            xwayland: v["shell"] == "xwayland",
        }));
    }
    if kids.is_empty() {
        return None;
    }
    // tabbed/stacked containers are treated like a side-by-side split
    let horizontal = v["layout"] != "splitv" && v["layout"] != "stacked";
    Some(Node::Split {
        id,
        horizontal,
        rect,
        children: kids,
    })
}

/// The visible workspace on `output` (connector name like "eDP-1"; None = first).
pub fn screen(output: Option<&str>) -> Option<Screen> {
    let tree = request(GET_TREE, "")?;
    let outputs = tree["nodes"].as_array()?;
    let out = outputs
        .iter()
        .filter(|o| o["name"] != "__i3")
        .find(|o| output.map(|n| o["name"] == n).unwrap_or(true))
        .or_else(|| outputs.iter().find(|o| o["name"] != "__i3"))?;
    let current = out["current_workspace"].as_str().unwrap_or("");
    let ws = out["nodes"]
        .as_array()?
        .iter()
        .find(|w| w["name"] == current)?;
    let root = match node(ws) {
        Some(n) => n,
        None => Node::Split {
            id: ws["id"].as_i64()?,
            horizontal: true,
            rect: Rect::of(&ws["rect"]),
            children: Vec::new(),
        },
    };
    Some(Screen {
        output: Rect::of(&out["rect"]),
        num: ws["num"].as_i64().unwrap_or(-1),
        area: Rect::of(&ws["rect"]),
        root,
    })
}

/// Every tiled window on every workspace (to find a window by app id and title).
pub fn all_windows() -> Vec<Win> {
    let Some(tree) = request(GET_TREE, "") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    fn walk(v: &Value, out: &mut Vec<Win>) {
        if let Some(Node::Win(w)) = (v["type"] == "con" && v["pid"].as_i64().is_some())
            .then(|| node(v))
            .flatten()
        {
            out.push(w);
            return;
        }
        if let Some(a) = v["nodes"].as_array() {
            a.iter().for_each(|c| walk(c, out));
        }
    }
    walk(&tree, &mut out);
    out
}

/// One workspace = one "window group" in recents.
#[derive(Clone, Debug)]
pub struct Group {
    /// workspace number
    pub num: i64,
    pub rect: Rect,
    pub root: Node,
}

/// Every workspace with windows on `output` (None = first monitor).
pub fn groups(output: Option<&str>) -> Vec<Group> {
    let Some(tree) = request(GET_TREE, "") else {
        return Vec::new();
    };
    let Some(outputs) = tree["nodes"].as_array() else {
        return Vec::new();
    };
    let Some(out) = outputs
        .iter()
        .filter(|o| o["name"] != "__i3")
        .find(|o| output.map(|n| o["name"] == n).unwrap_or(true))
        .or_else(|| outputs.iter().find(|o| o["name"] != "__i3"))
    else {
        return Vec::new();
    };
    out["nodes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|ws| {
                    let root = node(ws)?;
                    (!root.windows().is_empty()).then(|| Group {
                        num: ws["num"].as_i64().unwrap_or(-1),
                        rect: Rect::of(&ws["rect"]),
                        root,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Is the visible workspace on `output` empty (the home screen shows through)?
pub fn showing_nothing(output: Option<&str>) -> bool {
    screen(output)
        .map(|s| s.root.windows().is_empty())
        .unwrap_or(false)
}

//! Instance discovery: every running server writes a small JSON file to a
//! per-user directory and removes it on shutdown. Readers validate each
//! entry by asking the server itself (`GET /instance`), so files left behind
//! by a crash are ignored and cleaned up rather than trusted.
//!
//! Directory ([`dir`]):
//! * Windows: `%LOCALAPPDATA%\noob-vst-webgui-framework\instances`
//! * macOS:   `~/Library/Application Support/noob-vst-webgui-framework/instances`
//! * Linux:   `$XDG_RUNTIME_DIR/noob-vst-webgui-framework/instances`, else `~/.local/state/noob-vst-webgui-framework/instances`
//!
//! File name: `<pid>-<port>.json`, holding an [`Instance`] as pretty JSON.
//!
//! # Who uses it
//!
//! * [`crate::serve`] publishes a record when `ServerConfig::discovery` is
//!   on (the default) and removes it when the `ServerHandle` is dropped.
//! * The `/instances` HTTP endpoint calls [`list_live`] so a page can list
//!   the other instances on the machine.
//! * `tools/instances.mjs` does the same scan from the shell.
//!
//! Everything here is blocking, plain-`std` I/O; call it from a worker or a
//! `spawn_blocking` task, never from the audio thread.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// What a running instance advertises: the JSON body of `GET /instance`
/// and the content of its discovery file. Field names are the JSON keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Instance {
    /// The bridge name given to `NoobVstWebguiFramework::builder`.
    pub name: String,
    /// Process id of the server. Used to tell a live record from a stale
    /// one whose port was reused by another process.
    pub pid: u32,
    /// TCP port on `127.0.0.1`.
    pub port: u16,
    /// `http://127.0.0.1:<port>/`.
    pub url: String,
    /// Unix seconds when the server started.
    pub started: u64,
    /// The wire protocol version the server speaks
    /// ([`crate::wire::PROTOCOL_VERSION`]).
    pub protocol: u16,
}

impl Instance {
    /// A record for this process serving `name` on `port`, stamped with the
    /// current time.
    pub fn new(name: &str, port: u16) -> Self {
        Instance {
            name: name.to_string(),
            pid: std::process::id(),
            port,
            url: format!("http://127.0.0.1:{port}/"),
            started: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            protocol: crate::wire::PROTOCOL_VERSION,
        }
    }

    /// `<pid>-<port>.json`.
    pub fn file_name(&self) -> String {
        format!("{}-{}.json", self.pid, self.port)
    }
}

/// The per-user instances directory, if one can be determined (see the
/// module docs for the per-platform location). `None` when the relevant
/// environment variable is unset; discovery is then silently disabled.
pub fn dir() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
    }?;
    Some(base.join("noob-vst-webgui-framework").join("instances"))
}

/// Write the instance file (creating the directory). Returns its path, or
/// `None` if the directory is unavailable or the write fails (discovery
/// then silently does nothing).
pub fn publish(instance: &Instance) -> Option<PathBuf> {
    let dir = dir()?;
    fs::create_dir_all(&dir).ok()?;
    let path = dir.join(instance.file_name());
    let json = serde_json::to_string_pretty(instance).ok()?;
    fs::write(&path, json).ok()?;
    Some(path)
}

/// Remove an instance file written by [`publish`]. Errors are ignored.
pub fn unpublish(path: &PathBuf) {
    let _ = fs::remove_file(path);
}

/// Every instance file on disk, unvalidated (may include crash leftovers).
/// Files that do not parse as an [`Instance`] are skipped.
pub fn list_files() -> Vec<(PathBuf, Instance)> {
    let Some(dir) = dir() else { return Vec::new() };
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&path)
            && let Ok(inst) = serde_json::from_str::<Instance>(&text)
        {
            out.push((path, inst));
        }
    }
    out
}

/// Ask the server on `port` who it is (blocking, with `timeout` applied to
/// connect, read and write). Returns `None` if nothing answers within the
/// timeout or the answer is not a noob-vst-webgui-framework `/instance` body.
///
/// This is a minimal hand-written HTTP/1.1 GET, so the crate needs no HTTP
/// client dependency.
pub fn probe(port: u16, timeout: Duration) -> Option<Instance> {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let mut s = TcpStream::connect_timeout(&addr, timeout).ok()?;
    s.set_read_timeout(Some(timeout)).ok()?;
    s.set_write_timeout(Some(timeout)).ok()?;
    s.write_all(b"GET /instance HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .ok()?;
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let body = text.split("\r\n\r\n").nth(1)?;
    serde_json::from_str(body.trim()).ok()
}

/// Live instances only, oldest first (then by port). Every file is
/// validated with [`probe`]; files whose server no longer answers, or
/// answers as a different process, are deleted on the way. Blocks for up to
/// `timeout` per file.
pub fn list_live(timeout: Duration) -> Vec<Instance> {
    let mut out = Vec::new();
    for (path, inst) in list_files() {
        match probe(inst.port, timeout) {
            Some(live) if live.pid == inst.pid => out.push(live),
            _ => unpublish(&path),
        }
    }
    out.sort_by(|a, b| a.started.cmp(&b.started).then(a.port.cmp(&b.port)));
    out
}

//! File-backed persistence for the UI store, for hosts that have no plug-in
//! state to ride on (standalone binaries, tools). A plug-in should persist
//! the store inside its own state instead (the nih-plug adapter's
//! `StoreSlot` does that).
//!
//! The UI store itself lives in the bridge (see the `store_*` methods on
//! [`Vst3WebStratum`]): a JSON object the page reads and writes through
//! `client.store`, shared by every client of the instance. [`FileStore`]
//! loads it from a file at start-up, marks it dirty on every change through
//! the store hook, and writes it back when the host loop calls
//! [`flush`](FileStore::flush).
//!
//! ```ignore
//! let store = FileStore::attach(&bridge, FileStore::default_path("my-app"));
//! loop {
//!     store.flush().ok();   // writes only when something changed
//!     // ... host work ...
//! }
//! ```
//!
//! Writes are atomic at the file level: the JSON goes to `<path>.tmp` first
//! and is renamed over the target, so a crash mid-write cannot leave a
//! truncated store.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Vst3WebStratum;
use crate::bridge::StoreHook;

/// Keeps a [`Vst3WebStratum`]'s UI store in a JSON file.
///
/// Owns the bridge's store hook while alive; dropping it flushes once more
/// and removes the hook. Only one `FileStore` (or other store hook) should
/// be attached to a bridge at a time.
pub struct FileStore {
    bridge: Vst3WebStratum,
    path: PathBuf,
    /// Set by the store hook on any change, cleared by `flush`.
    dirty: Arc<AtomicBool>,
}

impl FileStore {
    /// Load `path` into the store (a missing file is an empty store) and
    /// watch for changes. An unreadable or malformed file is logged and
    /// ignored, leaving the store as it was.
    ///
    /// Loading replaces the whole store and pushes `store.all` to every
    /// connected client, so call this before or right after `serve`.
    pub fn attach(bridge: &Vst3WebStratum, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        match std::fs::read_to_string(&path) {
            Ok(json) => {
                if let Err(e) = bridge.store_load_json(&json) {
                    log::warn!(
                        "bridge: ignoring unreadable UI store {}: {e}",
                        path.display()
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("bridge: could not read UI store {}: {e}", path.display()),
        }
        let dirty = Arc::new(AtomicBool::new(false));
        let flag = dirty.clone();
        let hook: StoreHook = Arc::new(move |_key: &str, _value: &serde_json::Value| {
            flag.store(true, Ordering::Release);
        });
        bridge.set_store_hook(Some(hook));
        FileStore {
            bridge: bridge.clone(),
            path,
            dirty,
        }
    }

    /// Where the file lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write the store out if anything changed since the last flush. Returns
    /// whether a write happened. Call it from the host loop; it is one
    /// atomic swap when nothing changed.
    ///
    /// # Errors
    ///
    /// Any I/O error from creating the parent directory, writing the
    /// temporary file or renaming it. The dirty flag is already cleared, so
    /// a failed flush is retried only after the next change.
    pub fn flush(&self) -> std::io::Result<bool> {
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return Ok(false);
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, self.bridge.store_json())?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(true)
    }

    /// `<per-user data dir>/vst3-web-stratum/<name>.store.json`, next to the
    /// discovery records (see [`crate::discovery::dir`]); falls back to the
    /// system temp directory when no per-user directory can be found.
    #[cfg(feature = "server")]
    pub fn default_path(name: &str) -> PathBuf {
        crate::discovery::dir()
            .and_then(|d| d.parent().map(Path::to_path_buf))
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("{name}.store.json"))
    }
}

impl Drop for FileStore {
    /// Flush pending changes (errors ignored) and detach the store hook.
    fn drop(&mut self) {
        let _ = self.flush();
        self.bridge.set_store_hook(None);
    }
}

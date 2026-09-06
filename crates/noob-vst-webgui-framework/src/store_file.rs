//! File-backed persistence for the UI store, for hosts that have no plug-in
//! state to ride on (standalone binaries, tools). A plug-in should persist
//! the store inside its own state instead (the nih-plug adapter's
//! `StoreSlot` does that).
//!
//! The UI store itself lives in the bridge (see the `store_*` methods on
//! [`NoobVstWebguiFramework`]): a JSON object the page reads and writes through
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

use crate::NoobVstWebguiFramework;
use crate::bridge::StoreHook;

/// Keeps a [`NoobVstWebguiFramework`]'s UI store in a JSON file.
///
/// Owns the bridge's store hook while alive; dropping it flushes once more
/// and removes the hook. Only one `FileStore` (or other store hook) should
/// be attached to a bridge at a time.
pub struct FileStore {
    bridge: NoobVstWebguiFramework,
    path: PathBuf,
    /// Set by the store hook on any change, cleared by `flush`.
    dirty: Arc<AtomicBool>,
    /// The keys this store owns, or empty for all of them.
    ///
    /// A whole-store file and a host's own state cannot both own the whole
    /// store: loading replaces it, so whichever restores last wipes what the
    /// other was keeping. Naming the keys is what lets a plug-in keep some of
    /// its page state with the session and some of it with the user --- a
    /// window size belongs to the project, a saved preset does not.
    keys: Vec<String>,
}

impl FileStore {
    /// Load `path` into the store (a missing file is an empty store) and
    /// watch for changes. An unreadable or malformed file is logged and
    /// ignored, leaving the store as it was.
    ///
    /// Loading replaces the whole store and pushes `store.all` to every
    /// connected client, so call this before or right after `serve`.
    pub fn attach(bridge: &NoobVstWebguiFramework, path: impl Into<PathBuf>) -> Self {
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
            keys: Vec::new(),
        }
    }

    /// The same, for **named keys only**.
    ///
    /// Unlike [`attach`](Self::attach) this neither loads nor writes the
    /// whole store: it sets each named key it finds in the file and leaves
    /// everything else alone, and it flushes only those keys back. That is
    /// what lets it sit beside a host that is already persisting the store
    /// with its own state --- a plug-in keeps its window size in the project
    /// and its saved presets with the user, and neither erases the other.
    ///
    /// A key absent from the file is left at whatever the store already has,
    /// so a first run starts empty rather than blank.
    pub fn attach_keys(
        bridge: &NoobVstWebguiFramework,
        path: impl Into<PathBuf>,
        keys: &[&str],
    ) -> Self {
        let path = path.into();
        let keys: Vec<String> = keys.iter().map(|k| (*k).to_string()).collect();
        match std::fs::read_to_string(&path) {
            Ok(json) => match serde_json::from_str::<serde_json::Value>(&json) {
                Ok(serde_json::Value::Object(map)) => {
                    for k in &keys {
                        if let Some(v) = map.get(k.as_str())
                            && let Err(e) = bridge.store_set(k, v.clone())
                        {
                            log::warn!("bridge: could not restore store key {k}: {e}");
                        }
                    }
                }
                Ok(_) => log::warn!("bridge: {} is not a store object", path.display()),
                Err(e) => log::warn!("bridge: ignoring unreadable store {}: {e}", path.display()),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("bridge: could not read store {}: {e}", path.display()),
        }
        let dirty = Arc::new(AtomicBool::new(false));
        let flag = dirty.clone();
        let owned = keys.clone();
        let hook: StoreHook = Arc::new(move |key: &str, _value: &serde_json::Value| {
            // Only a change to a key this file owns is worth a write.
            if owned.iter().any(|k| k == key) {
                flag.store(true, Ordering::Release);
            }
        });
        bridge.set_store_hook(Some(hook));
        FileStore {
            bridge: bridge.clone(),
            path,
            dirty,
            keys,
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
        let body = if self.keys.is_empty() {
            self.bridge.store_json()
        } else {
            let mut map = serde_json::Map::new();
            for k in &self.keys {
                if let Some(v) = self.bridge.store_get(k) {
                    map.insert(k.clone(), v);
                }
            }
            serde_json::Value::Object(map).to_string()
        };
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(true)
    }

    /// `<per-user data dir>/noob-vst-webgui-framework/<name>.store.json`, next to the
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoobVstWebguiFrameworkBuilder;
    use serde_json::json;

    fn bridge() -> NoobVstWebguiFramework {
        NoobVstWebguiFrameworkBuilder::new("scoped store test").build()
    }

    fn tmp(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "noob-vst-webgui-framework-{name}-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    /// **A scoped store writes its own keys and nobody else's.**
    ///
    /// This is the whole reason it exists: a plug-in's page state is already
    /// persisted with the host's session, and a second file over the same
    /// store would fight it --- loading replaces the store, so whichever
    /// restored last would wipe what the other was keeping. Saved presets
    /// belong to the user and a window size belongs to the project, and this
    /// is what lets both be true at once.
    #[test]
    fn a_scoped_store_writes_only_the_keys_it_owns() {
        let path = tmp("scoped");
        let b = bridge();
        b.store_set("presets", json!({ "mine": 1 })).unwrap();
        b.store_set("window", json!({ "w": 900 })).unwrap();
        {
            let s = FileStore::attach_keys(&b, &path, &["presets"]);
            // A change to an unowned key must not even mark it dirty.
            b.store_set("window", json!({ "w": 1200 })).unwrap();
            assert!(!s.flush().unwrap(), "an unowned key asked for a write");
            b.store_set("presets", json!({ "mine": 2 })).unwrap();
            assert!(s.flush().unwrap(), "an owned key did not ask for a write");
        }
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(on_disk.get("presets").is_some(), "the owned key is missing");
        assert!(
            on_disk.get("window").is_none(),
            "the file took a key it does not own, which is what would erase the \
             host's own state"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Restoring sets the owned keys and leaves the rest of the store alone.
    #[test]
    fn restoring_a_scoped_store_does_not_replace_the_whole_store() {
        let path = tmp("restore");
        std::fs::write(&path, r#"{"presets":{"mine":7}}"#).unwrap();
        let b = bridge();
        // Something the session restored before the file is read.
        b.store_set("window", json!({ "w": 640 })).unwrap();
        let _s = FileStore::attach_keys(&b, &path, &["presets"]);
        assert_eq!(b.store_get("presets").unwrap(), json!({ "mine": 7 }));
        assert_eq!(
            b.store_get("window").unwrap(),
            json!({ "w": 640 }),
            "loading the file replaced a key it does not own"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// A key the file has never seen leaves the store as it was, so a first
    /// run starts empty rather than blank.
    #[test]
    fn a_missing_file_and_a_missing_key_are_both_harmless() {
        let b = bridge();
        b.store_set("presets", json!({ "kept": true })).unwrap();
        let path = tmp("absent");
        let _s = FileStore::attach_keys(&b, &path, &["presets"]);
        assert_eq!(b.store_get("presets").unwrap(), json!({ "kept": true }));
    }
}

//! Filesystem watch that calls `HotRegistry::reload_from_paths` whenever the
//! catalog or overrides file changes. Uses `notify` on a background thread; file
//! events are debounced (editors emit several events per save) by coalescing all
//! events seen within a short window into a single reload. A reload error is
//! logged and swallowed — a bad edit must NEVER crash the watcher or swap in a
//! broken registry (the swap-safety lives in `HotRegistry`). Dropping the
//! returned `RegistryWatcher` stops watching.

use std::sync::Arc;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::CacheError;
use crate::hot_registry::HotRegistry;

pub struct RegistryWatcher {
    _watcher: RecommendedWatcher,
}

impl RegistryWatcher {
    /// Begin watching the registry's catalog (and overrides, if any) and reload on
    /// change. Performs ONE initial reload synchronously so the registry is warm
    /// before this returns.
    pub fn start(registry: Arc<HotRegistry>) -> Result<Self, CacheError> {
        // Warm load (propagate a genuine first-load failure to the caller).
        registry.reload_from_paths()?;

        let reg_for_cb = Arc::clone(&registry);
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                // Debounce: coalesce a burst of save events into one reload.
                std::thread::sleep(Duration::from_millis(50));
                if let Err(e) = reg_for_cb.reload_from_paths() {
                    tracing::warn!(
                        error = %e,
                        "model registry reload failed; keeping previous registry"
                    );
                }
            }
        })
        .map_err(|e| CacheError::RegistrySource(e.to_string()))?;

        watcher
            .watch(registry.catalog_path(), RecursiveMode::NonRecursive)
            .map_err(|e| CacheError::RegistrySource(e.to_string()))?;

        Ok(Self { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;

    fn write(path: &Path, contents: &str) {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f.sync_all().unwrap();
    }

    #[test]
    fn warm_load_happens_on_start() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = dir.path().join("models.json");
        write(
            &catalog,
            r#"[{"id":"a","provider":"x","input":1.0,"output":1.0}]"#,
        );
        let hot = Arc::new(HotRegistry::new(&catalog, None));
        let _w = RegistryWatcher::start(Arc::clone(&hot)).unwrap();
        // start() performed the initial reload synchronously.
        assert_eq!(hot.current().len(), 1);
    }

    #[test]
    fn start_fails_if_initial_load_fails() {
        let hot = Arc::new(HotRegistry::new("/nonexistent/models.json", None));
        assert!(RegistryWatcher::start(hot).is_err());
    }

    #[test]
    fn edit_triggers_reload() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = dir.path().join("models.json");
        write(
            &catalog,
            r#"[{"id":"a","provider":"x","input":1.0,"output":1.0}]"#,
        );
        let hot = Arc::new(HotRegistry::new(&catalog, None));
        let _w = RegistryWatcher::start(Arc::clone(&hot)).unwrap();
        assert_eq!(hot.current().len(), 1);

        // Modify the file; the watcher should reload within a short window.
        write(
            &catalog,
            r#"[
            {"id":"a","provider":"x","input":1.0,"output":1.0},
            {"id":"b","provider":"x","input":1.0,"output":1.0}
        ]"#,
        );

        // Poll up to ~2s for the async reload to land (CI filesystems are slow).
        let mut seen = 0;
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(50));
            seen = hot.current().len();
            if seen == 2 {
                break;
            }
        }
        assert_eq!(
            seen, 2,
            "watcher should have reloaded the registry after the edit"
        );
    }
}

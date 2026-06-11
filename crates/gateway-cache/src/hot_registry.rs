//! A registry handle whose contents can be reloaded WITHOUT blocking readers.
//! Backed by `ArcSwap<ModelRegistry>`: `current()` hands out an `Arc` snapshot a
//! reader keeps for the duration of one request; `reload_from_paths` parses the
//! files into a FRESH registry and publishes it with a single atomic pointer
//! store. A reader holding an old snapshot finishes against it; the next reader
//! sees the new one. A failed reload (bad JSON, missing catalog) leaves the
//! current registry UNCHANGED and returns the error — we never swap in a broken
//! registry (atomic-swap invariant).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;
use gateway_spine::ModelRegistry;

use crate::error::CacheError;
use crate::registry_source::build_registry_from_paths;

pub struct HotRegistry {
    inner: ArcSwap<ModelRegistry>,
    catalog_path: PathBuf,
    overrides_path: Option<PathBuf>,
}

impl HotRegistry {
    /// Start empty (no models). Call `reload_from_paths` to populate.
    pub fn new(catalog_path: impl Into<PathBuf>, overrides_path: Option<PathBuf>) -> Self {
        Self {
            inner: ArcSwap::from_pointee(ModelRegistry::new()),
            catalog_path: catalog_path.into(),
            overrides_path,
        }
    }

    /// Build directly from an already-parsed registry (used by tests / first boot
    /// when the registry came from somewhere other than the watched files).
    pub fn from_registry(
        registry: ModelRegistry,
        catalog_path: impl Into<PathBuf>,
        overrides_path: Option<PathBuf>,
    ) -> Self {
        Self {
            inner: ArcSwap::from_pointee(registry),
            catalog_path: catalog_path.into(),
            overrides_path,
        }
    }

    /// A snapshot for one request. Cheap (an `Arc` clone); never blocks a reload.
    pub fn current(&self) -> Arc<ModelRegistry> {
        self.inner.load_full()
    }

    /// Re-read the watched files and atomically publish a fresh registry. On parse
    /// failure the existing registry is preserved and the error returned.
    pub fn reload_from_paths(&self) -> Result<(), CacheError> {
        let fresh = build_registry_from_paths(&self.catalog_path, self.overrides_path.as_deref())?;
        self.inner.store(Arc::new(fresh));
        Ok(())
    }

    pub fn catalog_path(&self) -> &Path {
        &self.catalog_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::{TokenUsage, Usd};
    use std::io::Write;

    fn write(path: &Path, contents: &str) {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
    }

    #[test]
    fn reload_replaces_registry_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = dir.path().join("models.json");
        write(
            &catalog,
            r#"[{"id":"gpt-4o","provider":"openai","input":2.5,"output":10.0}]"#,
        );

        let hot = HotRegistry::new(&catalog, None);
        assert_eq!(hot.current().len(), 0); // empty before first load

        hot.reload_from_paths().unwrap();
        let snap = hot.current();
        assert_eq!(snap.len(), 1);
        let u = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        };
        assert_eq!(snap.cost("gpt-4o", &u), Some(Usd::from_micros(7_500)));

        // Rewrite the file with a new price and reload.
        write(
            &catalog,
            r#"[{"id":"gpt-4o","provider":"openai","input":2.0,"output":8.0}]"#,
        );
        hot.reload_from_paths().unwrap();
        assert_eq!(
            hot.current().get("gpt-4o").unwrap().price.input_per_mtok,
            2_000_000
        );
    }

    #[test]
    fn old_snapshot_survives_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = dir.path().join("models.json");
        write(
            &catalog,
            r#"[{"id":"a","provider":"x","input":1.0,"output":1.0}]"#,
        );
        let hot = HotRegistry::new(&catalog, None);
        hot.reload_from_paths().unwrap();

        let old = hot.current(); // snapshot taken BEFORE the next reload
        write(
            &catalog,
            r#"[{"id":"b","provider":"x","input":1.0,"output":1.0}]"#,
        );
        hot.reload_from_paths().unwrap();

        // old snapshot still sees "a"; new readers see "b".
        assert!(old.get("a").is_some());
        assert!(old.get("b").is_none());
        assert!(hot.current().get("b").is_some());
        assert!(hot.current().get("a").is_none());
    }

    #[test]
    fn failed_reload_preserves_current_registry() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = dir.path().join("models.json");
        write(
            &catalog,
            r#"[{"id":"good","provider":"x","input":1.0,"output":1.0}]"#,
        );
        let hot = HotRegistry::new(&catalog, None);
        hot.reload_from_paths().unwrap();
        assert!(hot.current().get("good").is_some());

        // Corrupt the file, reload must FAIL and keep the good registry.
        write(&catalog, "{ this is not valid json");
        assert!(hot.reload_from_paths().is_err());
        assert!(
            hot.current().get("good").is_some(),
            "broken reload must not swap"
        );
    }

    #[test]
    fn missing_catalog_errors_without_swapping() {
        let hot = HotRegistry::new("/nonexistent/models.json", None);
        assert!(hot.reload_from_paths().is_err());
        assert_eq!(hot.current().len(), 0);
    }
}

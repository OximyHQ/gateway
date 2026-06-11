//! File-watch hot reload. Watches the config file; on a write, re-runs the full
//! load pipeline (interpolate → validate) and hands the validated `Config` to a
//! callback. A config that fails validation is REJECTED (the callback is not
//! invoked) so a bad edit never tears down a healthy running gateway — the last
//! good config keeps serving. Backed by `notify`; debounced by re-reading on
//! each event.

use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::Duration;

use notify::{Event, RecursiveMode, Watcher};

use crate::error::ConfigError;
use crate::load::load;
use crate::model::Config;

/// Read + load a config file once (interpolating from the process environment).
pub fn load_file(path: &Path) -> Result<Config, ConfigError> {
    let raw = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
        detail: e.to_string(),
    })?;
    load(&raw, &|name| std::env::var(name).ok())
}

/// Watch `path`; call `on_reload` with each newly validated `Config`. Blocks the
/// calling thread, so callers spawn it. `on_reload` returning `false` stops the
/// watch loop (clean shutdown). Validation failures are logged-and-skipped, never
/// fatal.
pub fn watch<F>(path: PathBuf, mut on_reload: F) -> Result<(), ConfigError>
where
    F: FnMut(Config) -> bool,
{
    let (tx, rx) = channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(|e| ConfigError::Io {
        detail: e.to_string(),
    })?;
    watcher
        .watch(&path, RecursiveMode::NonRecursive)
        .map_err(|e| ConfigError::Io {
            detail: e.to_string(),
        })?;

    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(_event)) => match load_file(&path) {
                Ok(config) => {
                    if !on_reload(config) {
                        return Ok(());
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "config reload rejected; keeping last good config");
                }
            },
            Ok(Err(_)) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Periodic wake; lets callers stop a watch in tests deterministically.
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn load_file_reads_and_validates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"{{ "providers": [{{ "id": "openai", "kind": "openai" }}] }}"#
        )
        .unwrap();
        let c = load_file(&path).unwrap();
        assert_eq!(c.providers.len(), 1);
    }

    #[test]
    fn load_file_rejects_invalid_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut f = std::fs::File::create(&path).unwrap();
        // Provider missing required `kind`.
        write!(f, r#"{{ "providers": [{{ "id": "x" }}] }}"#).unwrap();
        assert!(matches!(
            load_file(&path),
            Err(ConfigError::Validation { .. })
        ));
    }

    #[test]
    fn watch_delivers_reload_then_stops_on_false() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(
                f,
                r#"{{ "providers": [{{ "id": "a", "kind": "openai" }}] }}"#
            )
            .unwrap();
        }

        let watch_path = path.clone();
        let handle = std::thread::spawn(move || {
            // Return false on the first reload to stop the loop deterministically.
            watch(watch_path, |cfg| {
                assert_eq!(cfg.providers[0].id, "b");
                false
            })
        });

        // Give the watcher a moment to register, then write a new valid config.
        std::thread::sleep(Duration::from_millis(200));
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(
                f,
                r#"{{ "providers": [{{ "id": "b", "kind": "openai" }}] }}"#
            )
            .unwrap();
            f.flush().unwrap();
        }

        handle.join().unwrap().unwrap();
    }
}

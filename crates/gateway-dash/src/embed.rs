//! Compile-time-embedded SPA assets + the rules for serving them. The folder
//! `ui/dist/` is embedded by `rust-embed`; a clean checkout has the committed
//! placeholder bundle; CI/release overwrite it with the real Svelte build. The
//! serving rules (history-fallback, content-type, cache headers) live here so
//! they are unit-testable without spinning up the full server.

use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "ui/dist/"]
struct Assets;

/// `true` iff the embedded bundle contains `index.html` (the SPA shell). The
/// binary surfaces this in health/`--version` so a release built without the
/// UI step fails loudly instead of shipping a blank dashboard.
pub fn index_present() -> bool {
    Assets::get("index.html").is_some()
}

/// Number of embedded files. Used by tests to assert the bundle embedded and
/// by the binary's diagnostics.
pub fn asset_count() -> usize {
    Assets::iter().count()
}

/// A resolved asset ready to serve: its bytes and content type.
pub struct ResolvedAsset {
    pub bytes: std::borrow::Cow<'static, [u8]>,
    pub content_type: String,
    /// `true` for the SPA shell (`index.html`) — served `no-cache` so a new
    /// deploy is picked up; fingerprinted assets are immutable.
    pub is_index: bool,
}

/// Resolve a request path to an asset using SPA rules:
/// - exact asset hit → serve it;
/// - root or any path WITHOUT a file extension → serve `index.html` (client-side
///   routing owns it);
/// - a path WITH an extension that misses → `None` (real 404 for a missing asset).
pub fn resolve(path: &str) -> Option<ResolvedAsset> {
    let trimmed = path.trim_start_matches('/');

    if let Some(file) = Assets::get(trimmed)
        && !trimmed.is_empty()
    {
        let ct = mime_guess::from_path(trimmed)
            .first_or_octet_stream()
            .to_string();
        return Some(ResolvedAsset {
            bytes: file.data,
            content_type: ct,
            is_index: trimmed == "index.html",
        });
    }

    // History fallback: only for "page" paths (no file extension). A missing
    // `.js`/`.css`/`.png` is a genuine 404, not the SPA shell.
    let looks_like_file = trimmed
        .rsplit('/')
        .next()
        .is_some_and(|seg| seg.contains('.'));
    if looks_like_file {
        return None;
    }

    let index = Assets::get("index.html")?;
    Some(ResolvedAsset {
        bytes: index.data,
        content_type: "text/html".to_string(),
        is_index: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_bundle_is_embedded() {
        assert!(index_present(), "ui/dist/index.html must be embedded");
        assert!(asset_count() >= 1);
    }

    #[test]
    fn root_resolves_to_index() {
        let a = resolve("/").expect("root serves the SPA shell");
        assert!(a.is_index);
        assert_eq!(a.content_type, "text/html");
    }

    #[test]
    fn extensionless_page_path_falls_back_to_index() {
        // Client-side routes like /keys, /usage, /logs are owned by the SPA.
        let a = resolve("/keys").expect("page path serves the shell");
        assert!(a.is_index);
    }

    #[test]
    fn missing_asset_with_extension_is_404() {
        // A fingerprinted asset that isn't in the bundle is a real miss.
        assert!(resolve("/assets/nope-12345.js").is_none());
    }

    #[test]
    fn index_html_is_marked_as_index() {
        let a = resolve("/index.html").expect("index.html serves directly");
        assert!(a.is_index);
        assert_eq!(a.content_type, "text/html");
    }
}

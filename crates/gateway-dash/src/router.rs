//! The embedded-dashboard router: serves static assets and the SPA shell. It is
//! mounted LAST by the binary, under the API router, so `/v1/*` and `/health`
//! always win; only paths the API doesn't claim reach here. This router owns NO
//! data endpoint — that is the `gateway-dash` thin-client invariant (design §3).

use axum::{
    Router,
    body::Body,
    extract::Path,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};

use crate::embed::{ResolvedAsset, resolve};

/// Build the dashboard router. Two routes: `/` (the shell) and `/{*path}` (any
/// asset or SPA page). Both go through the same `resolve` rules.
pub fn dash_router() -> Router {
    Router::new()
        .route("/", get(serve_root))
        .route("/{*path}", get(serve_path))
}

async fn serve_root() -> Response {
    serve("/")
}

async fn serve_path(Path(path): Path<String>) -> Response {
    serve(&path)
}

fn serve(path: &str) -> Response {
    match resolve(path) {
        Some(asset) => asset_response(asset),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn asset_response(asset: ResolvedAsset) -> Response {
    let cache = if asset.is_index {
        // The shell must never be cached hard, or a new deploy is invisible.
        "no-cache"
    } else {
        // Fingerprinted assets are content-addressed → immutable forever.
        "public, max-age=31536000, immutable"
    };
    let ct = HeaderValue::from_str(&asset.content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    let body = Body::from(asset.bytes.into_owned());
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, ct),
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request;
    use tower::ServiceExt; // for `oneshot`

    async fn get_path(path: &str) -> Response {
        dash_router()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn root_serves_html_shell() {
        let res = get_path("/").await;
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(ct.to_str().unwrap().starts_with("text/html"));
        let cache = res.headers().get(header::CACHE_CONTROL).unwrap();
        assert_eq!(cache, "no-cache");
        let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(html.contains("Oximy Gateway"), "must contain brand name");
        assert!(html.contains("Overview"), "must contain Overview view");
        assert!(html.contains("Models"), "must contain Models view");
        assert!(html.contains("Keys"), "must contain Keys view");
        assert!(html.contains("Playground"), "must contain Playground view");
    }

    #[tokio::test]
    async fn spa_page_path_serves_shell_not_404() {
        // /keys, /usage, /logs are client-side routes → the shell, 200.
        let res = get_path("/keys").await;
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(ct.to_str().unwrap().starts_with("text/html"));
    }

    #[tokio::test]
    async fn missing_fingerprinted_asset_is_404() {
        let res = get_path("/assets/missing-deadbeef.js").await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn router_exposes_no_json_data_route() {
        // Thin-client invariant: even an API-looking path resolves to the shell
        // (the binary mounts the real API ABOVE this router; in isolation the
        // dash router must NEVER answer with data — only the shell or 404).
        let res = get_path("/admin/keys").await;
        // No extension → SPA fallback (shell), never a JSON body from this crate.
        let ct = res.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(ct.to_str().unwrap().starts_with("text/html"));
    }
}

//! Integration: the dashboard router, merged under a stand-in API router exactly
//! as `oximy-gateway up` does, serves the SPA shell on page paths and yields to
//! API routes mounted above it (mount-order invariant).

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use tower::ServiceExt;

fn app() -> Router {
    // Stand-in for the P1.4 api_router: one authenticated-ish admin route.
    let api = Router::new().route("/admin/ping", get(|| async { "pong" }));
    // Same merge order the binary uses: API first, dashboard last.
    api.merge(gateway_dash::dash_router())
}

#[tokio::test]
async fn dashboard_shell_is_served_at_root() {
    let res = app()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("Oximy Gateway"));
}

#[tokio::test]
async fn spa_page_path_serves_shell() {
    let res = app()
        .oneshot(
            Request::builder()
                .uri("/usage")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn api_route_wins_over_dashboard_fallback() {
    let res = app()
        .oneshot(
            Request::builder()
                .uri("/admin/ping")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    assert_eq!(
        &body[..],
        b"pong",
        "the API route must win, not the SPA shell"
    );
}

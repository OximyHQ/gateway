//! Admin REST API — all 8 authenticated admin/usage endpoints (P1.8).
//!
//! Every handler uses the same bearer-auth as `/v1/*`: any non-revoked,
//! non-expired key may call admin routes. Mount via `admin_router()`.
//!
//! Endpoints:
//!   GET  /v1/admin/overview
//!   GET  /v1/admin/keys
//!   POST /v1/admin/keys
//!   POST /v1/admin/keys/{id}/revoke
//!   GET  /v1/usage
//!   GET  /v1/admin/logs
//!   GET  /v1/admin/providers
//!   GET  /v1/admin/mcp

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use gateway_spine::{Clock, RateLimits, Usd, VirtualKey};
use gateway_telemetry::store::{GroupBy, TimeRange};
use serde::{Deserialize, Serialize};

use crate::auth::authenticate;
use crate::state::AppState;

// ── wire types ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct TopModel {
    model: String,
    cost_usd: f64,
    requests: u64,
}

#[derive(Debug, Serialize)]
pub struct OverviewResponse {
    total_cost_usd: f64,
    requests_total: u64,
    active_keys: u64,
    models_available: u64,
    providers_configured: u64,
    mcp_servers: u64,
    top_models: Vec<TopModel>,
}

#[derive(Debug, Serialize)]
pub struct KeySummary {
    id: String,
    name: String,
    prefix: String,
    budget_usd: Option<f64>,
    spent_usd: f64,
    models: Option<Vec<String>>,
    rpm: Option<i64>,
    tpm: Option<i64>,
    revoked: bool,
}

#[derive(Debug, Serialize)]
pub struct KeysResponse {
    keys: Vec<KeySummary>,
}

#[derive(Debug, Deserialize)]
pub struct CreateKeyRequest {
    name: String,
    budget_usd: Option<f64>,
    models: Option<Vec<String>>,
    rpm: Option<i64>,
    tpm: Option<i64>,
    /// Namespaced MCP tool allowlist (`server__tool`). `None`/absent = all tools
    /// allowed. Present = the key may call ONLY these tools.
    tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct CreateKeyResponse {
    id: String,
    prefix: String,
    /// Shown exactly once; never stored after this response.
    secret: String,
    budget_usd: Option<f64>,
    models: Option<Vec<String>>,
    rpm: Option<i64>,
    tpm: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_allowlist: Option<Vec<String>>,
    revoked: bool,
}

#[derive(Debug, Serialize)]
pub struct RevokeResponse {
    id: String,
    revoked: bool,
}

#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    group_by: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UsageBucket {
    label: String,
    cost_usd: f64,
    requests: u64,
    tokens: u64,
}

#[derive(Debug, Serialize)]
pub struct UsageResponse {
    group_by: String,
    buckets: Vec<UsageBucket>,
    /// Durable spend for the authenticated key (from SQLite store). Survives restarts.
    spent_usd: f64,
    /// Total cost across all telemetry buckets (in-memory, resets on restart).
    total_usd: f64,
}

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct LogRow {
    ts_ms: i64,
    key_id: String,
    model: String,
    provider: String,
    status: u16,
    cost_usd: f64,
    latency_ms: i64,
    ttft_ms: Option<i64>,
    cache_status: String,
}

#[derive(Debug, Serialize)]
pub struct LogsResponse {
    logs: Vec<LogRow>,
}

#[derive(Debug, Serialize)]
pub struct ProviderInfo {
    id: String,
    configured: bool,
    models_count: u64,
    /// The egress base URL (not secret). Lets the dashboard pre-fill the edit form.
    #[serde(skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProvidersResponse {
    providers: Vec<ProviderInfo>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProviderRequest {
    /// Provider id (the value used by `ModelEntry.provider` and route targets).
    id: String,
    /// OpenAI-compatible base URL, e.g. `https://api.groq.com/openai`.
    base_url: String,
    /// Upstream API key (sent as the bearer credential to the provider).
    api_key: String,
}

#[derive(Debug, Serialize)]
pub struct CreateProviderResponse {
    id: String,
    base_url: String,
}

#[derive(Debug, Serialize)]
pub struct McpToolInfo {
    name: String,
    description: String,
}

#[derive(Debug, Serialize)]
pub struct McpServerInfo {
    name: String,
    healthy: bool,
    tools: Vec<McpToolInfo>,
}

#[derive(Debug, Serialize)]
pub struct McpResponse {
    servers: Vec<McpServerInfo>,
}

// ── router builder ────────────────────────────────────────────────────────────

/// Build and return the admin sub-router.  Merge this into the main `router()`
/// to mount the admin endpoints.
pub fn admin_router<C: Clock + 'static>(state: Arc<AppState<C>>) -> Router {
    Router::new()
        .route("/v1/admin/overview", get(overview::<C>))
        .route("/v1/admin/keys", get(list_keys::<C>).post(create_key::<C>))
        .route("/v1/admin/keys/{id}/revoke", post(revoke_key::<C>))
        .route("/v1/usage", get(usage::<C>))
        .route("/v1/admin/logs", get(logs::<C>))
        .route(
            "/v1/admin/providers",
            get(providers::<C>).post(create_provider::<C>),
        )
        .route(
            "/v1/admin/providers/{id}",
            put(update_provider::<C>).delete(delete_provider::<C>),
        )
        .route("/v1/admin/mcp", get(mcp::<C>))
        .with_state(state)
}

// ── shared auth helper ────────────────────────────────────────────────────────

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
}

// ── handlers ─────────────────────────────────────────────────────────────────

/// GET /v1/admin/overview
async fn overview<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }

    // Aggregate from spend store.
    let buckets = state
        .spend_store
        .query(GroupBy::Model, TimeRange::default(), None);
    let total_cost_usd: f64 = buckets.iter().map(|b| b.cost.as_dollars_f64()).sum();
    let requests_total: u64 = buckets.iter().map(|b| b.requests as u64).sum();

    // Top 5 models by cost.
    let mut model_buckets: Vec<_> = buckets
        .iter()
        .map(|b| TopModel {
            model: b.group.clone(),
            cost_usd: b.cost.as_dollars_f64(),
            requests: b.requests as u64,
        })
        .collect();
    model_buckets.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    model_buckets.truncate(5);

    // Active (non-revoked) keys.
    let active_keys = mutable_key_count(&state);

    // Models in registry.
    let models_available = state.registry.read().unwrap().len() as u64;

    // Provider count.
    let providers_configured = state.providers.count() as u64;

    // MCP server count.
    let mcp_servers = state.federation.server_names().await.len() as u64;

    Json(OverviewResponse {
        total_cost_usd,
        requests_total,
        active_keys,
        models_available,
        providers_configured,
        mcp_servers,
        top_models: model_buckets,
    })
    .into_response()
}

/// GET /v1/admin/keys
async fn list_keys<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }

    let mks = match state.keys.as_any_mutable() {
        Some(mks) => mks,
        None => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                Json(serde_json::json!({"error":{"message":"key store does not support listing","type":"api_error"}})),
            )
                .into_response();
        }
    };

    let keys = mks.all_keys();
    let mut summaries: Vec<KeySummary> = Vec::with_capacity(keys.len());
    for k in keys {
        // Read durable spend from SQLite store (survives restarts).
        let spent_usd = state
            .store
            .spent(&k.id)
            .await
            .unwrap_or(gateway_spine::Usd::ZERO)
            .as_dollars_f64();
        // Use the id as a human name (strip "key_" prefix for display).
        let name = k.id.trim_start_matches("key_").to_string();
        summaries.push(KeySummary {
            id: k.id.clone(),
            name,
            prefix: k.token_prefix.clone(),
            budget_usd: k.max_budget.map(|u| u.as_dollars_f64()),
            spent_usd,
            models: k.model_allowlist.clone(),
            rpm: k.limits.rpm,
            tpm: k.limits.tpm,
            revoked: k.revoked,
        });
    }

    Json(KeysResponse { keys: summaries }).into_response()
}

/// POST /v1/admin/keys
async fn create_key<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
    Json(body): Json<CreateKeyRequest>,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }

    let mks = match state.keys.as_any_mutable() {
        Some(mks) => mks,
        None => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                Json(serde_json::json!({"error":{"message":"key store does not support creation","type":"api_error"}})),
            )
                .into_response();
        }
    };

    // Generate a new `ogw_` prefixed secret.
    let secret = crate::firstboot_shim::generate_secret();
    let ts = state.clock.now_ms();
    let name_slug = body.name.replace(' ', "_");
    let key_id = format!("key_{name_slug}_{ts}");
    let token_prefix: String = secret.chars().take(12).collect();

    let max_budget = body.budget_usd.map(Usd::from_dollars_f64);
    let limits = RateLimits {
        rpm: body.rpm,
        tpm: body.tpm,
        max_parallel: None,
    };

    let key = VirtualKey {
        id: key_id.clone(),
        token_hash: VirtualKey::hash_secret(&secret),
        token_prefix: token_prefix.clone(),
        max_budget,
        limits,
        model_allowlist: body.models.clone(),
        tool_allowlist: body.tool_allowlist.clone(),
        expires_at: None,
        revoked: false,
        parent_id: None,
    };

    // Persist to mutable store (this calls the hook to write state file).
    mks.insert(key.clone());

    // Register budget in the ledger so requests work immediately.
    state.ledger.set_budget(&key_id, max_budget, Usd::ZERO);

    // Seed the per-key MCP tool allowlist (in-memory federation policy). Absent =
    // the key stays open to all federated tools.
    if let Some(allow) = &body.tool_allowlist {
        let set: std::collections::HashSet<String> = allow.iter().cloned().collect();
        state.federation.acl_mut().await.set(&key_id, Some(set));
    }

    // Persist to durable store (async, best-effort — log on failure).
    let stored_key = gateway_store::StoredKey {
        id: key_id.clone(),
        name: body.name.clone(),
        token_hash: key.token_hash.clone(),
        token_prefix: token_prefix.clone(),
        budget_micros: max_budget.map(|u| u.micros()),
        spent_micros: 0,
        rpm: body.rpm,
        tpm: body.tpm,
        max_parallel: None,
        model_allowlist: body.models.clone(),
        expires_at_ms: None,
        revoked: false,
        parent_id: None,
        created_at_ms: ts,
    };
    if let Err(e) = state.store.upsert_key(&stored_key).await {
        tracing::warn!(err = %e, key_id = %key_id, "failed to persist new key to store");
    }

    // Rate-limiter state is keyed by the VirtualKey.limits field at acquire-time;
    // no pre-configuration step is required — the limiter reads limits from the
    // key on each request.

    (
        StatusCode::CREATED,
        Json(CreateKeyResponse {
            id: key_id,
            prefix: token_prefix,
            secret,
            budget_usd: body.budget_usd,
            models: body.models,
            rpm: body.rpm,
            tpm: body.tpm,
            tool_allowlist: body.tool_allowlist,
            revoked: false,
        }),
    )
        .into_response()
}

/// POST /v1/admin/keys/{id}/revoke
async fn revoke_key<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }

    let mks = match state.keys.as_any_mutable() {
        Some(mks) => mks,
        None => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                Json(serde_json::json!({"error":{"message":"key store does not support revocation","type":"api_error"}})),
            )
                .into_response();
        }
    };

    if mks.revoke(&id) {
        // Also persist revocation to the durable store.
        if let Err(e) = state.store.revoke_key(&id).await {
            tracing::warn!(err = %e, key_id = %id, "failed to persist key revocation to store");
        }
        Json(RevokeResponse { id, revoked: true }).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error":{"message":"key not found","type":"invalid_request_error"}})),
        )
            .into_response()
    }
}

/// GET /v1/usage?group_by=model|key|day
async fn usage<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
    Query(params): Query<UsageQuery>,
) -> Response {
    let key = match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(k) => k,
        Err(e) => return e.into_response(),
    };

    let group_by_str = params.group_by.as_deref().unwrap_or("model");
    let group_by = match group_by_str {
        "key" => GroupBy::Key,
        "day" => GroupBy::Tag, // day grouping deferred; use Tag slot for now
        _ => GroupBy::Model,
    };

    let buckets = state
        .spend_store
        .query(group_by, TimeRange::default(), None);

    let total_usd: f64 = buckets.iter().map(|b| b.cost.as_dollars_f64()).sum();

    let wire: Vec<UsageBucket> = buckets
        .into_iter()
        .map(|b| UsageBucket {
            label: b.group,
            cost_usd: b.cost.as_dollars_f64(),
            requests: b.requests as u64,
            tokens: (b.input_tokens + b.output_tokens) as u64,
        })
        .collect();

    // Durable spend from SQLite store for the authenticated key (survives restarts).
    let spent_usd = state
        .store
        .spent(&key.id)
        .await
        .unwrap_or(gateway_spine::Usd::ZERO)
        .as_dollars_f64();

    Json(UsageResponse {
        group_by: group_by_str.to_string(),
        buckets: wire,
        spent_usd,
        total_usd,
    })
    .into_response()
}

/// GET /v1/admin/logs?limit=N (default 100, max 1000)
async fn logs<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
    Query(params): Query<LogsQuery>,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }

    let limit = params.limit.unwrap_or(100).min(1000);
    let rows = state.spend_store.recent(TimeRange::default(), limit);

    let wire: Vec<LogRow> = rows
        .into_iter()
        .map(|r| LogRow {
            ts_ms: r.ts_ms,
            key_id: r.key_id,
            model: r.model,
            provider: r.provider,
            status: r.status,
            cost_usd: r.cost.as_dollars_f64(),
            latency_ms: r.latency_ms,
            ttft_ms: r.ttft_ms,
            cache_status: format!("{:?}", r.cache_status).to_lowercase(),
        })
        .collect();

    Json(LogsResponse { logs: wire }).into_response()
}

/// GET /v1/admin/providers
async fn providers<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }

    let reg = state.registry.read().unwrap();
    let all_providers: Vec<ProviderInfo> = state
        .providers
        .all_ids()
        .into_iter()
        .map(|id| {
            let models_count = reg
                .ids()
                .into_iter()
                .filter(|mid| reg.get(mid).is_some_and(|e| e.provider == id))
                .count() as u64;
            let base_url = state
                .providers
                .get(&id)
                .and_then(|d| d.credentials.base_url.clone());
            ProviderInfo {
                id,
                configured: true,
                models_count,
                base_url,
            }
        })
        .collect();

    Json(ProvidersResponse {
        providers: all_providers,
    })
    .into_response()
}

/// POST /v1/admin/providers
///
/// Add (or replace) an OpenAI-compatible egress provider at runtime. The new
/// provider is live for the very next request that routes to it; if the binary
/// installed a persistence hook (`AppState.provider_persist`), it also survives
/// a restart. Mirrors `create_key`: same bearer auth, returns 201 on success.
async fn create_provider<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
    Json(body): Json<CreateProviderRequest>,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }

    let id = body.id.trim().to_string();
    let base_url = body.base_url.trim().to_string();
    if id.is_empty() || base_url.is_empty() || body.api_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":{"message":"id, base_url and api_key are required","type":"invalid_request_error"}})),
        )
            .into_response();
    }

    // Validate the id against the model catalog. Routing resolves a model to a
    // provider by the catalog's provider id, so a provider whose id matches no
    // model (a typo like "Tets", or an arbitrary name) would register as "active"
    // yet never serve a request. Reject it with guidance instead of creating a
    // dead provider.
    let matched = {
        let reg = state.registry.read().unwrap();
        reg.ids()
            .into_iter()
            .filter(|mid| reg.get(mid).is_some_and(|e| e.provider == id))
            .count()
    };
    if matched == 0 {
        let mut sample: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        {
            let reg = state.registry.read().unwrap();
            for mid in reg.ids() {
                if let Some(e) = reg.get(&mid) {
                    sample.insert(e.provider.clone());
                }
                if sample.len() >= 6 {
                    break;
                }
            }
        }
        let examples = sample.into_iter().collect::<Vec<_>>().join(", ");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":{
                "message": format!("provider id '{id}' matches 0 models in the catalog, so it could never be called. Use a catalog provider id (e.g. {examples})."),
                "type":"invalid_request_error"
            }})),
        )
            .into_response();
    }

    // Build an OpenAI-compatible deployment (transport + base_url + api_key),
    // exactly as the env-var presets do at boot.
    use gateway_llm::Credentials;
    use gateway_llm::transports::openai::OpenAi;
    let deployment = crate::providers::Deployment {
        provider: Arc::new(OpenAi::new()),
        credentials: Arc::new(
            Credentials::new(body.api_key.clone()).with_base_url(base_url.clone()),
        ),
    };

    // Register at runtime (interior mutability — no &mut needed).
    state.providers.insert(id.clone(), deployment);

    // Persist so it survives a restart, if the binary wired a hook. Best-effort:
    // the provider is already live in memory, so a persist failure is logged but
    // does not fail the request.
    if let Some(persist) = &state.provider_persist {
        let rp = crate::providers::RuntimeProvider {
            id: id.clone(),
            base_url: base_url.clone(),
            api_key: body.api_key.clone(),
        };
        if let Err(e) = persist.persist(&rp) {
            tracing::warn!(err = %e, provider_id = %id, "failed to persist runtime provider");
        }
    }

    tracing::info!(provider_id = %id, base_url = %base_url, "provider registered at runtime");

    (
        StatusCode::CREATED,
        Json(CreateProviderResponse { id, base_url }),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
pub struct UpdateProviderRequest {
    base_url: String,
    api_key: String,
}

/// PUT /v1/admin/providers/{id} — update an existing provider's base_url + api_key.
async fn update_provider<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateProviderRequest>,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }
    let base_url = body.base_url.trim().to_string();
    if base_url.is_empty() || body.api_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":{"message":"base_url and api_key are required","type":"invalid_request_error"}})),
        )
            .into_response();
    }
    // Update is for an existing provider only (the id keeps its catalog meaning).
    if state.providers.get(&id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error":{"message":format!("provider '{id}' not found"),"type":"invalid_request_error"}})),
        )
            .into_response();
    }

    use gateway_llm::Credentials;
    use gateway_llm::transports::openai::OpenAi;
    let deployment = crate::providers::Deployment {
        provider: Arc::new(OpenAi::new()),
        credentials: Arc::new(
            Credentials::new(body.api_key.clone()).with_base_url(base_url.clone()),
        ),
    };
    state.providers.insert(id.clone(), deployment);

    if let Some(persist) = &state.provider_persist {
        let rp = crate::providers::RuntimeProvider {
            id: id.clone(),
            base_url: base_url.clone(),
            api_key: body.api_key.clone(),
        };
        if let Err(e) = persist.persist(&rp) {
            tracing::warn!(err = %e, provider_id = %id, "failed to persist updated provider");
        }
    }

    tracing::info!(provider_id = %id, base_url = %base_url, "provider updated at runtime");
    (
        StatusCode::OK,
        Json(CreateProviderResponse { id, base_url }),
    )
        .into_response()
}

/// DELETE /v1/admin/providers/{id} — remove a provider from the live registry and
/// from durable storage. (An env-derived provider is removed for this run but
/// re-registers from its env var on the next boot.)
async fn delete_provider<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }
    if !state.providers.remove(&id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error":{"message":format!("provider '{id}' not found"),"type":"invalid_request_error"}})),
        )
            .into_response();
    }
    if let Some(persist) = &state.provider_persist
        && let Err(e) = persist.remove(&id)
    {
        tracing::warn!(err = %e, provider_id = %id, "failed to remove persisted provider");
    }
    tracing::info!(provider_id = %id, "provider removed at runtime");
    (
        StatusCode::OK,
        Json(serde_json::json!({"id": id, "removed": true})),
    )
        .into_response()
}

/// GET /v1/admin/mcp
async fn mcp<C: Clock + 'static>(
    State(state): State<Arc<AppState<C>>>,
    headers: HeaderMap,
) -> Response {
    match authenticate(state.keys.as_ref(), state.clock.as_ref(), bearer(&headers)) {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }

    let server_names = state.federation.server_names().await;
    let tools_list = state.federation.list_tools(None).await;

    // Group tools by server prefix (they are namespaced as `server__tool`).
    let mut servers: Vec<McpServerInfo> = server_names
        .iter()
        .map(|name| {
            let server_tools: Vec<McpToolInfo> = tools_list
                .tools
                .iter()
                .filter(|t| t.name.starts_with(&format!("{name}__")))
                .map(|t| McpToolInfo {
                    name: t.name.clone(),
                    description: t.description.clone().unwrap_or_default(),
                })
                .collect();
            McpServerInfo {
                name: name.clone(),
                healthy: true,
                tools: server_tools,
            }
        })
        .collect();
    servers.sort_by(|a, b| a.name.cmp(&b.name));

    Json(McpResponse { servers }).into_response()
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn mutable_key_count<C: Clock>(state: &AppState<C>) -> u64 {
    match state.keys.as_any_mutable() {
        Some(mks) => mks.active_count(),
        None => 0,
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::empty_chain;
    use crate::keystore::MutableKeyStore;
    use crate::providers::{Deployment, ProviderRegistry};
    use async_trait::async_trait;
    use axum::body::{Body, to_bytes};
    use gateway_llm::{
        ChatRequest, ChatResponse, ContentPart, Credentials, DeltaStream, FinishReason, Provider,
        ProviderCapabilities, ProviderError,
    };
    use gateway_spine::{
        MemoryAudit, MockClock, ModelEntry, ModelPrice, RateLimits, TokenUsage, Usd, VirtualKey,
    };
    use gateway_telemetry::{
        DEFAULT_CHANNEL_CAPACITY, GatewayMetrics, MemorySpendStore, spawn as spawn_telem,
    };
    use http::Request;
    use std::sync::Arc;
    use tower::ServiceExt;

    // ── stub provider ──────────────────────────────────────────────────────

    struct Echo;
    #[async_trait]
    impl Provider for Echo {
        fn id(&self) -> &str {
            "echo"
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_streaming: false,
                supports_tools: false,
                supports_vision: false,
                supports_idempotency: true,
            }
        }
        async fn chat(
            &self,
            req: &ChatRequest,
            _: &Credentials,
            _: &str,
        ) -> Result<ChatResponse, ProviderError> {
            Ok(ChatResponse {
                model: req.model.clone(),
                content: vec![ContentPart::text("hi")],
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                usage: TokenUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                    ..Default::default()
                },
                provider_response_id: None,
            })
        }
        async fn stream(
            &self,
            _: &ChatRequest,
            _: &Credentials,
            _: &str,
        ) -> Result<DeltaStream, ProviderError> {
            unreachable!()
        }
    }

    fn gpt4o() -> ModelEntry {
        ModelEntry {
            id: "gpt-4o".into(),
            provider: "openai".into(),
            price: ModelPrice {
                input_per_mtok: 2_500_000,
                output_per_mtok: 10_000_000,
                cache_read_per_mtok: 0,
                cache_write_per_mtok: 0,
            },
            context_window: Some(128_000),
            max_output_tokens: Some(16_384),
            supports_tools: true,
            supports_vision: true,
            supports_streaming: true,
        }
    }

    /// Build a test state with a `MutableKeyStore` and a shared spend store.
    async fn test_state() -> (Arc<AppState<MockClock>>, Arc<MemorySpendStore>) {
        let mks = Arc::new(MutableKeyStore::new());
        mks.insert(VirtualKey {
            id: "key_1".into(),
            token_hash: VirtualKey::hash_secret("sk-good"),
            token_prefix: "sk-good".into(),
            max_budget: Some(Usd::from_dollars_f64(10.0)),
            limits: RateLimits::default(),
            model_allowlist: None,
            tool_allowlist: None,
            expires_at: None,
            revoked: false,
            parent_id: None,
        });

        let providers = ProviderRegistry::new();
        providers.insert(
            "openai",
            Deployment {
                provider: Arc::new(Echo),
                credentials: Arc::new(Credentials::new("up")),
            },
        );

        let metrics = Arc::new(GatewayMetrics::new());
        let spend_store = Arc::new(MemorySpendStore::new());
        let (sink, _writer) = spawn_telem(
            Arc::clone(&spend_store),
            Arc::clone(&metrics),
            DEFAULT_CHANNEL_CAPACITY,
        );

        // In-memory SQLite store for durable budget/key persistence in tests.
        let durable_store = Arc::new(
            gateway_store::Store::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        // Seed the key_1 into the durable store with budget.
        durable_store
            .upsert_key(&gateway_store::StoredKey {
                id: "key_1".to_string(),
                name: "key_1".to_string(),
                token_hash: VirtualKey::hash_secret("sk-good"),
                token_prefix: "sk-good".to_string(),
                budget_micros: Some(Usd::from_dollars_f64(10.0).micros()),
                spent_micros: 0,
                rpm: None,
                tpm: None,
                max_parallel: None,
                model_allowlist: None,
                expires_at_ms: None,
                revoked: false,
                parent_id: None,
                created_at_ms: 0,
            })
            .await
            .unwrap();

        let state = Arc::new(AppState::with_parts_and_telemetry(
            mks,
            Arc::new(MockClock::new(0)),
            providers,
            Arc::new(empty_chain()),
            Arc::new(MemoryAudit::new()),
            sink,
            metrics,
            Arc::clone(&spend_store) as Arc<dyn gateway_telemetry::SpendStore>,
            durable_store,
        ));
        state.registry.write().unwrap().insert(gpt4o());
        state
            .ledger
            .set_budget("key_1", Some(Usd::from_dollars_f64(10.0)), Usd::ZERO);

        (state, spend_store)
    }

    // ── overview ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn overview_unauthenticated_is_401() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/admin/overview")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn overview_returns_expected_shape() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/admin/overview")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v.get("total_cost_usd").is_some());
        assert!(v.get("requests_total").is_some());
        assert!(v.get("active_keys").is_some());
        assert_eq!(v["models_available"], 1);
        assert_eq!(v["providers_configured"], 1);
        assert!(v["top_models"].is_array());
    }

    // ── keys list ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn keys_list_returns_seeded_key() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/admin/keys")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v["keys"].is_array());
        assert!(!v["keys"].as_array().unwrap().is_empty());
        // The seeded key should appear.
        let k = &v["keys"][0];
        assert_eq!(k["id"], "key_1");
        assert_eq!(k["revoked"], false);
    }

    // ── key create ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn create_key_returns_201_with_secret() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/admin/keys")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"mykey","budget_usd":5.0}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v["secret"].as_str().unwrap().starts_with("ogw_"));
        assert!(v["id"].as_str().unwrap().contains("mykey"));
        assert_eq!(v["budget_usd"], 5.0);
        assert_eq!(v["revoked"], false);
    }

    #[tokio::test]
    async fn create_key_unauthenticated_is_401() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/admin/keys")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"x"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── key revoke ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn revoke_existing_key_returns_revoked_true() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/admin/keys/key_1/revoke")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["id"], "key_1");
        assert_eq!(v["revoked"], true);
    }

    #[tokio::test]
    async fn revoke_missing_key_is_404() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/admin/keys/nonexistent/revoke")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── usage ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn usage_empty_returns_correct_shape() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/usage?group_by=model")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["group_by"], "model");
        assert!(v["buckets"].is_array());
    }

    // ── logs ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn logs_returns_correct_shape() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/admin/logs?limit=10")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v["logs"].is_array());
    }

    // ── providers ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn providers_returns_openai() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/admin/providers")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v["providers"].is_array());
        let providers = v["providers"].as_array().unwrap();
        assert!(!providers.is_empty());
        assert_eq!(providers[0]["id"], "openai");
        assert_eq!(providers[0]["configured"], true);
    }

    #[tokio::test]
    async fn create_provider_returns_201_and_registers_at_runtime() {
        let (state, _) = test_state().await;
        // Provider not present before the POST.
        assert!(state.providers.get("groq").is_none());
        // A catalog model under provider "groq" must exist for the id to validate.
        {
            let mut g = gpt4o();
            g.id = "groq/llama-3.1-8b".into();
            g.provider = "groq".into();
            state.registry.write().unwrap().insert(g);
        }

        let app = admin_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/admin/providers")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"id":"groq","base_url":"https://api.groq.com/openai","api_key":"sk-up"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["id"], "groq");
        assert_eq!(v["base_url"], "https://api.groq.com/openai");

        // The provider is now live in the registry (hot path can resolve it).
        assert!(state.providers.get("groq").is_some());
    }

    #[tokio::test]
    async fn create_provider_unknown_catalog_id_is_400() {
        // An id that matches no catalog model is rejected (would be a dead provider).
        let (state, _) = test_state().await;
        let app = admin_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/admin/providers")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"id":"Tets","base_url":"http://x","api_key":"k"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(state.providers.get("Tets").is_none());
    }

    #[tokio::test]
    async fn delete_provider_removes_then_404() {
        let (state, _) = test_state().await;
        state.providers.insert(
            "foo",
            crate::providers::Deployment {
                provider: Arc::new(gateway_llm::transports::openai::OpenAi::new()),
                credentials: Arc::new(gateway_llm::Credentials::new("k")),
            },
        );
        let app = admin_router(Arc::clone(&state));
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/v1/admin/providers/foo")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(state.providers.get("foo").is_none());

        let resp2 = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/v1/admin/providers/foo")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_provider_updates_then_404() {
        let (state, _) = test_state().await;
        state.providers.insert(
            "foo",
            crate::providers::Deployment {
                provider: Arc::new(gateway_llm::transports::openai::OpenAi::new()),
                credentials: Arc::new(gateway_llm::Credentials::new("old")),
            },
        );
        let app = admin_router(Arc::clone(&state));
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/admin/providers/foo")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"base_url":"https://new","api_key":"new"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(state.providers.get("foo").is_some());

        let resp2 = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/admin/providers/missing")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"base_url":"x","api_key":"y"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_provider_unauthenticated_is_401() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/admin/providers")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"id":"groq","base_url":"https://x","api_key":"k"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn create_provider_missing_fields_is_400() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/admin/providers")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"id":"","base_url":"","api_key":""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── mcp ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn mcp_empty_federation_returns_empty_list() {
        let (state, _) = test_state().await;
        let app = admin_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/admin/mcp")
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["servers"].as_array().unwrap().len(), 0);
    }

    // ── create + revoke live integration ──────────────────────────────────

    #[tokio::test]
    async fn create_key_then_use_it_then_revoke() {
        let (state, _) = test_state().await;
        // The main router already includes admin routes via `router()`.
        let app: axum::Router = crate::server::router(Arc::clone(&state));

        // Create a key.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/admin/keys")
                    .header("authorization", "Bearer sk-good")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"test_key"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let new_secret = created["secret"].as_str().unwrap().to_string();
        let new_id = created["id"].as_str().unwrap().to_string();
        assert!(new_secret.starts_with("ogw_"));

        // The new key should work for auth (models endpoint).
        let resp2 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/models")
                    .header("authorization", format!("Bearer {new_secret}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK, "new key must authenticate");

        // Revoke the key.
        let resp3 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/admin/keys/{new_id}/revoke"))
                    .header("authorization", "Bearer sk-good")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp3.status(), StatusCode::OK);

        // The revoked key must now be rejected.
        let resp4 = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/models")
                    .header("authorization", format!("Bearer {new_secret}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp4.status(),
            StatusCode::UNAUTHORIZED,
            "revoked key must be rejected"
        );
    }
}

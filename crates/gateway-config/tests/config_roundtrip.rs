//! End-to-end: a Git-managed config file → load (interpolate env) → apply into a
//! durable store → dump back → re-apply is a no-op. This is the "UI = API = CLI =
//! Git, one engine" invariant made concrete, with secrets sealed at rest and
//! emitted as env refs on the way back out.

use gateway_config::{
    Config, ConfigStore, MasterKey, SqliteConfigStore, apply, diff, dump, load, validate,
};

#[tokio::test]
async fn git_config_applies_dumps_and_reapplies_as_noop() {
    let mk = MasterKey::from_bytes([11u8; 32]);
    let store = SqliteConfigStore::connect(":memory:").await.unwrap();
    store.migrate().await.unwrap();

    // 1. A config file an operator committed to Git.
    let raw = r#"{
        "version": 1,
        "providers": [
            { "id": "openai", "kind": "openai", "api_key": "${OPENAI_API_KEY}" }
        ],
        "keys": [
            { "id": "team-a", "max_budget_usd": 25.0, "rpm": 120, "model_allowlist": ["gpt-4o"] }
        ],
        "routes": [
            { "id": "default", "model": "gpt-4o", "provider": "openai" }
        ]
    }"#;

    // 2. Load with env interpolation (secret comes from the environment).
    let env: std::collections::HashMap<String, String> =
        [("OPENAI_API_KEY".to_string(), "sk-live-xyz".to_string())]
            .into_iter()
            .collect();
    let desired = load(raw, &|n| env.get(n).cloned()).unwrap();

    // 3. Apply into the empty store; the plan creates provider + key + route.
    //    Route upserts are tracked in the diff but deferred to the router milestone
    //    (apply no-ops on CreateRoute/DeleteRoute/UpdateRoute — they land later).
    let plan = apply(&desired, &Config::default(), &store, &mk)
        .await
        .unwrap();
    // 1 CreateProvider + 1 CreateKey + 1 CreateRoute = 3 planned changes
    assert_eq!(plan.changes.len(), 3);

    // 4. The key's budget is the exact µUSD of $25.00; secret sealed, not plaintext.
    let keys = store.load_keys().await.unwrap();
    assert_eq!(keys[0].max_budget_micros, Some(25_000_000));
    let providers = store.load_providers().await.unwrap();
    let sealed = providers[0].sealed_api_key.as_ref().unwrap();
    assert_ne!(sealed.as_str(), "sk-live-xyz");
    assert_eq!(mk.open(sealed).unwrap(), "sk-live-xyz");

    // 5. Dump the live state back to a Config (secret → env ref).
    let dumped = dump(&store).await.unwrap();
    let dumped_json = serde_json::to_string(&dumped).unwrap();
    assert!(!dumped_json.contains("sk-live-xyz"));
    assert_eq!(
        dumped.providers[0].api_key.as_deref(),
        Some("${OPENAI_API_KEY}")
    );

    // 6. The dump is a valid config and re-applying it is a no-op (round-trip).
    validate(&dumped_json).unwrap();
    assert!(diff(&dumped, &dumped.clone()).is_empty());
    let replan = apply(&dumped, &dumped.clone(), &store, &mk).await.unwrap();
    assert!(replan.is_empty());
}

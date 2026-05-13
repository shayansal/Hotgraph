use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use rg_api::{
    router, serve_with_graceful_shutdown, ApiRole, ApiState, AuthConfig, ContextPackIntentProvider,
    FixtureContextPackIntentProvider, FixtureQuestionEmbeddingProvider, QuestionEmbeddingProvider,
    ServiceAccount,
};
use rg_core::{SourceId, TenantId, TxTime};
use rg_governance::{
    GovernanceEngine, PermissionPolicy, PermissionScope, PrincipalId, RedactionEvent,
    SourceAccessPolicy,
};
use rg_storage::FileEventLog;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn api_creates_queries_paths_and_evidence_packs_against_in_memory_graph() {
    let app = router(ApiState::new_in_memory());

    let source = request_json(
        app.clone(),
        "POST",
        "/v1/sources",
        json!({
            "id": "source-employment",
            "source_type": "Document",
            "uri": "file://employment.md",
            "content_hash": "sha256:employment",
            "trust_score": 0.95
        }),
    )
    .await;
    assert_eq!(source["id"], "source-employment");

    request_json(
        app.clone(),
        "POST",
        "/v1/entities",
        json!({"id": "person-a", "type": "Person", "name": "Person A"}),
    )
    .await;
    request_json(
        app.clone(),
        "POST",
        "/v1/entities",
        json!({"id": "company-b", "type": "Organization", "name": "Company B"}),
    )
    .await;
    request_json(
        app.clone(),
        "POST",
        "/v1/entities",
        json!({"id": "city-c", "type": "Place", "name": "City C"}),
    )
    .await;

    let assertion = request_json(
        app.clone(),
        "POST",
        "/v1/assertions",
        json!({
            "id": "assertion-worked-at",
            "subject": "person-a",
            "predicate": "WORKED_AT",
            "object": {"entity_id": "company-b"},
            "valid_from": "2021-01-01",
            "valid_to": "2025-01-01",
            "confidence": 0.92,
            "sources": ["source-employment"],
            "context": "world"
        }),
    )
    .await;
    assert_eq!(assertion["predicate"], "WORKED_AT");
    assert_eq!(assertion["sources"], json!(["source-employment"]));

    request_json(
        app.clone(),
        "POST",
        "/v1/assertions",
        json!({
            "id": "assertion-located-in",
            "subject": "company-b",
            "predicate": "LOCATED_IN",
            "object": {"entity_id": "city-c"},
            "valid_from": "2020-01-01",
            "confidence": 0.88,
            "sources": ["source-employment"],
            "context": "world"
        }),
    )
    .await;

    let entity = request_empty(app.clone(), "GET", "/v1/entities/person-a").await;
    assert_eq!(entity["canonical_name"], "Person A");

    let state = request_empty(
        app.clone(),
        "GET",
        "/v1/entities/person-a/state?valid_at=2024-01-01",
    )
    .await;
    assert_eq!(
        state["assertions"][0]["assertion_id"],
        "assertion-worked-at"
    );

    let query = request_json(
        app.clone(),
        "POST",
        "/v1/query",
        json!({
            "subject": {"entity_id": "person-a"},
            "predicate": "WORKED_AT",
            "valid_at": "2024-01-01T00:00:00Z",
            "min_confidence": 0.8,
            "include_sources": true
        }),
    )
    .await;
    assert_eq!(query["results"][0]["assertion_id"], "assertion-worked-at");
    assert_eq!(query["results"][0]["sources"], json!(["source-employment"]));

    let path = request_json(
        app.clone(),
        "POST",
        "/v1/path",
        json!({
            "start": "person-a",
            "end": "city-c",
            "predicates": ["WORKED_AT", "LOCATED_IN"],
            "valid_at": "2024-01-01",
            "max_depth": 2,
            "min_confidence": 0.8
        }),
    )
    .await;
    assert_eq!(
        path["paths"][0]["hops"][0]["assertion_id"],
        "assertion-worked-at"
    );

    let pack = request_json(
        app.clone(),
        "POST",
        "/v1/evidence-pack",
        json!({
            "query": "Where did Person A work in 2024?",
            "graph_query": {
                "subject": {"entity_id": "person-a"},
                "predicate": "WORKED_AT",
                "valid_at": "2024-01-01",
                "min_confidence": 0.8
            },
            "path_query": {
                "start": "person-a",
                "end": "city-c",
                "predicates": ["WORKED_AT", "LOCATED_IN"],
                "valid_at": "2024-01-01",
                "max_depth": 2,
                "min_confidence": 0.8
            }
        }),
    )
    .await;
    assert_eq!(
        pack["assertions"][0]["assertion_id"],
        "assertion-located-in"
    );
    assert_eq!(
        pack["paths"][0]["hops"][1]["assertion_id"],
        "assertion-located-in"
    );

    let ingest = request_json(
        app.clone(),
        "POST",
        "/v1/ingest/document",
        json!({
            "id": "doc-api",
            "source_id": "source-employment",
            "uri": "file://employment.md",
            "content": "candidate: Person A | worked_at | Company B | valid=2021..2025 | confidence=0.92 | evidence=Person A worked at Company B."
        }),
    )
    .await;
    assert_eq!(ingest["candidates"][0]["subject_text"], "Person A");

    let health = request_empty(app.clone(), "GET", "/v1/health").await;
    assert_eq!(health["status"], "ok");
    assert_eq!(health["event_log"], "ok");
    assert_eq!(health["index_health"]["status"], "ok");

    let docs = request_empty(app, "GET", "/v1/openapi.json").await;
    let docs_text = serde_json::to_string(&docs).expect("docs JSON");
    assert!(docs_text.contains("/v1/entities"));
    assert!(docs_text.contains("/v1/evidence-pack"));
}

#[tokio::test]
async fn auth_middleware_enforces_api_keys_roles_and_masks_secrets() {
    let app = router(authenticated_state());

    let denied = raw_json(
        app.clone(),
        "POST",
        "/v1/sources",
        json!({
            "id": "source-secure",
            "source_type": "Document",
            "content_hash": "sha256:secure"
        }),
        &[],
    )
    .await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let denied_body = response_json(denied).await;
    assert_eq!(denied_body["code"], "unauthorized");
    assert!(!denied_body["error"]
        .as_str()
        .expect("error string")
        .contains("writer-key"));

    let forbidden = raw_json(
        app.clone(),
        "POST",
        "/v1/sources",
        json!({
            "id": "source-secure",
            "source_type": "Document",
            "content_hash": "sha256:secure"
        }),
        &[("x-api-key", "reader-key")],
    )
    .await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    let forbidden_body = response_json(forbidden).await;
    assert_eq!(forbidden_body["code"], "forbidden");

    let health = request_empty(app, "GET", "/v1/health").await;
    assert_eq!(health["status"], "ok");
}

#[tokio::test]
async fn production_env_requires_auth_unless_dev_override() {
    let _env_guard = ENV_LOCK.lock().expect("env lock");
    let previous_keys = std::env::var("RG_API_KEYS").ok();
    let previous_dev = std::env::var("HOTGRAPH_DEV_AUTH_DISABLED").ok();
    let previous_log = std::env::var("RG_EVENT_LOG_PATH").ok();
    let previous_replicas = std::env::var("RG_REPLICA_COUNT").ok();
    std::env::remove_var("RG_API_KEYS");
    std::env::remove_var("HOTGRAPH_DEV_AUTH_DISABLED");
    std::env::remove_var("RG_EVENT_LOG_PATH");
    std::env::remove_var("RG_REPLICA_COUNT");

    let error = ApiState::from_env().expect_err("production env without auth must fail");
    assert!(error.to_string().contains("auth"));

    std::env::set_var("HOTGRAPH_DEV_AUTH_DISABLED", "true");
    ApiState::from_env().expect("dev override allows local disabled auth");

    restore_env("RG_API_KEYS", previous_keys);
    restore_env("HOTGRAPH_DEV_AUTH_DISABLED", previous_dev);
    restore_env("RG_EVENT_LOG_PATH", previous_log);
    restore_env("RG_REPLICA_COUNT", previous_replicas);
}

#[tokio::test]
async fn production_env_rejects_multiple_replicas_without_durable_storage() {
    let _env_guard = ENV_LOCK.lock().expect("env lock");
    let previous_keys = std::env::var("RG_API_KEYS").ok();
    let previous_dev = std::env::var("HOTGRAPH_DEV_AUTH_DISABLED").ok();
    let previous_log = std::env::var("RG_EVENT_LOG_PATH").ok();
    let previous_replicas = std::env::var("RG_REPLICA_COUNT").ok();
    std::env::set_var(
        "RG_API_KEYS",
        "writer-secret:writer:tenant-lab:reader|writer",
    );
    std::env::remove_var("HOTGRAPH_DEV_AUTH_DISABLED");
    std::env::remove_var("RG_EVENT_LOG_PATH");
    std::env::set_var("RG_REPLICA_COUNT", "2");

    let error =
        ApiState::from_env().expect_err("multiple replicas without durable storage must fail");
    assert!(error.to_string().contains("multiple API replicas"));

    std::env::set_var("RG_EVENT_LOG_PATH", temp_file("api-env-events"));
    ApiState::from_env().expect("durable event log allows configured replica count");

    restore_env("RG_API_KEYS", previous_keys);
    restore_env("HOTGRAPH_DEV_AUTH_DISABLED", previous_dev);
    restore_env("RG_EVENT_LOG_PATH", previous_log);
    restore_env("RG_REPLICA_COUNT", previous_replicas);
}

#[tokio::test]
async fn auth_config_parses_env_accounts_and_does_not_store_raw_keys() {
    let auth = AuthConfig::from_env_value(
        "writer-secret:writer-sa:tenant-a:reader|writer,admin-secret:admin-sa:tenant-a:admin",
    )
    .expect("parse env auth");

    assert!(auth.authenticate("writer-secret").is_some());
    assert!(auth.authenticate("admin-secret").is_some());
    assert!(auth
        .debug_key_material()
        .iter()
        .all(|value| { !value.contains("writer-secret") && !value.contains("admin-secret") }));
}

#[tokio::test]
async fn durable_api_state_recovers_events_and_idempotency_after_restart() {
    let event_log_path = temp_file("api-events");
    let idempotency_path = temp_file("api-idempotency");
    {
        let app = router(
            ApiState::from_durable_event_log(&event_log_path)
                .expect("durable state")
                .with_idempotency_path(&idempotency_path)
                .expect("idempotency persistence")
                .with_auth(AuthConfig::new(vec![ServiceAccount::new(
                    "writer-key",
                    "writer",
                    "tenant-lab",
                    vec![ApiRole::Reader, ApiRole::Writer],
                )])),
        );
        request_json_with_headers(
            app,
            "POST",
            "/v1/sources",
            json!({
                "id": "source-restart",
                "source_type": "Document",
                "content_hash": "sha256:restart"
            }),
            &[
                ("x-api-key", "writer-key"),
                ("idempotency-key", "source-restart-once"),
            ],
        )
        .await;
    }

    let records = FileEventLog::open(&event_log_path)
        .expect("open wal")
        .read_records()
        .expect("read wal records");
    assert_eq!(
        records[0].idempotency_key.as_deref(),
        Some("source-restart-once")
    );

    let restarted = router(
        ApiState::from_durable_event_log(&event_log_path)
            .expect("reload durable state")
            .with_idempotency_path(&idempotency_path)
            .expect("reload idempotency")
            .with_auth(AuthConfig::new(vec![ServiceAccount::new(
                "writer-key",
                "writer",
                "tenant-lab",
                vec![ApiRole::Reader, ApiRole::Writer],
            )])),
    );
    let source = request_text_with_headers(
        restarted.clone(),
        "GET",
        "/v1/sources/source-restart",
        &[("x-api-key", "writer-key")],
    )
    .await;
    assert!(source.contains("source-restart"));

    let replayed = request_json_with_headers(
        restarted.clone(),
        "POST",
        "/v1/sources",
        json!({
            "id": "source-restart",
            "source_type": "Document",
            "content_hash": "sha256:restart"
        }),
        &[
            ("x-api-key", "writer-key"),
            ("idempotency-key", "source-restart-once"),
        ],
    )
    .await;
    assert_eq!(replayed["event_type"], "source_added");
    let metrics = request_text_with_headers(
        restarted,
        "GET",
        "/v1/metrics",
        &[("x-api-key", "writer-key")],
    )
    .await;
    assert!(metrics.contains("rg_graph_events_total 1"));

    let _ = fs::remove_file(event_log_path);
    let _ = fs::remove_file(idempotency_path);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reader_nodes_proxy_writes_and_tail_writer_replication_batches() {
    let reader_path = temp_file("api-reader-redb");
    let leader_path = temp_file("api-fake-leader-redb");
    let writer = router(
        ApiState::from_redb_graph_store(&leader_path)
            .expect("leader redb state")
            .with_auth(replication_auth()),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind writer listener");
    let writer_url = format!(
        "http://{}",
        listener.local_addr().expect("writer listener addr")
    );
    let (shutdown_writer, writer_shutdown) = tokio::sync::oneshot::channel::<()>();
    let writer_task = tokio::spawn(async move {
        axum::serve(listener, writer)
            .with_graceful_shutdown(async {
                let _ = writer_shutdown.await;
            })
            .await
            .expect("writer server");
    });

    let reader = router(
        ApiState::from_redb_graph_store(&reader_path)
            .expect("reader redb state")
            .with_auth(replication_auth())
            .with_reader_role(writer_url.clone(), Some(0))
            .with_replication_api_key("admin-key"),
    );

    let proxied_response = raw_json(
        reader.clone(),
        "POST",
        "/v1/sources",
        json!({
            "id": "source-proxied",
            "source_type": "Document",
            "content_hash": "sha256:proxied"
        }),
        &[("x-api-key", "writer-key")],
    )
    .await;
    let proxied_response = expect_ok_response(proxied_response, "proxied source write").await;
    let proxied = response_json(proxied_response).await;
    assert_eq!(proxied["id"], "source-proxied");

    let caught_up = raw_empty(
        reader.clone(),
        "POST",
        "/v1/admin/replication/catch-up",
        &[("x-api-key", "admin-key")],
    )
    .await;
    let caught_up = expect_ok_response(caught_up, "reader catch-up").await;
    let caught_up = response_json(caught_up).await;
    assert_eq!(caught_up["replay_lag"], 0);
    assert_eq!(caught_up["follower_applied_lsn"], 1);

    let local_source = request_empty_with_headers(
        reader,
        "GET",
        "/v1/sources/source-proxied",
        &[("x-api-key", "reader-key")],
    )
    .await;
    assert_eq!(local_source["id"], "source-proxied");

    let _ = shutdown_writer.send(());
    writer_task.await.expect("join writer server");

    let _ = fs::remove_file(leader_path);
    let _ = fs::remove_file(reader_path);
}

#[tokio::test]
async fn request_limits_query_defaults_and_histograms_are_enforced() {
    let body_limited_app = router(ApiState::new_in_memory().with_max_body_bytes(64).with_auth(
        AuthConfig::new(vec![ServiceAccount::new(
            "reader-key",
            "reader",
            "tenant-lab",
            vec![ApiRole::Reader],
        )]),
    ));

    let oversized = raw_json(
        body_limited_app,
        "POST",
        "/v1/query",
        json!({
            "predicate": "WORKED_AT",
            "context": "tenant:tenant-lab",
            "padding": "this request body is intentionally too large for the configured API body limit"
        }),
        &[("x-api-key", "reader-key")],
    )
    .await;
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let app = router(
        ApiState::new_in_memory()
            .with_default_query_limit(25)
            .with_max_query_limit(50)
            .with_max_path_depth(3)
            .with_auth(AuthConfig::new(vec![ServiceAccount::new(
                "reader-key",
                "reader",
                "tenant-lab",
                vec![ApiRole::Reader],
            )])),
    );

    let path_too_deep = raw_json(
        app.clone(),
        "POST",
        "/v1/path",
        json!({
            "start": "person-a",
            "predicates": [],
            "max_depth": 4
        }),
        &[("x-api-key", "reader-key")],
    )
    .await;
    assert_eq!(path_too_deep.status(), StatusCode::BAD_REQUEST);

    let query_limit = raw_json(
        app.clone(),
        "POST",
        "/v1/query",
        json!({
            "predicate": "WORKED_AT",
            "limit": 10000
        }),
        &[("x-api-key", "reader-key")],
    )
    .await;
    assert_eq!(query_limit.status(), StatusCode::BAD_REQUEST);

    let metrics =
        request_text_with_headers(app, "GET", "/v1/metrics", &[("x-api-key", "reader-key")]).await;
    assert!(metrics.contains("rg_api_request_duration_seconds_bucket"));
    assert!(metrics.contains("operation=\"query\""));
}

#[tokio::test]
async fn idempotency_keys_replay_successful_write_without_appending_events() {
    let app = router(authenticated_state());
    let headers = [
        ("x-api-key", "writer-key"),
        ("idempotency-key", "source-once"),
    ];

    let first = request_json_with_headers(
        app.clone(),
        "POST",
        "/v1/sources",
        json!({
            "id": "source-once",
            "source_type": "Document",
            "content_hash": "sha256:source-once"
        }),
        &headers,
    )
    .await;
    let second = request_json_with_headers(
        app.clone(),
        "POST",
        "/v1/sources",
        json!({
            "id": "source-once",
            "source_type": "Document",
            "content_hash": "sha256:source-once"
        }),
        &headers,
    )
    .await;

    assert_eq!(first, second);
    let metrics =
        request_text_with_headers(app, "GET", "/v1/metrics", &[("x-api-key", "reader-key")]).await;
    assert!(metrics.contains("rg_graph_events_total 1"));
    assert!(metrics.contains("rg_graph_sources_total 1"));
}

#[tokio::test]
async fn authenticated_queries_are_scoped_to_the_callers_tenant() {
    let app = router(authenticated_state());

    seed_tenant_graph(
        app.clone(),
        "writer-key",
        "tenant-a",
        "person-a",
        "company-a",
    )
    .await;
    seed_tenant_graph(
        app.clone(),
        "tenant-b-key",
        "tenant-b",
        "person-b",
        "company-b",
    )
    .await;

    let tenant_a = request_json_with_headers(
        app.clone(),
        "POST",
        "/v1/query",
        json!({
            "predicate": "WORKED_AT",
            "valid_at": "2024-01-01",
            "min_confidence": 0.8
        }),
        &[("x-api-key", "reader-key")],
    )
    .await;
    assert_eq!(tenant_a["results"].as_array().expect("results").len(), 1);
    assert_eq!(tenant_a["results"][0]["subject"], "person-a");
    assert_eq!(tenant_a["results"][0]["context"], "tenant:tenant-a");

    let tenant_b = request_json_with_headers(
        app,
        "POST",
        "/v1/query",
        json!({
            "predicate": "WORKED_AT",
            "valid_at": "2024-01-01",
            "min_confidence": 0.8
        }),
        &[("x-api-key", "tenant-b-key")],
    )
    .await;
    assert_eq!(tenant_b["results"].as_array().expect("results").len(), 1);
    assert_eq!(tenant_b["results"][0]["subject"], "person-b");
    assert_eq!(tenant_b["results"][0]["context"], "tenant:tenant-b");
}

#[test]
fn deterministic_ai_providers_are_stable_for_context_pack_tests() {
    let embedding_provider = FixtureQuestionEmbeddingProvider;
    let model_provider = FixtureContextPackIntentProvider;

    let embedding = embedding_provider.embed_question("Where did Person A work in 2024?");
    assert_eq!(embedding, vec![1.0, 0.0, 0.0, 0.0]);
    assert_eq!(
        model_provider.infer_predicate("Where did Person A work in 2024?", &embedding),
        Some("WORKED_AT".to_owned())
    );
}

#[tokio::test]
async fn ai_context_pack_infers_work_intent_and_returns_structured_evidence() {
    let app = router(ApiState::new_in_memory());

    request_json(
        app.clone(),
        "POST",
        "/v1/sources",
        json!({
            "id": "source-employment-a",
            "source_type": "Document",
            "uri": "file://employment-a.md",
            "content_hash": "sha256:employment-a",
            "trust_score": 0.95
        }),
    )
    .await;
    request_json(
        app.clone(),
        "POST",
        "/v1/sources",
        json!({
            "id": "source-employment-b",
            "source_type": "Document",
            "uri": "file://employment-b.md",
            "content_hash": "sha256:employment-b",
            "trust_score": 0.7
        }),
    )
    .await;
    request_json(
        app.clone(),
        "POST",
        "/v1/entities",
        json!({"id": "person-a", "type": "Person", "name": "Person A"}),
    )
    .await;
    request_json(
        app.clone(),
        "POST",
        "/v1/entities",
        json!({"id": "company-b", "type": "Company", "name": "Company B"}),
    )
    .await;
    request_json(
        app.clone(),
        "POST",
        "/v1/entities",
        json!({"id": "company-c", "type": "Company", "name": "Company C"}),
    )
    .await;
    request_json(
        app.clone(),
        "POST",
        "/v1/entities",
        json!({"id": "city-d", "type": "Place", "name": "City D"}),
    )
    .await;

    request_json(
        app.clone(),
        "POST",
        "/v1/assertions",
        json!({
            "id": "assertion-worked-at-b",
            "subject": "person-a",
            "predicate": "WORKED_AT",
            "object": {"entity_id": "company-b"},
            "valid_from": "2021-01-01",
            "valid_to": "2025-01-01",
            "confidence": 0.92,
            "sources": ["source-employment-a"],
            "context": "world"
        }),
    )
    .await;
    request_json(
        app.clone(),
        "POST",
        "/v1/assertions",
        json!({
            "id": "assertion-worked-at-c",
            "subject": "person-a",
            "predicate": "WORKED_AT",
            "object": {"entity_id": "company-c"},
            "valid_from": "2023-01-01",
            "valid_to": "2024-12-31",
            "confidence": 0.84,
            "sources": ["source-employment-b"],
            "context": "world"
        }),
    )
    .await;
    request_json(
        app.clone(),
        "POST",
        "/v1/assertions",
        json!({
            "id": "assertion-located-in",
            "subject": "person-a",
            "predicate": "LOCATED_IN",
            "object": {"entity_id": "city-d"},
            "valid_from": "2020-01-01",
            "confidence": 0.9,
            "sources": ["source-employment-a"],
            "context": "world"
        }),
    )
    .await;

    let pack = request_json(
        app.clone(),
        "POST",
        "/v1/ai/context-pack",
        json!({
            "question": "Where did Person A work in 2024?",
            "valid_at": "2024-01-01",
            "entity_ids": ["person-a"],
            "min_confidence": 0.8,
            "limit": 10
        }),
    )
    .await;

    assert_eq!(pack["query"], "Where did Person A work in 2024?");
    let assertion_ids = pack["assertions"]
        .as_array()
        .expect("assertions")
        .iter()
        .map(|assertion| assertion["assertion_id"].as_str().expect("assertion id"))
        .collect::<Vec<_>>();
    assert!(assertion_ids.contains(&"assertion-worked-at-b"));
    assert!(assertion_ids.contains(&"assertion-worked-at-c"));
    assert!(!assertion_ids.contains(&"assertion-located-in"));
    assert_eq!(pack["sources"].as_array().expect("sources").len(), 2);
    assert!(!pack["contradictions"]
        .as_array()
        .expect("contradictions")
        .is_empty());

    let docs = request_empty(app, "GET", "/v1/openapi.json").await;
    let docs_text = serde_json::to_string(&docs).expect("docs JSON");
    assert!(docs_text.contains("/v1/ai/context-pack"));
}

#[tokio::test]
async fn ai_context_pack_is_reader_scoped_for_authenticated_tenants() {
    let app = router(authenticated_state());

    seed_tenant_graph(
        app.clone(),
        "writer-key",
        "tenant-a",
        "person-a",
        "company-a",
    )
    .await;
    seed_tenant_graph(
        app.clone(),
        "tenant-b-key",
        "tenant-b",
        "person-b",
        "company-b",
    )
    .await;

    let pack = request_json_with_headers(
        app.clone(),
        "POST",
        "/v1/ai/context-pack",
        json!({
            "question": "Where did people work in 2024?",
            "valid_at": "2024-01-01",
            "min_confidence": 0.8
        }),
        &[("x-api-key", "reader-key")],
    )
    .await;
    assert_eq!(pack["assertions"].as_array().expect("assertions").len(), 1);
    assert_eq!(pack["assertions"][0]["subject"], "person-a");
    assert_eq!(pack["assertions"][0]["context"], "tenant:tenant-a");

    let denied = raw_json(
        app,
        "POST",
        "/v1/ai/context-pack",
        json!({
            "question": "Where did people work in 2024?",
            "valid_at": "2024-01-01"
        }),
        &[],
    )
    .await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn governance_redaction_and_source_acl_are_enforced_on_read_paths() {
    let source_id = SourceId::new("source-governed");
    let governance = GovernanceEngine::new(
        PermissionPolicy::new(TenantId::new("tenant-a"))
            .allow_read(PermissionScope::Tenant(TenantId::new("tenant-a")))
            .with_source_policy(SourceAccessPolicy::restricted(
                source_id.clone(),
                vec![PrincipalId::new("writer-service")],
            ))
            .with_redaction(RedactionEvent::source(
                source_id.clone(),
                PrincipalId::new("privacy-officer"),
                "right to be forgotten",
                TxTime::new(99),
            )),
    );
    let app = router(authenticated_state().with_governance(governance));

    request_json_with_headers(
        app.clone(),
        "POST",
        "/v1/entities",
        json!({"id": "person-governed", "type": "Person", "name": "Person Governed"}),
        &[("x-api-key", "writer-key")],
    )
    .await;
    request_json_with_headers(
        app.clone(),
        "POST",
        "/v1/entities",
        json!({"id": "company-governed", "type": "Organization", "name": "Company Governed"}),
        &[("x-api-key", "writer-key")],
    )
    .await;
    request_json_with_headers(
        app.clone(),
        "POST",
        "/v1/sources",
        json!({
            "id": "source-governed",
            "source_type": "Document",
            "content_hash": "sha256:governed"
        }),
        &[("x-api-key", "writer-key")],
    )
    .await;
    request_json_with_headers(
        app.clone(),
        "POST",
        "/v1/assertions",
        json!({
            "id": "assertion-governed",
            "subject": "person-governed",
            "predicate": "WORKED_AT",
            "object": {"entity_id": "company-governed"},
            "valid_from": "2021-01-01",
            "valid_to": "2025-01-01",
            "confidence": 0.92,
            "sources": ["source-governed"]
        }),
        &[("x-api-key", "writer-key")],
    )
    .await;

    let source_fetch = raw_empty(
        app.clone(),
        "GET",
        "/v1/sources/source-governed",
        &[("x-api-key", "reader-key")],
    )
    .await;
    assert_eq!(source_fetch.status(), StatusCode::NOT_FOUND);

    let query = request_json_with_headers(
        app.clone(),
        "POST",
        "/v1/query",
        json!({
            "predicate": "WORKED_AT",
            "valid_at": "2024-01-01",
            "min_confidence": 0.8,
            "limit": 10
        }),
        &[("x-api-key", "reader-key")],
    )
    .await;
    let assertion_ids = query["results"]
        .as_array()
        .expect("query results")
        .iter()
        .map(|result| result["assertion_id"].as_str().expect("assertion id"))
        .collect::<Vec<_>>();
    assert!(!assertion_ids.contains(&"assertion-governed"));

    let pack = request_json_with_headers(
        app,
        "POST",
        "/v1/evidence-pack",
        json!({
            "query": "Where did the governed person work?",
            "graph_query": {
                "subject": {"entity_id": "person-governed"},
                "predicate": "WORKED_AT",
                "valid_at": "2024-01-01"
            }
        }),
        &[("x-api-key", "reader-key")],
    )
    .await;
    assert_eq!(pack["assertions"].as_array().expect("assertions").len(), 0);
    assert_eq!(pack["sources"].as_array().expect("sources").len(), 0);
}

#[tokio::test]
async fn api_server_supports_graceful_shutdown() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");

    let result = serve_with_graceful_shutdown(listener, ApiState::new_in_memory(), async {}).await;

    assert!(result.is_ok());
}

async fn request_json(app: axum::Router, method: &str, uri: &str, body: Value) -> Value {
    request_json_with_headers(app, method, uri, body, &[]).await
}

async fn request_json_with_headers(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Value,
    headers: &[(&str, &str)],
) -> Value {
    let response = app
        .oneshot(json_request(method, uri, body, headers))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn request_empty(app: axum::Router, method: &str, uri: &str) -> Value {
    request_empty_with_headers(app, method, uri, &[]).await
}

async fn request_empty_with_headers(
    app: axum::Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
) -> Value {
    let response = app
        .oneshot(empty_request(method, uri, headers))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json body")
}

async fn raw_empty(
    app: axum::Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
) -> axum::response::Response {
    app.oneshot(empty_request(method, uri, headers))
        .await
        .expect("response")
}

async fn request_text_with_headers(
    app: axum::Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
) -> String {
    let response = app
        .oneshot(empty_request(method, uri, headers))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf8 body")
}

async fn raw_json(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Value,
    headers: &[(&str, &str)],
) -> axum::response::Response {
    app.oneshot(json_request(method, uri, body, headers))
        .await
        .expect("response")
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json body")
}

async fn expect_ok_response(
    response: axum::response::Response,
    operation: &str,
) -> axum::response::Response {
    if response.status() == StatusCode::OK {
        return response;
    }
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    panic!(
        "{operation} returned {status}: {}",
        String::from_utf8_lossy(&bytes)
    );
}

fn json_request(method: &str, uri: &str, body: Value, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::from(body.to_string())).expect("request")
}

fn empty_request(method: &str, uri: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::empty()).expect("request")
}

fn authenticated_state() -> ApiState {
    ApiState::new_in_memory()
        .with_auth(AuthConfig::new(vec![
            ServiceAccount::new(
                "writer-key",
                "writer-service",
                "tenant-a",
                vec![ApiRole::Reader, ApiRole::Writer],
            ),
            ServiceAccount::new(
                "reader-key",
                "reader-service",
                "tenant-a",
                vec![ApiRole::Reader],
            ),
            ServiceAccount::new(
                "tenant-b-key",
                "tenant-b-service",
                "tenant-b",
                vec![ApiRole::Reader, ApiRole::Writer],
            ),
        ]))
        .with_slow_query_threshold(Duration::from_secs(10))
}

fn replication_auth() -> AuthConfig {
    AuthConfig::new(vec![
        ServiceAccount::new(
            "writer-key",
            "writer-service",
            "tenant-a",
            vec![ApiRole::Reader, ApiRole::Writer],
        ),
        ServiceAccount::new(
            "reader-key",
            "reader-service",
            "tenant-a",
            vec![ApiRole::Reader],
        ),
        ServiceAccount::new(
            "admin-key",
            "admin-service",
            "tenant-a",
            vec![ApiRole::Admin],
        ),
    ])
}

fn temp_file(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let mut path = std::env::temp_dir();
    path.push(format!(
        "hotgraph-api-{name}-{}-{}-{nonce}.log",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = fs::remove_file(&path);
    path
}

fn restore_env(name: &str, value: Option<String>) {
    if let Some(value) = value {
        std::env::set_var(name, value);
    } else {
        std::env::remove_var(name);
    }
}

async fn seed_tenant_graph(
    app: axum::Router,
    api_key: &str,
    tenant: &str,
    person: &str,
    company: &str,
) {
    let headers = [("x-api-key", api_key)];
    request_json_with_headers(
        app.clone(),
        "POST",
        "/v1/sources",
        json!({
            "id": format!("source-{tenant}"),
            "source_type": "Document",
            "content_hash": format!("sha256:{tenant}")
        }),
        &headers,
    )
    .await;
    request_json_with_headers(
        app.clone(),
        "POST",
        "/v1/entities",
        json!({"id": person, "type": "Person", "name": person}),
        &headers,
    )
    .await;
    request_json_with_headers(
        app.clone(),
        "POST",
        "/v1/entities",
        json!({"id": company, "type": "Organization", "name": company}),
        &headers,
    )
    .await;
    request_json_with_headers(
        app,
        "POST",
        "/v1/assertions",
        json!({
            "id": format!("assertion-{tenant}"),
            "subject": person,
            "predicate": "WORKED_AT",
            "object": {"entity_id": company},
            "valid_from": "2021-01-01",
            "valid_to": "2025-01-01",
            "confidence": 0.92,
            "sources": [format!("source-{tenant}")]
        }),
        &headers,
    )
    .await;
}

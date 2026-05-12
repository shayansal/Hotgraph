use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use rg_api::{
    router, serve_with_graceful_shutdown, ApiRole, ApiState, AuthConfig, ContextPackIntentProvider,
    DeterministicContextPackModelProvider, DeterministicQuestionEmbeddingProvider,
    QuestionEmbeddingProvider, ServiceAccount,
};
use serde_json::{json, Value};
use std::time::Duration;
use tower::ServiceExt;

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

    let health = request_empty(app, "GET", "/v1/health").await;
    assert_eq!(health["status"], "ok");
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
    let embedding_provider = DeterministicQuestionEmbeddingProvider;
    let model_provider = DeterministicContextPackModelProvider;

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
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json body")
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

//! HTTP API boundary for exposing Reality Graph services.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    extract::{Path, Query, Request, State},
    http::{header::CONTENT_TYPE, HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use rg_ai::{EvidencePackGenerator, EvidencePackRequest as AiEvidencePackRequest};
use rg_core::{
    Assertion, AssertionId, AssertionStatus, Confidence, ContextScope, Entity, EntityId,
    EntityType, GraphValue, PredicateId, Source, SourceId, SourceType, TenantId, TimeInterval,
    TxTime, ValidTime,
};
use rg_events::{AddAssertion, AddSource, CreateEntity, EventLog, GraphCommand, GraphCommandError};
use rg_ingest::{
    DeterministicFixtureExtractor, DocumentId, DocumentInput, IngestionPipeline, LineChunker,
};
use rg_query::{
    EntityPattern, GraphQuery, ObjectPattern, PathQuery, PredicatePattern, QueryEngine, QueryResult,
};
use rg_storage::{InMemoryStorage, StorageError};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};
use utoipa::{IntoParams, OpenApi, ToSchema};

#[derive(Clone)]
pub struct ApiState {
    log: Arc<Mutex<EventLog>>,
    auth: Arc<AuthConfig>,
    idempotency: Arc<Mutex<BTreeMap<String, IdempotencyRecord>>>,
    slow_query_threshold: Duration,
}

impl ApiState {
    pub fn new_in_memory() -> Self {
        Self {
            log: Arc::new(Mutex::new(EventLog::new(TxTime::new(0)))),
            auth: Arc::new(AuthConfig::disabled()),
            idempotency: Arc::new(Mutex::new(BTreeMap::new())),
            slow_query_threshold: Duration::from_millis(100),
        }
    }

    pub fn from_event_log(log: EventLog) -> Self {
        Self {
            log: Arc::new(Mutex::new(log)),
            auth: Arc::new(AuthConfig::disabled()),
            idempotency: Arc::new(Mutex::new(BTreeMap::new())),
            slow_query_threshold: Duration::from_millis(100),
        }
    }

    pub fn with_auth(mut self, auth: AuthConfig) -> Self {
        self.auth = Arc::new(auth);
        self
    }

    pub fn with_slow_query_threshold(mut self, threshold: Duration) -> Self {
        self.slow_query_threshold = threshold;
        self
    }

    fn execute(
        &self,
        command: GraphCommand,
        idempotency_key: Option<String>,
    ) -> Result<rg_events::GraphEvent, ApiError> {
        let fingerprint = format!("{command:?}");
        if let Some(key) = &idempotency_key {
            let idempotency = self
                .idempotency
                .lock()
                .map_err(|_| ApiError::internal("idempotency lock poisoned"))?;
            if let Some(record) = idempotency.get(key) {
                if record.fingerprint == fingerprint {
                    return Ok(record.event.clone());
                }
                return Err(ApiError::conflict(
                    "idempotency key reused for different command",
                ));
            }
        }

        let mut log = self
            .log
            .lock()
            .map_err(|_| ApiError::internal("event log lock poisoned"))?;
        let event = log.execute(command).map_err(ApiError::from)?;
        drop(log);

        if let Some(key) = idempotency_key {
            self.idempotency
                .lock()
                .map_err(|_| ApiError::internal("idempotency lock poisoned"))?
                .insert(
                    key,
                    IdempotencyRecord {
                        fingerprint,
                        event: event.clone(),
                    },
                );
        }
        Ok(event)
    }

    fn storage_snapshot(&self) -> Result<InMemoryStorage, ApiError> {
        let events = {
            let log = self
                .log
                .lock()
                .map_err(|_| ApiError::internal("event log lock poisoned"))?;
            log.events().to_vec()
        };
        InMemoryStorage::replay(&events).map_err(ApiError::from)
    }

    fn metrics_snapshot(&self) -> Result<MetricsResponse, ApiError> {
        let storage = self.storage_snapshot()?;
        Ok(MetricsResponse {
            entities: storage.graph_state().entities.len(),
            assertions: storage.graph_state().assertions.len(),
            sources: storage.graph_state().sources.len(),
            events: storage.events().len(),
            agent_memories: storage.graph_state().agent_memories.len(),
        })
    }

    fn health_snapshot(&self) -> Result<HealthResponse, ApiError> {
        let metrics = self.metrics_snapshot()?;
        Ok(HealthResponse {
            status: "ok".to_owned(),
            event_log: "ok".to_owned(),
            index_health: IndexHealthResponse {
                status: "ok".to_owned(),
                entities: metrics.entities,
                assertions: metrics.assertions,
                sources: metrics.sources,
                events: metrics.events,
            },
        })
    }

    fn prometheus_metrics(&self) -> Result<String, ApiError> {
        let metrics = self.metrics_snapshot()?;
        Ok(format!(
            "# HELP rg_graph_events_total Total events appended to the Reality Graph log.\n\
             # TYPE rg_graph_events_total counter\n\
             rg_graph_events_total {}\n\
             # HELP rg_graph_entities_total Total materialized entities.\n\
             # TYPE rg_graph_entities_total gauge\n\
             rg_graph_entities_total {}\n\
             # HELP rg_graph_assertions_total Total materialized assertions.\n\
             # TYPE rg_graph_assertions_total gauge\n\
             rg_graph_assertions_total {}\n\
             # HELP rg_graph_sources_total Total materialized sources.\n\
             # TYPE rg_graph_sources_total gauge\n\
             rg_graph_sources_total {}\n\
             # HELP rg_graph_agent_memories_total Total materialized agent memories.\n\
             # TYPE rg_graph_agent_memories_total gauge\n\
             rg_graph_agent_memories_total {}\n\
             # HELP rg_graph_index_health Index health status, where 1 is healthy.\n\
             # TYPE rg_graph_index_health gauge\n\
             rg_graph_index_health 1\n",
            metrics.events,
            metrics.entities,
            metrics.assertions,
            metrics.sources,
            metrics.agent_memories
        ))
    }

    fn generated_tx(&self) -> Result<TxTime, ApiError> {
        let log = self
            .log
            .lock()
            .map_err(|_| ApiError::internal("event log lock poisoned"))?;
        Ok(TxTime::new(log.events().len() as i64))
    }
}

#[derive(Clone, Debug, PartialEq)]
struct IdempotencyRecord {
    fingerprint: String,
    event: rg_events::GraphEvent,
}

pub trait QuestionEmbeddingProvider {
    fn embed_question(&self, question: &str) -> Vec<f32>;
}

pub trait ContextPackIntentProvider {
    fn infer_predicate(&self, question: &str, embedding: &[f32]) -> Option<String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DeterministicQuestionEmbeddingProvider;

impl QuestionEmbeddingProvider for DeterministicQuestionEmbeddingProvider {
    fn embed_question(&self, question: &str) -> Vec<f32> {
        let normalized = question.to_ascii_lowercase();
        if contains_any(&normalized, &["work", "employ", "job"]) {
            vec![1.0, 0.0, 0.0, 0.0]
        } else if contains_any(&normalized, &["located", "location", "based", "where is"]) {
            vec![0.0, 1.0, 0.0, 0.0]
        } else if contains_any(&normalized, &["supply", "supplier", "chain"]) {
            vec![0.0, 0.0, 1.0, 0.0]
        } else if contains_any(&normalized, &["memory", "remember", "preference"]) {
            vec![0.0, 0.0, 0.0, 1.0]
        } else {
            vec![0.25, 0.25, 0.25, 0.25]
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DeterministicContextPackModelProvider;

impl ContextPackIntentProvider for DeterministicContextPackModelProvider {
    fn infer_predicate(&self, question: &str, embedding: &[f32]) -> Option<String> {
        let normalized = question.to_ascii_lowercase();
        if contains_any(&normalized, &["work", "worked", "employ", "job"]) {
            Some("WORKED_AT".to_owned())
        } else if contains_any(&normalized, &["ceo", "chief executive"]) {
            Some("CEO_OF".to_owned())
        } else if contains_any(&normalized, &["own", "owns", "ownership", "acquired"]) {
            Some("OWNS".to_owned())
        } else if contains_any(&normalized, &["supply", "supplier", "chain"]) {
            Some("SUPPLIES".to_owned())
        } else if contains_any(&normalized, &["located", "location", "based", "where is"]) {
            Some("LOCATED_IN".to_owned())
        } else if embedding.first().copied() == Some(1.0) {
            Some("WORKED_AT".to_owned())
        } else if embedding.get(1).copied() == Some(1.0) {
            Some("LOCATED_IN".to_owned())
        } else if embedding.get(2).copied() == Some(1.0) {
            Some("SUPPLIES".to_owned())
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct AiContextPackIntent {
    subject: Option<EntityId>,
    predicate: Option<PredicateId>,
    valid_at: Option<i64>,
    known_at: Option<i64>,
    context: Option<ContextScope>,
    min_confidence: Option<f32>,
    limit: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiRole {
    Reader,
    Writer,
    Admin,
}

fn role_rank(role: &ApiRole) -> u8 {
    match role {
        ApiRole::Reader => 0,
        ApiRole::Writer => 1,
        ApiRole::Admin => 2,
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ServiceAccount {
    api_key: String,
    service_account_id: String,
    tenant_id: TenantId,
    roles: Vec<ApiRole>,
}

impl ServiceAccount {
    pub fn new(
        api_key: impl Into<String>,
        service_account_id: impl Into<String>,
        tenant_id: impl Into<String>,
        roles: Vec<ApiRole>,
    ) -> Self {
        let mut roles = roles;
        roles.sort_by_key(role_rank);
        roles.dedup();
        Self {
            api_key: api_key.into(),
            service_account_id: service_account_id.into(),
            tenant_id: TenantId::new(tenant_id.into()),
            roles,
        }
    }
}

impl std::fmt::Debug for ServiceAccount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceAccount")
            .field("api_key", &"<redacted>")
            .field("service_account_id", &self.service_account_id)
            .field("tenant_id", &self.tenant_id)
            .field("roles", &self.roles)
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiPrincipal {
    pub service_account_id: String,
    pub tenant_id: TenantId,
    pub roles: Vec<ApiRole>,
}

impl ApiPrincipal {
    fn has_role(&self, required: ApiRole) -> bool {
        self.roles.contains(&ApiRole::Admin) || self.roles.contains(&required)
    }

    fn tenant_context(&self) -> ContextScope {
        ContextScope::Named(format!("tenant:{}", self.tenant_id))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthConfig {
    accounts_by_key: BTreeMap<String, ServiceAccount>,
}

impl AuthConfig {
    pub fn new(accounts: Vec<ServiceAccount>) -> Self {
        let accounts_by_key = accounts
            .into_iter()
            .map(|account| (account.api_key.clone(), account))
            .collect();
        Self { accounts_by_key }
    }

    pub fn disabled() -> Self {
        Self::default()
    }

    fn is_enabled(&self) -> bool {
        !self.accounts_by_key.is_empty()
    }

    fn authenticate(&self, api_key: &str) -> Option<ApiPrincipal> {
        self.accounts_by_key
            .get(api_key)
            .map(|account| ApiPrincipal {
                service_account_id: account.service_account_id.clone(),
                tenant_id: account.tenant_id.clone(),
                roles: account.roles.clone(),
            })
    }
}

pub fn router(state: ApiState) -> Router {
    let auth_state = state.clone();
    Router::new()
        .route("/v1/entities", post(create_entity))
        .route("/v1/assertions", post(add_assertion))
        .route("/v1/sources", post(add_source))
        .route("/v1/events", post(post_event))
        .route("/v1/query", post(execute_query))
        .route("/v1/path", post(execute_path))
        .route("/v1/evidence-pack", post(evidence_pack))
        .route("/v1/ai/context-pack", post(ai_context_pack))
        .route("/v1/ingest/document", post(ingest_document))
        .route("/v1/entities/:id", get(get_entity))
        .route("/v1/entities/:id/state", get(get_entity_state))
        .route("/v1/assertions/:id", get(get_assertion))
        .route("/v1/sources/:id", get(get_source))
        .route("/v1/health", get(health))
        .route("/v1/metrics", get(prometheus_metrics))
        .route("/v1/metrics.json", get(metrics_json))
        .route("/v1/openapi.json", get(openapi_endpoint))
        .layer(middleware::from_fn_with_state(auth_state, auth_middleware))
        .with_state(state)
}

pub async fn serve_with_graceful_shutdown<F>(
    listener: tokio::net::TcpListener,
    state: ApiState,
    shutdown: F,
) -> std::io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await
}

pub fn openapi() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

#[derive(Clone, Debug, Default)]
struct RequestContext {
    principal: Option<ApiPrincipal>,
}

impl RequestContext {
    fn tenant_context(&self) -> Option<ContextScope> {
        self.principal.as_ref().map(ApiPrincipal::tenant_context)
    }
}

async fn auth_middleware(
    State(state): State<ApiState>,
    mut request: Request,
    next: Next,
) -> Response {
    let required_role = required_role(request.method(), request.uri().path());
    if !state.auth.is_enabled() {
        request.extensions_mut().insert(RequestContext::default());
        return next.run(request).await;
    }

    let Some(required_role) = required_role else {
        request.extensions_mut().insert(RequestContext::default());
        return next.run(request).await;
    };

    let Some(api_key) = request
        .headers()
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
    else {
        return ApiError::unauthorized("missing API key").into_response();
    };

    let Some(principal) = state.auth.authenticate(api_key) else {
        return ApiError::unauthorized("invalid API key").into_response();
    };

    if !principal.has_role(required_role) {
        warn!(
            service_account_id = %principal.service_account_id,
            tenant_id = %principal.tenant_id,
            required_role = ?required_role,
            "api_request_forbidden"
        );
        return ApiError::forbidden("service account lacks required role").into_response();
    }

    info!(
        service_account_id = %principal.service_account_id,
        tenant_id = %principal.tenant_id,
        required_role = ?required_role,
        "api_request_authorized"
    );
    request.extensions_mut().insert(RequestContext {
        principal: Some(principal),
    });
    next.run(request).await
}

fn required_role(method: &Method, path: &str) -> Option<ApiRole> {
    if matches!(path, "/v1/health" | "/v1/openapi.json") {
        return None;
    }
    if method == Method::GET {
        return Some(ApiRole::Reader);
    }
    if method == Method::POST
        && matches!(
            path,
            "/v1/query" | "/v1/path" | "/v1/evidence-pack" | "/v1/ai/context-pack"
        )
    {
        return Some(ApiRole::Reader);
    }
    Some(ApiRole::Writer)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        create_entity,
        add_assertion,
        add_source,
        post_event,
        execute_query,
        execute_path,
        evidence_pack,
        ai_context_pack,
        ingest_document,
        get_entity,
        get_entity_state,
        get_assertion,
        get_source,
        health,
        prometheus_metrics,
        metrics_json,
        openapi_endpoint
    ),
    components(schemas(
        AddAssertionRequest,
        AssertionResponse,
        AiContextPackRequest,
        CandidateAssertionResponse,
        ContradictionResponse,
        CreateEntityRequest,
        CreateSourceRequest,
        EntityRefRequest,
        EntityResponse,
        EntityStateQuery,
        EntityStateResponse,
        ErrorResponse,
        EventResponse,
        EvidencePackApiRequest,
        EvidencePackResponse,
        GraphCommandEnvelope,
        GraphQueryRequest,
        GraphValueRequest,
        GraphValueResponse,
        HealthResponse,
        IndexHealthResponse,
        IngestDocumentRequest,
        IngestDocumentResponse,
        MetricsResponse,
        PathQueryRequest,
        PathResponse,
        PathResultResponse,
        PostEventRequest,
        QueryResponse,
        QueryResultResponse,
        SourceExcerptResponse,
        SourceResponse
    ))
)]
struct ApiDoc;

#[utoipa::path(
    post,
    path = "/v1/entities",
    request_body = CreateEntityRequest,
    responses((status = 200, body = EntityResponse), (status = 400, body = ErrorResponse))
)]
#[instrument(skip(state, request))]
async fn create_entity(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateEntityRequest>,
) -> Result<Json<EntityResponse>, ApiError> {
    let command = create_entity_command(request);
    let entity_id = command.id.clone();
    state.execute(
        GraphCommand::CreateEntity(command),
        idempotency_key(&headers)?,
    )?;
    let storage = state.storage_snapshot()?;
    let entity = storage
        .entity(&entity_id)
        .ok_or_else(|| ApiError::internal("entity was not materialized"))?;
    Ok(Json(EntityResponse::from(entity)))
}

#[utoipa::path(
    post,
    path = "/v1/sources",
    request_body = CreateSourceRequest,
    responses((status = 200, body = SourceResponse), (status = 400, body = ErrorResponse))
)]
#[instrument(skip(state, request))]
async fn add_source(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateSourceRequest>,
) -> Result<Json<SourceResponse>, ApiError> {
    let source_id = source_id_from_request(&request);
    let command = add_source_command(request, source_id.clone())?;
    state.execute(GraphCommand::AddSource(command), idempotency_key(&headers)?)?;
    let storage = state.storage_snapshot()?;
    let source = storage
        .source(&source_id)
        .ok_or_else(|| ApiError::internal("source was not materialized"))?;
    Ok(Json(SourceResponse::from(source)))
}

#[utoipa::path(
    post,
    path = "/v1/assertions",
    request_body = AddAssertionRequest,
    responses((status = 200, body = AssertionResponse), (status = 400, body = ErrorResponse))
)]
#[instrument(skip(state, request))]
async fn add_assertion(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    headers: HeaderMap,
    Json(request): Json<AddAssertionRequest>,
) -> Result<Json<AssertionResponse>, ApiError> {
    let assertion_id = assertion_id_from_request(&request);
    let command = add_assertion_command(request, assertion_id.clone(), context.tenant_context())?;
    state.execute(
        GraphCommand::AddAssertion(command),
        idempotency_key(&headers)?,
    )?;
    let storage = state.storage_snapshot()?;
    let assertion = storage
        .assertion(&assertion_id)
        .ok_or_else(|| ApiError::internal("assertion was not materialized"))?;
    Ok(Json(AssertionResponse::from(assertion)))
}

#[utoipa::path(
    post,
    path = "/v1/events",
    request_body = PostEventRequest,
    responses((status = 200, body = EventResponse), (status = 400, body = ErrorResponse))
)]
#[instrument(skip(state, request))]
async fn post_event(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    headers: HeaderMap,
    Json(request): Json<PostEventRequest>,
) -> Result<Json<EventResponse>, ApiError> {
    let command = graph_command_from_envelope(request.command, context.tenant_context())?;
    let event = state.execute(command, idempotency_key(&headers)?)?;
    Ok(Json(EventResponse::from(&event)))
}

#[utoipa::path(
    post,
    path = "/v1/query",
    request_body = GraphQueryRequest,
    responses((status = 200, body = QueryResponse), (status = 400, body = ErrorResponse))
)]
#[instrument(skip(state, request))]
async fn execute_query(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Json(request): Json<GraphQueryRequest>,
) -> Result<Json<QueryResponse>, ApiError> {
    let start = Instant::now();
    let storage = state.storage_snapshot()?;
    let engine = QueryEngine::from_storage(storage);
    let results = engine
        .execute_graph(graph_query_from_request(request, context.tenant_context())?)
        .iter()
        .map(QueryResultResponse::from)
        .collect();
    log_slow_query(&state, "graph_query", start);
    Ok(Json(QueryResponse { results }))
}

#[utoipa::path(
    post,
    path = "/v1/path",
    request_body = PathQueryRequest,
    responses((status = 200, body = PathResponse), (status = 400, body = ErrorResponse))
)]
#[instrument(skip(state, request))]
async fn execute_path(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Json(request): Json<PathQueryRequest>,
) -> Result<Json<PathResponse>, ApiError> {
    let start = Instant::now();
    let storage = state.storage_snapshot()?;
    let engine = QueryEngine::from_storage(storage);
    let tenant_context = context.tenant_context();
    let mut paths = engine.execute_path(request.try_into()?);
    retain_paths_for_tenant(&mut paths, tenant_context.as_ref());
    let paths = paths.iter().map(PathResultResponse::from).collect();
    log_slow_query(&state, "path_query", start);
    Ok(Json(PathResponse { paths }))
}

#[utoipa::path(
    post,
    path = "/v1/evidence-pack",
    request_body = EvidencePackApiRequest,
    responses((status = 200, body = EvidencePackResponse), (status = 400, body = ErrorResponse))
)]
#[instrument(skip(state, request))]
async fn evidence_pack(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Json(request): Json<EvidencePackApiRequest>,
) -> Result<Json<EvidencePackResponse>, ApiError> {
    let start = Instant::now();
    let storage = state.storage_snapshot()?;
    let generator = EvidencePackGenerator::new(&storage);
    let tenant_context = context.tenant_context();
    let generated_at = request
        .generated_at
        .as_deref()
        .map(parse_timestamp)
        .transpose()?
        .map_or_else(|| state.generated_tx(), |value| Ok(TxTime::new(value)))?;
    let pack = generator.generate(AiEvidencePackRequest {
        query: request.query,
        graph_query: graph_query_from_request(request.graph_query, tenant_context.clone())?,
        path_query: request.path_query.map(TryInto::try_into).transpose()?,
        generated_at,
    });
    let mut response = EvidencePackResponse::from(&pack);
    retain_evidence_response_for_tenant(&mut response, tenant_context.as_ref());
    log_slow_query(&state, "evidence_pack", start);
    Ok(Json(response))
}

#[utoipa::path(
    post,
    path = "/v1/ai/context-pack",
    request_body = AiContextPackRequest,
    responses((status = 200, body = EvidencePackResponse), (status = 400, body = ErrorResponse))
)]
#[instrument(skip(state, request))]
async fn ai_context_pack(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Json(request): Json<AiContextPackRequest>,
) -> Result<Json<EvidencePackResponse>, ApiError> {
    let start = Instant::now();
    let storage = state.storage_snapshot()?;
    let tenant_context = context.tenant_context();
    let intent = ai_context_pack_intent(&storage, &request, tenant_context.clone())?;
    let graph_query = GraphQuery {
        subject: intent.subject.clone().map(EntityPattern::Id),
        predicate: intent.predicate.clone().map(PredicatePattern::Id),
        object: None,
        valid_at: intent.valid_at,
        known_at: intent.known_at,
        context: intent.context,
        min_confidence: intent.min_confidence,
        limit: intent.limit,
    };
    let path_query = intent.subject.clone().map(|start| PathQuery {
        start,
        end: None,
        predicates: intent.predicate.iter().cloned().collect(),
        valid_at: intent.valid_at,
        max_depth: 2,
        min_confidence: intent.min_confidence,
    });
    let generator = EvidencePackGenerator::new(&storage);
    let pack = generator.generate(AiEvidencePackRequest {
        query: request.question,
        graph_query,
        path_query,
        generated_at: state.generated_tx()?,
    });
    let mut response = EvidencePackResponse::from(&pack);
    retain_evidence_response_for_tenant(&mut response, tenant_context.as_ref());
    log_slow_query(&state, "ai_context_pack", start);
    Ok(Json(response))
}

#[utoipa::path(
    post,
    path = "/v1/ingest/document",
    request_body = IngestDocumentRequest,
    responses((status = 200, body = IngestDocumentResponse), (status = 400, body = ErrorResponse))
)]
#[instrument(skip(request))]
async fn ingest_document(
    Json(request): Json<IngestDocumentRequest>,
) -> Result<Json<IngestDocumentResponse>, ApiError> {
    let document = DocumentInput {
        id: DocumentId::new(request.id),
        source_id: SourceId::new(request.source_id),
        uri: request.uri,
        content: request.content,
    };
    let pipeline = IngestionPipeline::new(
        LineChunker::new(),
        DeterministicFixtureExtractor::new("api-fixture-extractor-v1"),
    );
    let batch = pipeline.extract(&document).map_err(ApiError::from)?;
    Ok(Json(IngestDocumentResponse::from(&batch)))
}

#[utoipa::path(
    get,
    path = "/v1/entities/{id}",
    params(("id" = String, Path, description = "Entity ID")),
    responses((status = 200, body = EntityResponse), (status = 404, body = ErrorResponse))
)]
async fn get_entity(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<EntityResponse>, ApiError> {
    let storage = state.storage_snapshot()?;
    let entity = storage
        .entity(&EntityId::new(id.clone()))
        .ok_or_else(|| ApiError::not_found(format!("entity not found: {id}")))?;
    Ok(Json(EntityResponse::from(entity)))
}

#[utoipa::path(
    get,
    path = "/v1/entities/{id}/state",
    params(
        ("id" = String, Path, description = "Entity ID"),
        EntityStateQuery
    ),
    responses((status = 200, body = EntityStateResponse), (status = 404, body = ErrorResponse))
)]
async fn get_entity_state(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<String>,
    Query(query): Query<EntityStateQuery>,
) -> Result<Json<EntityStateResponse>, ApiError> {
    let storage = state.storage_snapshot()?;
    let entity_id = EntityId::new(id.clone());
    let entity = storage
        .entity(&entity_id)
        .ok_or_else(|| ApiError::not_found(format!("entity not found: {id}")))?;
    let valid_at = query
        .valid_at
        .as_deref()
        .map(parse_timestamp)
        .transpose()?
        .map(ValidTime::new);
    let mut assertions = storage.assertions_by_subject(&entity_id);
    let tenant_context = context.tenant_context();
    assertions.retain(|assertion| {
        valid_at.map_or(true, |instant| assertion.valid_time.contains(instant))
            && tenant_context
                .as_ref()
                .map_or(true, |context| &assertion.context == context)
    });
    assertions.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(Json(EntityStateResponse {
        entity: EntityResponse::from(entity),
        assertions: assertions
            .iter()
            .map(|assertion| AssertionResponse::from(*assertion))
            .collect(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/assertions/{id}",
    params(("id" = String, Path, description = "Assertion ID")),
    responses((status = 200, body = AssertionResponse), (status = 404, body = ErrorResponse))
)]
async fn get_assertion(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<String>,
) -> Result<Json<AssertionResponse>, ApiError> {
    let storage = state.storage_snapshot()?;
    let assertion = storage
        .assertion(&AssertionId::new(id.clone()))
        .ok_or_else(|| ApiError::not_found(format!("assertion not found: {id}")))?;
    if context
        .tenant_context()
        .as_ref()
        .is_some_and(|tenant_context| &assertion.context != tenant_context)
    {
        return Err(ApiError::not_found(format!("assertion not found: {id}")));
    }
    Ok(Json(AssertionResponse::from(assertion)))
}

#[utoipa::path(
    get,
    path = "/v1/sources/{id}",
    params(("id" = String, Path, description = "Source ID")),
    responses((status = 200, body = SourceResponse), (status = 404, body = ErrorResponse))
)]
async fn get_source(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<SourceResponse>, ApiError> {
    let storage = state.storage_snapshot()?;
    let source = storage
        .source(&SourceId::new(id.clone()))
        .ok_or_else(|| ApiError::not_found(format!("source not found: {id}")))?;
    Ok(Json(SourceResponse::from(source)))
}

#[utoipa::path(
    get,
    path = "/v1/health",
    responses((status = 200, body = HealthResponse))
)]
async fn health(State(state): State<ApiState>) -> Result<Json<HealthResponse>, ApiError> {
    Ok(Json(state.health_snapshot()?))
}

#[utoipa::path(
    get,
    path = "/v1/metrics",
    responses(
        (status = 200, content_type = "text/plain", body = String),
        (status = 500, body = ErrorResponse)
    )
)]
async fn prometheus_metrics(State(state): State<ApiState>) -> Result<Response, ApiError> {
    Ok((
        [(CONTENT_TYPE, "text/plain; version=0.0.4")],
        state.prometheus_metrics()?,
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/v1/metrics.json",
    responses((status = 200, body = MetricsResponse), (status = 500, body = ErrorResponse))
)]
async fn metrics_json(State(state): State<ApiState>) -> Result<Json<MetricsResponse>, ApiError> {
    Ok(Json(state.metrics_snapshot()?))
}

#[utoipa::path(
    get,
    path = "/v1/openapi.json",
    responses((status = 200, description = "OpenAPI document"))
)]
async fn openapi_endpoint() -> Json<utoipa::openapi::OpenApi> {
    Json(openapi())
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct CreateEntityRequest {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub entity_type: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct CreateSourceRequest {
    pub id: Option<String>,
    pub source_type: Option<String>,
    pub uri: Option<String>,
    pub content_hash: String,
    pub trust_score: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AddAssertionRequest {
    pub id: Option<String>,
    pub subject: String,
    pub predicate: String,
    pub object: GraphValueRequest,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub confidence: f32,
    pub sources: Vec<String>,
    pub context: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct EntityRefRequest {
    pub entity_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct GraphValueRequest {
    pub entity_id: Option<String>,
    pub text: Option<String>,
    pub integer: Option<i64>,
    pub decimal: Option<f64>,
    pub boolean: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct GraphQueryRequest {
    pub subject: Option<EntityRefRequest>,
    pub predicate: Option<String>,
    pub object: Option<GraphValueRequest>,
    pub valid_at: Option<String>,
    pub known_at: Option<String>,
    pub context: Option<String>,
    pub min_confidence: Option<f32>,
    pub limit: Option<usize>,
    pub include_sources: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct PathQueryRequest {
    pub start: String,
    pub end: Option<String>,
    pub predicates: Vec<String>,
    pub valid_at: Option<String>,
    pub max_depth: usize,
    pub min_confidence: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct EvidencePackApiRequest {
    pub query: String,
    pub graph_query: GraphQueryRequest,
    pub path_query: Option<PathQueryRequest>,
    pub generated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AiContextPackRequest {
    pub question: String,
    pub valid_at: Option<String>,
    pub known_at: Option<String>,
    pub entity_ids: Option<Vec<String>>,
    pub predicates: Option<Vec<String>>,
    pub context: Option<String>,
    pub min_confidence: Option<f32>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct IngestDocumentRequest {
    pub id: String,
    pub source_id: String,
    pub uri: Option<String>,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct PostEventRequest {
    pub command: GraphCommandEnvelope,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum GraphCommandEnvelope {
    CreateEntity(CreateEntityRequest),
    AddSource(CreateSourceRequest),
    AddAssertion(AddAssertionRequest),
}

#[derive(Clone, Debug, Deserialize, IntoParams, Serialize, ToSchema)]
pub struct EntityStateQuery {
    pub valid_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct EntityResponse {
    pub id: String,
    pub entity_type: String,
    pub canonical_name: Option<String>,
    pub created_tx: i64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct SourceResponse {
    pub id: String,
    pub source_type: String,
    pub uri: Option<String>,
    pub content_hash: String,
    pub observed_at: i64,
    pub trust_score: Option<f32>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AssertionResponse {
    pub assertion_id: String,
    pub subject: String,
    pub predicate: String,
    pub object: GraphValueResponse,
    pub valid_from: i64,
    pub valid_to: Option<i64>,
    pub tx_from: i64,
    pub tx_to: Option<i64>,
    pub confidence: f32,
    pub sources: Vec<String>,
    pub context: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct GraphValueResponse {
    pub entity_id: Option<String>,
    pub text: Option<String>,
    pub integer: Option<i64>,
    pub decimal: Option<f64>,
    pub boolean: Option<bool>,
    pub time: Option<i64>,
    pub null: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct QueryResultResponse {
    pub assertion_id: String,
    pub subject: String,
    pub predicate: String,
    pub object: GraphValueResponse,
    pub valid_from: i64,
    pub valid_to: Option<i64>,
    pub tx_from: i64,
    pub tx_to: Option<i64>,
    pub confidence: f32,
    pub sources: Vec<String>,
    pub context: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct QueryResponse {
    pub results: Vec<QueryResultResponse>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PathResultResponse {
    pub start: String,
    pub end: String,
    pub hops: Vec<QueryResultResponse>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PathResponse {
    pub paths: Vec<PathResultResponse>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct SourceExcerptResponse {
    pub source_id: String,
    pub source_type: String,
    pub uri: Option<String>,
    pub content_hash: String,
    pub snippet: String,
    pub trust_score: Option<f32>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ContradictionResponse {
    pub id: String,
    pub assertion_a: String,
    pub assertion_b: String,
    pub contradiction_type: String,
    pub severity: String,
    pub explanation: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct EvidencePackResponse {
    pub query: String,
    pub entities: Vec<EntityResponse>,
    pub assertions: Vec<AssertionResponse>,
    pub sources: Vec<SourceExcerptResponse>,
    pub paths: Vec<PathResultResponse>,
    pub contradictions: Vec<ContradictionResponse>,
    pub generated_at: i64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CandidateAssertionResponse {
    pub subject_text: String,
    pub predicate_text: String,
    pub object_text: String,
    pub valid_from: Option<i64>,
    pub valid_to: Option<i64>,
    pub confidence: f32,
    pub source_id: String,
    pub source_excerpt: String,
    pub extraction_model: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct IngestDocumentResponse {
    pub document_id: String,
    pub candidates: Vec<CandidateAssertionResponse>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct EntityStateResponse {
    pub entity: EntityResponse,
    pub assertions: Vec<AssertionResponse>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct EventResponse {
    pub event_id: String,
    pub transaction_time: i64,
    pub event_type: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub event_log: String,
    pub index_health: IndexHealthResponse,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct IndexHealthResponse {
    pub status: String,
    pub entities: usize,
    pub assertions: usize,
    pub sources: usize,
    pub events: usize,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MetricsResponse {
    pub entities: usize,
    pub assertions: usize,
    pub sources: usize,
    pub events: usize,
    pub agent_memories: usize,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

impl From<GraphCommandError> for ApiError {
    fn from(error: GraphCommandError) -> Self {
        Self::bad_request(format!("{error:?}"))
    }
}

impl From<StorageError> for ApiError {
    fn from(error: StorageError) -> Self {
        Self::internal(format!("{error:?}"))
    }
}

impl From<rg_ingest::IngestError> for ApiError {
    fn from(error: rg_ingest::IngestError) -> Self {
        Self::bad_request(error.to_string())
    }
}

impl TryFrom<GraphQueryRequest> for GraphQuery {
    type Error = ApiError;

    fn try_from(request: GraphQueryRequest) -> Result<Self, Self::Error> {
        graph_query_from_request(request, None)
    }
}

impl TryFrom<PathQueryRequest> for PathQuery {
    type Error = ApiError;

    fn try_from(request: PathQueryRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            start: EntityId::new(request.start),
            end: request.end.map(EntityId::new),
            predicates: request
                .predicates
                .into_iter()
                .map(PredicateId::new)
                .collect(),
            valid_at: request
                .valid_at
                .as_deref()
                .map(parse_timestamp)
                .transpose()?,
            max_depth: request.max_depth,
            min_confidence: request.min_confidence,
        })
    }
}

impl From<&Entity> for EntityResponse {
    fn from(entity: &Entity) -> Self {
        Self {
            id: entity.id.as_str().to_owned(),
            entity_type: entity_type_name(&entity.entity_type),
            canonical_name: entity.canonical_name.clone(),
            created_tx: entity.created_tx.as_i64(),
        }
    }
}

impl From<&Source> for SourceResponse {
    fn from(source: &Source) -> Self {
        Self {
            id: source.id.as_str().to_owned(),
            source_type: source_type_name(&source.source_type),
            uri: source.uri.clone(),
            content_hash: source.content_hash.as_str().to_owned(),
            observed_at: source.observed_at.as_i64(),
            trust_score: source.trust_score,
        }
    }
}

impl From<&Assertion> for AssertionResponse {
    fn from(assertion: &Assertion) -> Self {
        Self {
            assertion_id: assertion.id.as_str().to_owned(),
            subject: assertion.subject.as_str().to_owned(),
            predicate: assertion.predicate.as_str().to_owned(),
            object: GraphValueResponse::from(&assertion.object),
            valid_from: assertion.valid_time.start.as_i64(),
            valid_to: assertion.valid_time.end.map(ValidTime::as_i64),
            tx_from: assertion.transaction_time.start.as_i64(),
            tx_to: assertion.transaction_time.end.map(TxTime::as_i64),
            confidence: assertion.confidence.as_f32(),
            sources: assertion
                .source_ids
                .iter()
                .map(|source| source.as_str().to_owned())
                .collect(),
            context: context_name(&assertion.context),
            status: assertion_status_name(&assertion.status),
        }
    }
}

impl From<&GraphValue> for GraphValueResponse {
    fn from(value: &GraphValue) -> Self {
        match value {
            GraphValue::Entity(id) => Self {
                entity_id: Some(id.as_str().to_owned()),
                text: None,
                integer: None,
                decimal: None,
                boolean: None,
                time: None,
                null: false,
            },
            GraphValue::Text(value) => Self {
                entity_id: None,
                text: Some(value.clone()),
                integer: None,
                decimal: None,
                boolean: None,
                time: None,
                null: false,
            },
            GraphValue::Integer(value) => Self {
                entity_id: None,
                text: None,
                integer: Some(*value),
                decimal: None,
                boolean: None,
                time: None,
                null: false,
            },
            GraphValue::Decimal(value) => Self {
                entity_id: None,
                text: None,
                integer: None,
                decimal: Some(*value),
                boolean: None,
                time: None,
                null: false,
            },
            GraphValue::Boolean(value) => Self {
                entity_id: None,
                text: None,
                integer: None,
                decimal: None,
                boolean: Some(*value),
                time: None,
                null: false,
            },
            GraphValue::Time(value) => Self {
                entity_id: None,
                text: None,
                integer: None,
                decimal: None,
                boolean: None,
                time: Some(value.as_i64()),
                null: false,
            },
            GraphValue::Null => Self {
                entity_id: None,
                text: None,
                integer: None,
                decimal: None,
                boolean: None,
                time: None,
                null: true,
            },
        }
    }
}

impl From<&QueryResult> for QueryResultResponse {
    fn from(result: &QueryResult) -> Self {
        Self {
            assertion_id: result.assertion_id.as_str().to_owned(),
            subject: result.subject.as_str().to_owned(),
            predicate: result.predicate.as_str().to_owned(),
            object: GraphValueResponse::from(&result.object),
            valid_from: result.valid_from.as_i64(),
            valid_to: result.valid_to.map(ValidTime::as_i64),
            tx_from: result.tx_from.as_i64(),
            tx_to: result.tx_to.map(TxTime::as_i64),
            confidence: result.confidence.as_f32(),
            sources: result
                .source_ids
                .iter()
                .map(|source| source.as_str().to_owned())
                .collect(),
            context: context_name(&result.context),
        }
    }
}

impl From<&rg_query::PathResult> for PathResultResponse {
    fn from(path: &rg_query::PathResult) -> Self {
        Self {
            start: path.start.as_str().to_owned(),
            end: path.end.as_str().to_owned(),
            hops: path.hops.iter().map(QueryResultResponse::from).collect(),
        }
    }
}

impl From<&rg_ai::GraphPath> for PathResultResponse {
    fn from(path: &rg_ai::GraphPath) -> Self {
        Self {
            start: path.start.as_str().to_owned(),
            end: path.end.as_str().to_owned(),
            hops: path.hops.iter().map(QueryResultResponse::from).collect(),
        }
    }
}

impl From<&rg_ai::EvidencePack> for EvidencePackResponse {
    fn from(pack: &rg_ai::EvidencePack) -> Self {
        Self {
            query: pack.query.clone(),
            entities: pack.entities.iter().map(EntityResponse::from).collect(),
            assertions: pack
                .assertions
                .iter()
                .map(AssertionResponse::from)
                .collect(),
            sources: pack
                .sources
                .iter()
                .map(SourceExcerptResponse::from)
                .collect(),
            paths: pack.paths.iter().map(PathResultResponse::from).collect(),
            contradictions: pack
                .contradictions
                .iter()
                .map(|contradiction| ContradictionResponse {
                    id: contradiction.id.as_str().to_owned(),
                    assertion_a: contradiction.assertion_a.as_str().to_owned(),
                    assertion_b: contradiction.assertion_b.as_str().to_owned(),
                    contradiction_type: contradiction.contradiction_type.to_string(),
                    severity: contradiction.severity.to_string(),
                    explanation: contradiction.explanation.clone(),
                })
                .collect(),
            generated_at: pack.generated_at.as_i64(),
        }
    }
}

impl From<&rg_ai::SourceExcerpt> for SourceExcerptResponse {
    fn from(source: &rg_ai::SourceExcerpt) -> Self {
        Self {
            source_id: source.source_id.as_str().to_owned(),
            source_type: source_type_name(&source.source_type),
            uri: source.uri.clone(),
            content_hash: source.content_hash.as_str().to_owned(),
            snippet: source.snippet.clone(),
            trust_score: source.trust_score,
        }
    }
}

impl From<&rg_ingest::ExtractionBatch> for IngestDocumentResponse {
    fn from(batch: &rg_ingest::ExtractionBatch) -> Self {
        Self {
            document_id: batch.document_id.as_str().to_owned(),
            candidates: batch
                .candidates
                .iter()
                .map(CandidateAssertionResponse::from)
                .collect(),
        }
    }
}

impl From<&rg_ingest::CandidateAssertion> for CandidateAssertionResponse {
    fn from(candidate: &rg_ingest::CandidateAssertion) -> Self {
        Self {
            subject_text: candidate.subject_text.clone(),
            predicate_text: candidate.predicate_text.clone(),
            object_text: candidate.object_text.clone(),
            valid_from: candidate
                .valid_time
                .as_ref()
                .map(|valid| valid.start.as_i64()),
            valid_to: candidate
                .valid_time
                .as_ref()
                .and_then(|valid| valid.end.map(ValidTime::as_i64)),
            confidence: candidate.confidence.as_f32(),
            source_id: candidate.source_excerpt.source_id.as_str().to_owned(),
            source_excerpt: candidate.source_excerpt.text.clone(),
            extraction_model: candidate.extraction_model.clone(),
        }
    }
}

impl From<&rg_events::GraphEvent> for EventResponse {
    fn from(event: &rg_events::GraphEvent) -> Self {
        Self {
            event_id: event.event_id().as_str().to_owned(),
            transaction_time: event.transaction_time().as_i64(),
            event_type: event_type_name(event),
        }
    }
}

fn create_entity_command(request: CreateEntityRequest) -> CreateEntity {
    let id = request.id.unwrap_or_else(|| {
        format!(
            "entity-{}",
            slugify(request.name.as_deref().unwrap_or(&request.entity_type))
        )
    });
    CreateEntity {
        id: EntityId::new(id),
        entity_type: entity_type_from_name(&request.entity_type),
        canonical_name: request.name,
        properties: rg_core::PropertyMap::default(),
    }
}

fn add_source_command(
    request: CreateSourceRequest,
    source_id: SourceId,
) -> Result<AddSource, ApiError> {
    Ok(AddSource {
        id: source_id,
        source_type: request
            .source_type
            .as_deref()
            .map_or(SourceType::Document, source_type_from_name),
        uri: request.uri,
        content_hash: rg_core::ContentHash::new(request.content_hash),
        trust_score: request.trust_score,
    })
}

fn add_assertion_command(
    request: AddAssertionRequest,
    assertion_id: AssertionId,
    tenant_context: Option<ContextScope>,
) -> Result<AddAssertion, ApiError> {
    let valid_from = parse_timestamp(&request.valid_from)?;
    let valid_to = request
        .valid_to
        .as_deref()
        .map(parse_timestamp)
        .transpose()?;
    Ok(AddAssertion {
        id: assertion_id,
        subject: EntityId::new(request.subject),
        predicate: PredicateId::new(request.predicate),
        object: graph_value_from_request(request.object)?,
        valid_time: TimeInterval::new(ValidTime::new(valid_from), valid_to.map(ValidTime::new))
            .map_err(|error| ApiError::bad_request(format!("{error:?}")))?,
        confidence: Confidence::new(request.confidence)
            .map_err(|error| ApiError::bad_request(format!("{error:?}")))?,
        source_ids: request.sources.into_iter().map(SourceId::new).collect(),
        context: scoped_context(request.context, tenant_context)?,
    })
}

fn graph_command_from_envelope(
    envelope: GraphCommandEnvelope,
    tenant_context: Option<ContextScope>,
) -> Result<GraphCommand, ApiError> {
    match envelope {
        GraphCommandEnvelope::CreateEntity(request) => {
            Ok(GraphCommand::CreateEntity(create_entity_command(request)))
        }
        GraphCommandEnvelope::AddSource(request) => {
            let source_id = source_id_from_request(&request);
            Ok(GraphCommand::AddSource(add_source_command(
                request, source_id,
            )?))
        }
        GraphCommandEnvelope::AddAssertion(request) => {
            let assertion_id = assertion_id_from_request(&request);
            Ok(GraphCommand::AddAssertion(add_assertion_command(
                request,
                assertion_id,
                tenant_context,
            )?))
        }
    }
}

fn graph_query_from_request(
    request: GraphQueryRequest,
    tenant_context: Option<ContextScope>,
) -> Result<GraphQuery, ApiError> {
    Ok(GraphQuery {
        subject: request
            .subject
            .map(|subject| EntityPattern::Id(EntityId::new(subject.entity_id))),
        predicate: request
            .predicate
            .map(|predicate| PredicatePattern::Id(PredicateId::new(predicate))),
        object: request
            .object
            .map(graph_value_from_request)
            .transpose()?
            .map(ObjectPattern::Value),
        valid_at: request
            .valid_at
            .as_deref()
            .map(parse_timestamp)
            .transpose()?,
        known_at: request
            .known_at
            .as_deref()
            .map(parse_timestamp)
            .transpose()?,
        context: scoped_optional_context(request.context, tenant_context)?,
        min_confidence: request.min_confidence,
        limit: request.limit,
    })
}

fn ai_context_pack_intent(
    storage: &InMemoryStorage,
    request: &AiContextPackRequest,
    tenant_context: Option<ContextScope>,
) -> Result<AiContextPackIntent, ApiError> {
    let question = request.question.trim();
    if question.is_empty() {
        return Err(ApiError::bad_request("question must not be empty"));
    }

    let embedding_provider = DeterministicQuestionEmbeddingProvider;
    let model_provider = DeterministicContextPackModelProvider;
    let question_embedding = embedding_provider.embed_question(question);
    let predicate = request
        .predicates
        .as_ref()
        .and_then(|predicates| {
            predicates
                .iter()
                .find(|predicate| !predicate.trim().is_empty())
        })
        .map(|predicate| PredicateId::new(predicate.trim().to_owned()))
        .or_else(|| {
            model_provider
                .infer_predicate(question, &question_embedding)
                .map(PredicateId::new)
        });
    let subject = request
        .entity_ids
        .as_ref()
        .and_then(|entity_ids| {
            entity_ids
                .iter()
                .find(|entity_id| !entity_id.trim().is_empty())
        })
        .map(|entity_id| EntityId::new(entity_id.trim().to_owned()))
        .or_else(|| infer_entity_from_question(storage, question));

    Ok(AiContextPackIntent {
        subject,
        predicate,
        valid_at: request
            .valid_at
            .as_deref()
            .map(parse_timestamp)
            .transpose()?,
        known_at: request
            .known_at
            .as_deref()
            .map(parse_timestamp)
            .transpose()?,
        context: scoped_optional_context(request.context.clone(), tenant_context)?,
        min_confidence: request.min_confidence,
        limit: request.limit,
    })
}

fn infer_entity_from_question(storage: &InMemoryStorage, question: &str) -> Option<EntityId> {
    let normalized_question = question.to_ascii_lowercase();
    storage
        .graph_state()
        .entities
        .values()
        .find(|entity| {
            normalized_question.contains(&entity.id.as_str().to_ascii_lowercase())
                || entity
                    .canonical_name
                    .as_ref()
                    .is_some_and(|name| normalized_question.contains(&name.to_ascii_lowercase()))
        })
        .map(|entity| entity.id.clone())
}

fn scoped_context(
    requested_context: Option<String>,
    tenant_context: Option<ContextScope>,
) -> Result<ContextScope, ApiError> {
    scoped_optional_context(requested_context, tenant_context)
        .map(|context| context.unwrap_or(ContextScope::Global))
}

fn scoped_optional_context(
    requested_context: Option<String>,
    tenant_context: Option<ContextScope>,
) -> Result<Option<ContextScope>, ApiError> {
    let Some(tenant_context) = tenant_context else {
        return Ok(requested_context.map(ContextScope::Named));
    };
    match requested_context {
        Some(context) if ContextScope::Named(context.clone()) == tenant_context => {
            Ok(Some(ContextScope::Named(context)))
        }
        Some(_) => Err(ApiError::forbidden(
            "request context is outside the authenticated tenant",
        )),
        None => Ok(Some(tenant_context)),
    }
}

fn idempotency_key(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    headers
        .get("idempotency-key")
        .map(|value| {
            value
                .to_str()
                .map(str::trim)
                .map(str::to_owned)
                .map_err(|_| ApiError::bad_request("idempotency key must be valid UTF-8"))
        })
        .transpose()
        .map(|key| key.filter(|value| !value.is_empty()))
}

fn retain_paths_for_tenant(
    paths: &mut Vec<rg_query::PathResult>,
    tenant_context: Option<&ContextScope>,
) {
    let Some(tenant_context) = tenant_context else {
        return;
    };
    paths.retain(|path| path.hops.iter().all(|hop| &hop.context == tenant_context));
}

fn retain_evidence_response_for_tenant(
    response: &mut EvidencePackResponse,
    tenant_context: Option<&ContextScope>,
) {
    let Some(tenant_context) = tenant_context else {
        return;
    };
    let tenant_name = context_name(tenant_context);
    response
        .assertions
        .retain(|assertion| assertion.context == tenant_name);
    response
        .paths
        .retain(|path| path.hops.iter().all(|hop| hop.context == tenant_name));
}

fn log_slow_query(state: &ApiState, operation: &'static str, start: Instant) {
    let elapsed = start.elapsed();
    if elapsed >= state.slow_query_threshold {
        warn!(operation, elapsed_ms = elapsed.as_millis(), "slow_query");
    }
}

fn graph_value_from_request(request: GraphValueRequest) -> Result<GraphValue, ApiError> {
    if let Some(entity_id) = request.entity_id {
        return Ok(GraphValue::Entity(EntityId::new(entity_id)));
    }
    if let Some(text) = request.text {
        return Ok(GraphValue::Text(text));
    }
    if let Some(integer) = request.integer {
        return Ok(GraphValue::Integer(integer));
    }
    if let Some(decimal) = request.decimal {
        return Ok(GraphValue::Decimal(decimal));
    }
    if let Some(boolean) = request.boolean {
        return Ok(GraphValue::Boolean(boolean));
    }
    Err(ApiError::bad_request(
        "graph value must set one value field",
    ))
}

fn source_id_from_request(request: &CreateSourceRequest) -> SourceId {
    SourceId::new(
        request
            .id
            .clone()
            .unwrap_or_else(|| format!("source-{}", slugify(&request.content_hash))),
    )
}

fn assertion_id_from_request(request: &AddAssertionRequest) -> AssertionId {
    AssertionId::new(request.id.clone().unwrap_or_else(|| {
        let object = request
            .object
            .entity_id
            .as_deref()
            .or(request.object.text.as_deref())
            .unwrap_or("value");
        let source = request.sources.first().map_or("source", String::as_str);
        format!(
            "assertion-{}-{}-{}-{}",
            slugify(&request.subject),
            slugify(&request.predicate),
            slugify(object),
            slugify(source)
        )
    }))
}

fn parse_timestamp(value: &str) -> Result<i64, ApiError> {
    if let Ok(parsed) = value.parse::<i64>() {
        return Ok(parsed);
    }
    let digits = value
        .chars()
        .filter(|character| character.is_ascii_digit())
        .take(8)
        .collect::<String>();
    if digits.len() >= 4 {
        return digits
            .parse::<i64>()
            .map_err(|_| ApiError::bad_request(format!("timestamp could not be parsed: {value}")));
    }
    Err(ApiError::bad_request(format!(
        "timestamp could not be parsed: {value}"
    )))
}

fn entity_type_from_name(value: &str) -> EntityType {
    match value {
        "Person" | "person" => EntityType::Person,
        "Organization" | "organization" | "Company" | "company" => EntityType::Organization,
        "Place" | "place" => EntityType::Place,
        "Event" | "event" => EntityType::Event,
        "Document" | "document" => EntityType::Document,
        "Concept" | "concept" => EntityType::Concept,
        custom => EntityType::Custom(custom.to_owned()),
    }
}

fn entity_type_name(value: &EntityType) -> String {
    match value {
        EntityType::Person => "Person".to_owned(),
        EntityType::Organization => "Organization".to_owned(),
        EntityType::Place => "Place".to_owned(),
        EntityType::Event => "Event".to_owned(),
        EntityType::Document => "Document".to_owned(),
        EntityType::Concept => "Concept".to_owned(),
        EntityType::Custom(value) => value.clone(),
    }
}

fn source_type_from_name(value: &str) -> SourceType {
    match value {
        "Document" | "document" => SourceType::Document,
        "WebPage" | "web_page" | "webpage" => SourceType::WebPage,
        "DatabaseRecord" | "database_record" => SourceType::DatabaseRecord,
        "ApiResponse" | "api_response" => SourceType::ApiResponse,
        "HumanReport" | "human_report" => SourceType::HumanReport,
        "SensorReading" | "sensor_reading" => SourceType::SensorReading,
        custom => SourceType::Custom(custom.to_owned()),
    }
}

fn source_type_name(value: &SourceType) -> String {
    match value {
        SourceType::Document => "Document".to_owned(),
        SourceType::WebPage => "WebPage".to_owned(),
        SourceType::DatabaseRecord => "DatabaseRecord".to_owned(),
        SourceType::ApiResponse => "ApiResponse".to_owned(),
        SourceType::HumanReport => "HumanReport".to_owned(),
        SourceType::SensorReading => "SensorReading".to_owned(),
        SourceType::Custom(value) => value.clone(),
    }
}

fn context_name(value: &ContextScope) -> String {
    match value {
        ContextScope::Global => "global".to_owned(),
        ContextScope::Named(value) => value.clone(),
    }
}

fn assertion_status_name(value: &AssertionStatus) -> String {
    match value {
        AssertionStatus::Active => "active".to_owned(),
        AssertionStatus::Retracted => "retracted".to_owned(),
        AssertionStatus::Superseded => "superseded".to_owned(),
        AssertionStatus::Disputed => "disputed".to_owned(),
    }
}

fn event_type_name(event: &rg_events::GraphEvent) -> String {
    match event {
        rg_events::GraphEvent::EntityCreated(_) => "entity_created",
        rg_events::GraphEvent::AssertionAdded(_) => "assertion_added",
        rg_events::GraphEvent::AssertionRetracted(_) => "assertion_retracted",
        rg_events::GraphEvent::SourceAdded(_) => "source_added",
        rg_events::GraphEvent::EvidenceLinked(_) => "evidence_linked",
        rg_events::GraphEvent::EntityMerged(_) => "entity_merged",
        rg_events::GraphEvent::ConfidenceUpdated(_) => "confidence_updated",
        rg_events::GraphEvent::CausalLinkAdded(_) => "causal_link_added",
        rg_events::GraphEvent::AgentMemoryRecorded(_) => "agent_memory_recorded",
    }
    .to_owned()
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            previous_separator = false;
        } else if character == '_' {
            slug.push('_');
            previous_separator = false;
        } else if !previous_separator {
            slug.push('-');
            previous_separator = true;
        }
    }
    let slug = slug.trim_matches('-').to_owned();
    if slug.is_empty() {
        "unknown".to_owned()
    } else {
        slug
    }
}

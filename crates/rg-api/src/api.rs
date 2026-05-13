//! HTTP API boundary for exposing Reality Graph services.

use crate::fixture_ai::{
    ContextPackIntentProvider, FixtureContextPackIntentProvider, FixtureQuestionEmbeddingProvider,
    QuestionEmbeddingProvider,
};

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::future::Future;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    extract::{DefaultBodyLimit, Path, Query, Request, State},
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
use rg_governance::{AccessDenial, AuditReason, GovernanceEngine, Principal, PrincipalId};
use rg_ingest::{
    DeterministicFixtureExtractor, DocumentId, DocumentInput, IngestionPipeline, LineChunker,
};
use rg_query::{
    EntityPattern, GraphQuery, ObjectPattern, PathQuery, PredicatePattern, QueryEngine, QueryResult,
};
use rg_storage::{
    DurableAssertionQuery, FileEventLog, FollowerStatus, InMemoryStorage, RedbGraphStore,
    ReplicationBatch, StorageError, WalAppendMetadata,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{info, instrument, warn};
use utoipa::{IntoParams, OpenApi, ToSchema};

const DEFAULT_SLOW_QUERY_THRESHOLD: Duration = Duration::from_millis(100);
const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;
const DEFAULT_QUERY_LIMIT: usize = 100;
const DEFAULT_MAX_QUERY_LIMIT: usize = 1000;
const DEFAULT_MAX_PATH_DEPTH: usize = 8;
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, PartialEq)]
enum NodeRole {
    Writer,
    Reader {
        writer_url: String,
        max_lag_lsn: Option<u64>,
    },
}

impl NodeRole {
    fn from_env() -> Result<Self, ApiConfigError> {
        match std::env::var("HOTGRAPH_NODE_ROLE") {
            Ok(value) if value.eq_ignore_ascii_case("reader") => {
                let writer_url = std::env::var("HOTGRAPH_WRITER_URL").map_err(|_| {
                    ApiConfigError::new("HOTGRAPH_WRITER_URL is required for reader nodes")
                })?;
                if writer_url.trim().is_empty() {
                    return Err(ApiConfigError::new(
                        "HOTGRAPH_WRITER_URL is required for reader nodes",
                    ));
                }
                let max_lag_lsn = std::env::var("HOTGRAPH_READER_MAX_LAG_LSN")
                    .ok()
                    .map(|value| {
                        value.parse::<u64>().map_err(|error| {
                            ApiConfigError::new(format!(
                                "HOTGRAPH_READER_MAX_LAG_LSN must be a non-negative integer: {error}"
                            ))
                        })
                    })
                    .transpose()?;
                Ok(Self::Reader {
                    writer_url,
                    max_lag_lsn,
                })
            }
            Ok(value) if value.eq_ignore_ascii_case("writer") => Ok(Self::Writer),
            Ok(value) => Err(ApiConfigError::new(format!(
                "HOTGRAPH_NODE_ROLE must be writer or reader, got {value}"
            ))),
            Err(_) => Ok(Self::Writer),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ApiState {
    log: Arc<Mutex<EventLog>>,
    storage: Arc<Mutex<InMemoryStorage>>,
    durable_log: Option<Arc<Mutex<FileEventLog>>>,
    durable_store: Option<Arc<Mutex<RedbGraphStore>>>,
    node_role: NodeRole,
    auth: Arc<AuthConfig>,
    governance: Option<Arc<Mutex<GovernanceEngine>>>,
    idempotency: Arc<Mutex<BTreeMap<String, IdempotencyRecord>>>,
    idempotency_path: Option<Arc<PathBuf>>,
    slow_query_threshold: Duration,
    max_body_bytes: usize,
    default_query_limit: usize,
    max_query_limit: usize,
    max_path_depth: usize,
    request_timeout: Duration,
    metrics: Arc<Mutex<ApiMetrics>>,
    replication_api_key: Option<Arc<String>>,
}

impl ApiState {
    pub fn new_in_memory() -> Self {
        Self {
            log: Arc::new(Mutex::new(EventLog::new(TxTime::new(0)))),
            storage: Arc::new(Mutex::new(InMemoryStorage::new())),
            durable_log: None,
            durable_store: None,
            node_role: NodeRole::Writer,
            auth: Arc::new(AuthConfig::disabled()),
            governance: None,
            idempotency: Arc::new(Mutex::new(BTreeMap::new())),
            idempotency_path: None,
            slow_query_threshold: DEFAULT_SLOW_QUERY_THRESHOLD,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            default_query_limit: DEFAULT_QUERY_LIMIT,
            max_query_limit: DEFAULT_MAX_QUERY_LIMIT,
            max_path_depth: DEFAULT_MAX_PATH_DEPTH,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            metrics: Arc::new(Mutex::new(ApiMetrics::default())),
            replication_api_key: None,
        }
    }

    pub fn from_event_log(log: EventLog) -> Self {
        let storage = InMemoryStorage::replay(log.events()).expect("event log state is replayable");
        Self {
            log: Arc::new(Mutex::new(log)),
            storage: Arc::new(Mutex::new(storage)),
            durable_log: None,
            durable_store: None,
            node_role: NodeRole::Writer,
            auth: Arc::new(AuthConfig::disabled()),
            governance: None,
            idempotency: Arc::new(Mutex::new(BTreeMap::new())),
            idempotency_path: None,
            slow_query_threshold: DEFAULT_SLOW_QUERY_THRESHOLD,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            default_query_limit: DEFAULT_QUERY_LIMIT,
            max_query_limit: DEFAULT_MAX_QUERY_LIMIT,
            max_path_depth: DEFAULT_MAX_PATH_DEPTH,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            metrics: Arc::new(Mutex::new(ApiMetrics::default())),
            replication_api_key: None,
        }
    }

    pub fn from_file_event_log(path: impl AsRef<FsPath>) -> Result<Self, StorageError> {
        let file_log = FileEventLog::open(path)?;
        let events = file_log.read_all()?;
        let storage = InMemoryStorage::replay(&events)?;
        let log = EventLog::from_events(events).map_err(StorageError::Replay)?;
        Ok(Self {
            log: Arc::new(Mutex::new(log)),
            storage: Arc::new(Mutex::new(storage)),
            durable_log: Some(Arc::new(Mutex::new(file_log))),
            durable_store: None,
            node_role: NodeRole::Writer,
            auth: Arc::new(AuthConfig::disabled()),
            governance: None,
            idempotency: Arc::new(Mutex::new(BTreeMap::new())),
            idempotency_path: None,
            slow_query_threshold: DEFAULT_SLOW_QUERY_THRESHOLD,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            default_query_limit: DEFAULT_QUERY_LIMIT,
            max_query_limit: DEFAULT_MAX_QUERY_LIMIT,
            max_path_depth: DEFAULT_MAX_PATH_DEPTH,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            metrics: Arc::new(Mutex::new(ApiMetrics::default())),
            replication_api_key: None,
        })
    }

    pub fn from_durable_event_log(path: impl AsRef<FsPath>) -> Result<Self, StorageError> {
        Self::from_file_event_log(path)
    }

    pub fn from_redb_graph_store(path: impl AsRef<FsPath>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        let store = if path.exists() {
            RedbGraphStore::open(path)?
        } else {
            RedbGraphStore::create(path)?
        };
        let storage = store.materialized_storage()?;
        let log = EventLog::from_events(storage.events().to_vec()).map_err(StorageError::Replay)?;
        Ok(Self {
            log: Arc::new(Mutex::new(log)),
            storage: Arc::new(Mutex::new(storage)),
            durable_log: None,
            durable_store: Some(Arc::new(Mutex::new(store))),
            node_role: NodeRole::Writer,
            auth: Arc::new(AuthConfig::disabled()),
            governance: None,
            idempotency: Arc::new(Mutex::new(BTreeMap::new())),
            idempotency_path: None,
            slow_query_threshold: DEFAULT_SLOW_QUERY_THRESHOLD,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            default_query_limit: DEFAULT_QUERY_LIMIT,
            max_query_limit: DEFAULT_MAX_QUERY_LIMIT,
            max_path_depth: DEFAULT_MAX_PATH_DEPTH,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            metrics: Arc::new(Mutex::new(ApiMetrics::default())),
            replication_api_key: None,
        })
    }

    pub fn from_env() -> Result<Self, ApiConfigError> {
        let mut state = if let Some(path) = std::env::var_os("RG_REDB_PATH") {
            Self::from_redb_graph_store(PathBuf::from(path)).map_err(ApiConfigError::from)?
        } else if let Some(path) = std::env::var_os("RG_EVENT_LOG_PATH") {
            Self::from_durable_event_log(PathBuf::from(path)).map_err(ApiConfigError::from)?
        } else {
            Self::new_in_memory()
        };
        state.node_role = NodeRole::from_env()?;
        if state.durable_log.is_none()
            && state.durable_store.is_none()
            && configured_replica_count()? > 1
        {
            return Err(ApiConfigError::new(
                "multiple API replicas require RG_REDB_PATH or RG_EVENT_LOG_PATH durable storage",
            ));
        }
        if let Some(path) = std::env::var_os("RG_IDEMPOTENCY_LOG_PATH") {
            state = state
                .with_idempotency_path(PathBuf::from(path))
                .map_err(ApiConfigError::from)?;
        }
        if let Ok(api_key) = std::env::var("HOTGRAPH_REPLICATION_API_KEY") {
            state = state.with_replication_api_key(api_key);
        }

        match std::env::var("RG_API_KEYS") {
            Ok(value) => {
                let auth = AuthConfig::from_env_value(&value).map_err(ApiConfigError::from)?;
                if auth.is_enabled() {
                    Ok(state.with_auth(auth))
                } else if dev_auth_disabled() {
                    Ok(state)
                } else {
                    Err(ApiConfigError::new(
                        "auth is required: RG_API_KEYS did not contain any API keys",
                    ))
                }
            }
            Err(_) if dev_auth_disabled() => Ok(state),
            Err(_) => Err(ApiConfigError::new(
                "auth is required: set RG_API_KEYS or HOTGRAPH_DEV_AUTH_DISABLED=true for local development",
            )),
        }
    }

    pub fn with_auth(mut self, auth: AuthConfig) -> Self {
        self.auth = Arc::new(auth);
        self
    }

    pub fn with_governance(mut self, governance: GovernanceEngine) -> Self {
        self.governance = Some(Arc::new(Mutex::new(governance)));
        self
    }

    pub fn with_reader_role(
        mut self,
        writer_url: impl Into<String>,
        max_lag_lsn: Option<u64>,
    ) -> Self {
        self.node_role = NodeRole::Reader {
            writer_url: writer_url.into(),
            max_lag_lsn,
        };
        self
    }

    pub fn with_replication_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.replication_api_key = Some(Arc::new(api_key.into()));
        self
    }

    fn is_reader(&self) -> bool {
        matches!(self.node_role, NodeRole::Reader { .. })
    }

    async fn proxy_json_to_writer<T, R>(
        &self,
        endpoint: &str,
        headers: &HeaderMap,
        body: &T,
    ) -> Result<R, ApiError>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        let NodeRole::Reader { writer_url, .. } = &self.node_role else {
            return Err(ApiError::internal("write proxy called on writer node"));
        };
        let api_key = headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok());
        let idempotency_key = headers
            .get("idempotency-key")
            .and_then(|value| value.to_str().ok());
        simple_http_json(
            "POST",
            &format!("{}{}", writer_url.trim_end_matches('/'), endpoint),
            api_key,
            idempotency_key,
            Some(body),
        )
        .await
    }

    pub fn with_idempotency_path(mut self, path: impl AsRef<FsPath>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        let events = self
            .log
            .lock()
            .map_err(|_| StorageError::Io("event log lock poisoned".to_owned()))?
            .events()
            .to_vec();
        let idempotency = read_idempotency_records(&path, &events)?;
        self.idempotency = Arc::new(Mutex::new(idempotency));
        self.idempotency_path = Some(Arc::new(path));
        Ok(self)
    }

    pub fn with_slow_query_threshold(mut self, threshold: Duration) -> Self {
        self.slow_query_threshold = threshold;
        self
    }

    pub fn with_max_body_bytes(mut self, max_body_bytes: usize) -> Self {
        self.max_body_bytes = max_body_bytes;
        self
    }

    pub fn with_default_query_limit(mut self, default_query_limit: usize) -> Self {
        self.default_query_limit = default_query_limit;
        self
    }

    pub fn with_max_query_limit(mut self, max_query_limit: usize) -> Self {
        self.max_query_limit = max_query_limit;
        self
    }

    pub fn with_max_path_depth(mut self, max_path_depth: usize) -> Self {
        self.max_path_depth = max_path_depth;
        self
    }

    pub fn with_request_timeout(mut self, request_timeout: Duration) -> Self {
        self.request_timeout = request_timeout;
        self
    }

    fn execute(
        &self,
        command: GraphCommand,
        idempotency_key: Option<String>,
    ) -> Result<rg_events::GraphEvent, ApiError> {
        if let NodeRole::Reader { writer_url, .. } = &self.node_role {
            return Err(ApiError::writer_required(format!(
                "reader nodes do not acknowledge local writes; send the request to {writer_url}"
            )));
        }
        let start = Instant::now();
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
            if let Some(durable_store) = &self.durable_store {
                let store = durable_store
                    .lock()
                    .map_err(|_| ApiError::internal("durable graph store lock poisoned"))?;
                if let Some(lsn) = store.idempotency_lsn(key).map_err(ApiError::from)? {
                    let existing = store
                        .events_by_lsn(lsn, lsn)
                        .map_err(ApiError::from)?
                        .into_iter()
                        .next()
                        .map(|(_, event)| event)
                        .ok_or_else(|| ApiError::internal("idempotent event LSN was not found"))?;
                    return Ok(existing);
                }
            }
        }

        let mut log = self
            .log
            .lock()
            .map_err(|_| ApiError::internal("event log lock poisoned"))?;
        let mut candidate = log.clone();
        let event = candidate.execute(command).map_err(ApiError::from)?;
        if let Some(durable_store) = &self.durable_store {
            let append = durable_store
                .lock()
                .map_err(|_| ApiError::internal("durable graph store lock poisoned"))?
                .append_event(&event, idempotency_key.as_deref())
                .map_err(ApiError::from)?;
            if append.idempotency_replayed {
                let existing = durable_store
                    .lock()
                    .map_err(|_| ApiError::internal("durable graph store lock poisoned"))?
                    .events_by_lsn(append.lsn, append.lsn)
                    .map_err(ApiError::from)?
                    .into_iter()
                    .next()
                    .map(|(_, event)| event)
                    .ok_or_else(|| ApiError::internal("idempotent event LSN was not found"))?;
                return Ok(existing);
            }
        } else if let Some(durable_log) = &self.durable_log {
            let metadata = idempotency_key
                .as_deref()
                .map(|key| WalAppendMetadata::new().with_idempotency_key(key))
                .unwrap_or_default();
            durable_log
                .lock()
                .map_err(|_| ApiError::internal("durable event log lock poisoned"))?
                .append_with_metadata(&event, metadata)
                .map_err(ApiError::from)?;
        }
        self.storage
            .lock()
            .map_err(|_| ApiError::internal("storage lock poisoned"))?
            .append_event(event.clone())
            .map_err(ApiError::from)?;
        *log = candidate;
        drop(log);

        if let Some(key) = idempotency_key {
            let record = IdempotencyRecord {
                fingerprint,
                event: event.clone(),
            };
            if let Some(path) = &self.idempotency_path {
                append_idempotency_record(path, &key, &record).map_err(ApiError::from)?;
            }
            self.idempotency
                .lock()
                .map_err(|_| ApiError::internal("idempotency lock poisoned"))?
                .insert(key, record);
        }
        self.record_operation("write", start.elapsed());
        Ok(event)
    }

    fn apply_query_limits(
        &self,
        mut request: GraphQueryRequest,
    ) -> Result<GraphQueryRequest, ApiError> {
        let limit = self.normalize_query_limit(request.limit)?;
        request.limit = Some(limit);
        Ok(request)
    }

    fn normalize_query_limit(&self, limit: Option<usize>) -> Result<usize, ApiError> {
        let limit = limit.unwrap_or(self.default_query_limit);
        if limit > self.max_query_limit {
            return Err(ApiError::bad_request(format!(
                "query limit {limit} exceeds configured max {}",
                self.max_query_limit
            )));
        }
        Ok(limit)
    }

    fn validate_path_depth(&self, request: &PathQueryRequest) -> Result<(), ApiError> {
        if request.max_depth > self.max_path_depth {
            Err(ApiError::bad_request(format!(
                "path max_depth {} exceeds configured max {}",
                request.max_depth, self.max_path_depth
            )))
        } else {
            Ok(())
        }
    }

    fn storage_snapshot(&self) -> Result<InMemoryStorage, ApiError> {
        if let Some(durable_store) = &self.durable_store {
            return durable_store
                .lock()
                .map_err(|_| ApiError::internal("durable graph store lock poisoned"))?
                .materialized_storage()
                .map_err(ApiError::from);
        }
        self.storage
            .lock()
            .map_err(|_| ApiError::internal("storage lock poisoned"))
            .map(|storage| storage.clone())
    }

    fn metrics_snapshot(&self) -> Result<MetricsResponse, ApiError> {
        if let Some(durable_store) = &self.durable_store {
            let counts = durable_store
                .lock()
                .map_err(|_| ApiError::internal("durable graph store lock poisoned"))?
                .counts()
                .map_err(ApiError::from)?;
            return Ok(MetricsResponse {
                entities: counts.entities,
                assertions: counts.assertions,
                sources: counts.sources,
                events: counts.events,
                agent_memories: counts.memories,
            });
        }
        let storage = self.storage_snapshot()?;
        Ok(MetricsResponse {
            entities: storage.graph_state().entities.len(),
            assertions: storage.graph_state().assertions.len(),
            sources: storage.graph_state().sources.len(),
            events: storage.events().len(),
            agent_memories: storage.graph_state().agent_memories.len(),
        })
    }

    fn execute_graph_query(&self, query: GraphQuery) -> Result<Vec<QueryResult>, ApiError> {
        if let Some(durable_store) = &self.durable_store {
            let query = durable_query_from_graph_query(&query);
            let assertions = durable_store
                .lock()
                .map_err(|_| ApiError::internal("durable graph store lock poisoned"))?
                .query_assertions(&query)
                .map_err(ApiError::from)?;
            return Ok(assertions.iter().map(QueryResult::from_assertion).collect());
        }
        let storage = self.storage_snapshot()?;
        Ok(QueryEngine::from_storage(storage).execute_graph(query))
    }

    fn governance_principal(&self, context: &RequestContext) -> Option<Principal> {
        context.principal.as_ref().map(|principal| Principal {
            id: PrincipalId::new(principal.service_account_id.clone()),
            tenant_id: principal.tenant_id.clone(),
            agent_id: None,
        })
    }

    fn source_access_denial(
        &self,
        context: &RequestContext,
        source_id: &SourceId,
    ) -> Result<Option<AccessDenial>, ApiError> {
        let Some(governance) = &self.governance else {
            return Ok(None);
        };
        let Some(principal) = self.governance_principal(context) else {
            return Ok(None);
        };
        Ok(governance
            .lock()
            .map_err(|_| ApiError::internal("governance lock poisoned"))?
            .check_source_access(&principal, source_id))
    }

    fn governance_allows_sources(
        &self,
        context: &RequestContext,
        source_ids: &[SourceId],
    ) -> Result<bool, ApiError> {
        for source_id in source_ids {
            if self.source_access_denial(context, source_id)?.is_some() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn filter_query_results_for_governance(
        &self,
        context: &RequestContext,
        mut results: Vec<QueryResult>,
    ) -> Result<Vec<QueryResult>, ApiError> {
        if self.governance.is_none() {
            return Ok(results);
        }
        let mut allowed = Vec::new();
        for result in results.drain(..) {
            if self.governance_allows_sources(context, &result.source_ids)? {
                allowed.push(result);
            }
        }
        Ok(allowed)
    }

    fn health_snapshot(&self) -> Result<HealthResponse, ApiError> {
        let metrics = self.metrics_snapshot()?;
        if let Some(durable_store) = &self.durable_store {
            durable_store
                .lock()
                .map_err(|_| ApiError::internal("durable graph store lock poisoned"))?
                .health()
                .map_err(ApiError::from)?;
        }
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
        let latency = self
            .metrics
            .lock()
            .map_err(|_| ApiError::internal("metrics lock poisoned"))?
            .prometheus_histograms();
        let durable = self.durable_prometheus_metrics()?;
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
             rg_graph_index_health 1\n\
             {latency}\
             {durable}",
            metrics.events,
            metrics.entities,
            metrics.assertions,
            metrics.sources,
            metrics.agent_memories
        ))
    }

    fn durable_prometheus_metrics(&self) -> Result<String, ApiError> {
        let Some(durable_store) = &self.durable_store else {
            return Ok(String::new());
        };
        let health = durable_store
            .lock()
            .map_err(|_| ApiError::internal("durable graph store lock poisoned"))?
            .health()
            .map_err(ApiError::from)?;
        let lease_active = usize::from(health.writer_lease.is_some());
        Ok(format!(
            "# HELP rg_storage_last_lsn Last committed durable storage LSN.\n\
             # TYPE rg_storage_last_lsn gauge\n\
             rg_storage_last_lsn {}\n\
             # HELP rg_storage_applied_lsn Last locally applied durable storage LSN.\n\
             # TYPE rg_storage_applied_lsn gauge\n\
             rg_storage_applied_lsn {}\n\
             # HELP rg_replication_lag_lsn Durable follower replay lag in LSNs.\n\
             # TYPE rg_replication_lag_lsn gauge\n\
             rg_replication_lag_lsn {}\n\
             # HELP rg_writer_lease_active Writer lease active flag, where 1 means present.\n\
             # TYPE rg_writer_lease_active gauge\n\
             rg_writer_lease_active {}\n",
            health.last_lsn, health.applied_lsn, health.replay_lag, lease_active
        ))
    }

    fn record_operation(&self, operation: &'static str, duration: Duration) {
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.record(operation, duration);
        }
    }

    fn generated_tx(&self) -> Result<TxTime, ApiError> {
        let log = self
            .log
            .lock()
            .map_err(|_| ApiError::internal("event log lock poisoned"))?;
        Ok(TxTime::new(log.events().len() as i64))
    }

    fn replication_events_after(
        &self,
        after_lsn: u64,
        limit: usize,
    ) -> Result<ReplicationBatch, ApiError> {
        let Some(durable_store) = &self.durable_store else {
            return Err(ApiError::bad_request(
                "replication events require RG_REDB_PATH-backed storage",
            ));
        };
        durable_store
            .lock()
            .map_err(|_| ApiError::internal("durable graph store lock poisoned"))?
            .replication_batch_after(after_lsn, limit)
            .map_err(ApiError::from)
    }

    fn apply_replication_batch(
        &self,
        batch: &ReplicationBatch,
        max_lag_lsn: Option<u64>,
    ) -> Result<ReplicationStatusResponse, ApiError> {
        let Some(durable_store) = &self.durable_store else {
            return Err(ApiError::bad_request(
                "replication apply requires RG_REDB_PATH-backed storage",
            ));
        };
        let status = durable_store
            .lock()
            .map_err(|_| ApiError::internal("durable graph store lock poisoned"))?
            .apply_replication_batch(batch, max_lag_lsn)
            .map_err(ApiError::from)?;
        Ok(ReplicationStatusResponse::from(status))
    }

    async fn catch_up_from_writer(&self) -> Result<ReplicationStatusResponse, ApiError> {
        let NodeRole::Reader {
            writer_url,
            max_lag_lsn,
        } = &self.node_role
        else {
            return Err(ApiError::bad_request(
                "replication catch-up is only valid on reader nodes",
            ));
        };
        let Some(durable_store) = &self.durable_store else {
            return Err(ApiError::bad_request(
                "replication catch-up requires RG_REDB_PATH-backed storage",
            ));
        };
        let after_lsn = durable_store
            .lock()
            .map_err(|_| ApiError::internal("durable graph store lock poisoned"))?
            .health()
            .map_err(ApiError::from)?
            .applied_lsn;
        let api_key = self.replication_api_key.as_deref().map(String::as_str);
        let url = format!(
            "{}/v1/admin/replication/events?after_lsn={after_lsn}&limit=1000",
            writer_url.trim_end_matches('/')
        );
        let batch: ReplicationBatch =
            fetch_replication_batch_with_retries(&url, api_key, 3).await?;
        self.apply_replication_batch(&batch, *max_lag_lsn)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct IdempotencyRecord {
    fingerprint: String,
    event: rg_events::GraphEvent,
}

#[derive(Clone, Debug, Default)]
struct ApiMetrics {
    durations: BTreeMap<&'static str, Vec<f64>>,
}

impl ApiMetrics {
    fn record(&mut self, operation: &'static str, duration: Duration) {
        self.durations
            .entry(operation)
            .or_default()
            .push(duration.as_secs_f64());
    }

    fn prometheus_histograms(&self) -> String {
        let mut output = String::from(
            "# HELP rg_api_request_duration_seconds API request duration by operation.\n\
             # TYPE rg_api_request_duration_seconds histogram\n",
        );
        for operation in ["write", "query", "path", "evidence_pack", "ai_context_pack"] {
            let values = self
                .durations
                .get(operation)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            for bucket in [0.05_f64, 0.1, 0.5, 1.0, 5.0] {
                let count = values.iter().filter(|value| **value <= bucket).count();
                output.push_str(&format!(
                    "rg_api_request_duration_seconds_bucket{{operation=\"{operation}\",le=\"{bucket}\"}} {count}\n"
                ));
            }
            output.push_str(&format!(
                "rg_api_request_duration_seconds_bucket{{operation=\"{operation}\",le=\"+Inf\"}} {}\n",
                values.len()
            ));
            output.push_str(&format!(
                "rg_api_request_duration_seconds_sum{{operation=\"{operation}\"}} {}\n",
                values.iter().sum::<f64>()
            ));
            output.push_str(&format!(
                "rg_api_request_duration_seconds_count{{operation=\"{operation}\"}} {}\n",
                values.len()
            ));
        }
        output
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistedIdempotencyRecord {
    key: String,
    fingerprint: String,
    event_id: String,
}

fn read_idempotency_records(
    path: &FsPath,
    events: &[rg_events::GraphEvent],
) -> Result<BTreeMap<String, IdempotencyRecord>, StorageError> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| StorageError::Io(error.to_string()))?;
    let events_by_id = events
        .iter()
        .map(|event| (event.event_id().as_str().to_owned(), event.clone()))
        .collect::<BTreeMap<_, _>>();
    let file = File::open(path).map_err(|error| StorageError::Io(error.to_string()))?;
    let reader = BufReader::new(file);
    let mut records = BTreeMap::new();
    for line in reader.lines() {
        let line = line.map_err(|error| StorageError::Io(error.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        let persisted = serde_json::from_str::<PersistedIdempotencyRecord>(&line)
            .map_err(|error| StorageError::Codec(error.to_string()))?;
        let event = events_by_id.get(&persisted.event_id).ok_or_else(|| {
            StorageError::Codec(format!(
                "idempotency record references missing event {}",
                persisted.event_id
            ))
        })?;
        records.insert(
            persisted.key,
            IdempotencyRecord {
                fingerprint: persisted.fingerprint,
                event: event.clone(),
            },
        );
    }
    Ok(records)
}

fn append_idempotency_record(
    path: &FsPath,
    key: &str,
    record: &IdempotencyRecord,
) -> Result<(), StorageError> {
    let persisted = PersistedIdempotencyRecord {
        key: key.to_owned(),
        fingerprint: record.fingerprint.clone(),
        event_id: record.event.event_id().as_str().to_owned(),
    };
    let line = serde_json::to_string(&persisted)
        .map_err(|error| StorageError::Codec(error.to_string()))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| StorageError::Io(error.to_string()))?;
    writeln!(file, "{line}").map_err(|error| StorageError::Io(error.to_string()))?;
    file.sync_data()
        .map_err(|error| StorageError::Io(error.to_string()))
}

async fn simple_http_json<T, R>(
    method: &str,
    url: &str,
    api_key: Option<&str>,
    idempotency_key: Option<&str>,
    body: Option<&T>,
) -> Result<R, ApiError>
where
    T: Serialize,
    R: DeserializeOwned,
{
    let (host, path) = parse_http_url(url)?;
    let serialized_body = body
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| ApiError::internal(format!("serialize proxy request: {error}")))?;
    let body_text = serialized_body.as_deref().unwrap_or("");
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept: application/json\r\n"
    );
    if let Some(api_key) = api_key {
        request.push_str(&format!("x-api-key: {api_key}\r\n"));
    }
    if let Some(idempotency_key) = idempotency_key {
        request.push_str(&format!("idempotency-key: {idempotency_key}\r\n"));
    }
    if body.is_some() {
        request.push_str("content-type: application/json\r\n");
        request.push_str(&format!("content-length: {}\r\n", body_text.len()));
    }
    request.push_str("\r\n");
    request.push_str(body_text);

    let mut stream = TcpStream::connect(host)
        .await
        .map_err(|error| ApiError::writer_required(format!("connect writer: {error}")))?;
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| ApiError::writer_required(format!("write writer request: {error}")))?;
    let response = read_http_response(&mut stream)
        .await
        .map_err(|error| ApiError::writer_required(format!("read writer response: {error}")))?;
    let (head, response_body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| ApiError::writer_required("writer returned malformed HTTP response"))?;
    if !head.starts_with("HTTP/1.1 2") && !head.starts_with("HTTP/1.0 2") {
        if let Ok(error) = serde_json::from_str::<ErrorResponse>(response_body) {
            return Err(ApiError {
                status: StatusCode::BAD_GATEWAY,
                code: "writer_proxy_failed",
                message: format!("writer rejected proxied request: {}", error.error),
            });
        }
        return Err(ApiError::writer_required(format!(
            "writer returned non-success response: {head}"
        )));
    }
    serde_json::from_str(response_body)
        .map_err(|error| ApiError::internal(format!("decode writer response: {error}")))
}

async fn fetch_replication_batch_with_retries(
    url: &str,
    api_key: Option<&str>,
    attempts: usize,
) -> Result<ReplicationBatch, ApiError> {
    let attempts = attempts.max(1);
    let mut last_error = None;
    for attempt in 1..=attempts {
        match simple_http_json::<(), ReplicationBatch>("GET", url, api_key, None, None).await {
            Ok(batch) => return Ok(batch),
            Err(error) if error.code == "writer_required" && attempt < attempts => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| ApiError::writer_required("writer replication unavailable")))
}

fn parse_http_url(url: &str) -> Result<(&str, String), ApiError> {
    let without_scheme = url
        .strip_prefix("http://")
        .ok_or_else(|| ApiError::bad_request("only http:// writer URLs are supported"))?;
    let (host, path) = without_scheme
        .split_once('/')
        .map(|(host, path)| (host, format!("/{path}")))
        .unwrap_or((without_scheme, "/".to_owned()));
    if host.trim().is_empty() {
        return Err(ApiError::bad_request("writer URL host must not be empty"));
    }
    Ok((host, path))
}

async fn read_http_response(stream: &mut TcpStream) -> Result<String, std::io::Error> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
            Err(_) if !bytes.is_empty() => break,
            Err(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "timed out waiting for HTTP response",
                ))
            }
            Ok(Ok(0)) => break,
            Ok(Ok(read)) => {
                bytes.extend_from_slice(&buffer[..read]);
                if response_has_complete_body(&bytes) {
                    break;
                }
            }
            Ok(Err(error)) => return Err(error),
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn response_has_complete_body(bytes: &[u8]) -> bool {
    let Some(header_end) = find_header_end(bytes) else {
        return false;
    };
    let header = String::from_utf8_lossy(&bytes[..header_end]);
    let body_len = bytes.len().saturating_sub(header_end + 4);
    for line in header.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            return value
                .trim()
                .parse::<usize>()
                .is_ok_and(|expected| body_len >= expected);
        }
        if name.trim().eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
        {
            return bytes.ends_with(b"\r\n0\r\n\r\n") || bytes.ends_with(b"0\r\n\r\n");
        }
    }
    false
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
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

fn hash_api_key(api_key: &str) -> String {
    let digest = Sha256::digest(api_key.as_bytes());
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[derive(Clone, Eq, PartialEq)]
pub struct ServiceAccount {
    api_key_hash: String,
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
        let api_key = api_key.into();
        let mut roles = roles;
        roles.sort_by_key(role_rank);
        roles.dedup();
        Self {
            api_key_hash: hash_api_key(&api_key),
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
            .field("api_key_hash", &self.api_key_hash)
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
        self.roles
            .iter()
            .any(|role| role_rank(role) >= role_rank(&required))
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
            .map(|account| (account.api_key_hash.clone(), account))
            .collect();
        Self { accounts_by_key }
    }

    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn from_api_keys_env(value: &str) -> Result<Self, AuthConfigError> {
        let mut accounts = Vec::new();
        for (index, entry) in value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .enumerate()
        {
            let parts = entry.split(':').map(str::trim).collect::<Vec<_>>();
            let account =
                match parts.as_slice() {
                    [api_key] => ServiceAccount::new(
                        required_auth_part(api_key, "api key")?,
                        format!("env-service-account-{}", index + 1),
                        "default",
                        vec![ApiRole::Writer],
                    ),
                    [api_key, service_account_id, tenant_id, roles] => ServiceAccount::new(
                        required_auth_part(api_key, "api key")?,
                        required_auth_part(service_account_id, "service account id")?,
                        required_auth_part(tenant_id, "tenant id")?,
                        parse_roles(roles)?,
                    ),
                    _ => return Err(AuthConfigError::new(
                        "RG_API_KEYS entries must be `key` or `key:service_account:tenant:roles`",
                    )),
                };
            if accounts
                .iter()
                .any(|existing: &ServiceAccount| existing.api_key_hash == account.api_key_hash)
            {
                return Err(AuthConfigError::new("duplicate API key in RG_API_KEYS"));
            }
            accounts.push(account);
        }
        Ok(Self::new(accounts))
    }

    pub fn from_env_value(value: &str) -> Result<Self, AuthConfigError> {
        Self::from_api_keys_env(value)
    }

    pub fn is_enabled(&self) -> bool {
        !self.accounts_by_key.is_empty()
    }

    pub fn authenticate(&self, api_key: &str) -> Option<ApiPrincipal> {
        self.accounts_by_key
            .get(&hash_api_key(api_key))
            .map(|account| ApiPrincipal {
                service_account_id: account.service_account_id.clone(),
                tenant_id: account.tenant_id.clone(),
                roles: account.roles.clone(),
            })
    }

    pub fn debug_key_material(&self) -> Vec<String> {
        self.accounts_by_key
            .values()
            .map(|account| {
                format!(
                    "{}:{}:{:?}",
                    account.service_account_id, account.tenant_id, account.roles
                )
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct ApiConfigError {
    message: String,
}

impl ApiConfigError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ApiConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApiConfigError {}

impl From<AuthConfigError> for ApiConfigError {
    fn from(error: AuthConfigError) -> Self {
        Self::new(error.to_string())
    }
}

impl From<StorageError> for ApiConfigError {
    fn from(error: StorageError) -> Self {
        Self::new(format!("{error:?}"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthConfigError {
    message: String,
}

impl AuthConfigError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AuthConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AuthConfigError {}

fn required_auth_part<'a>(value: &'a str, field: &str) -> Result<&'a str, AuthConfigError> {
    if value.is_empty() {
        Err(AuthConfigError::new(format!(
            "RG_API_KEYS {field} must not be empty"
        )))
    } else {
        Ok(value)
    }
}

fn parse_roles(value: &str) -> Result<Vec<ApiRole>, AuthConfigError> {
    let roles = value
        .split(['+', '|'])
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(|role| match role.to_ascii_lowercase().as_str() {
            "reader" => Ok(ApiRole::Reader),
            "writer" => Ok(ApiRole::Writer),
            "admin" => Ok(ApiRole::Admin),
            other => Err(AuthConfigError::new(format!(
                "unknown RG_API_KEYS role `{other}`"
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if roles.is_empty() {
        Err(AuthConfigError::new(
            "RG_API_KEYS roles must include reader, writer, or admin",
        ))
    } else {
        Ok(roles)
    }
}

fn dev_auth_disabled() -> bool {
    std::env::var("HOTGRAPH_DEV_AUTH_DISABLED")
        .or_else(|_| std::env::var("RG_DEV_AUTH_DISABLED"))
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

fn configured_replica_count() -> Result<usize, ApiConfigError> {
    match std::env::var("HOTGRAPH_REPLICA_COUNT").or_else(|_| std::env::var("RG_REPLICA_COUNT")) {
        Ok(value) => value.trim().parse::<usize>().map_err(|_| {
            ApiConfigError::new(
                "HOTGRAPH_REPLICA_COUNT/RG_REPLICA_COUNT must be a positive integer",
            )
        }),
        Err(_) => Ok(1),
    }
}

pub fn router(state: ApiState) -> Router {
    let auth_state = state.clone();
    let timeout_state = state.clone();
    let max_body_bytes = state.max_body_bytes;
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
        .route("/v1/admin/replication/events", get(replication_events))
        .route("/v1/admin/replication/apply", post(replication_apply))
        .route("/v1/admin/replication/catch-up", post(replication_catch_up))
        .layer(middleware::from_fn_with_state(auth_state, auth_middleware))
        .layer(middleware::from_fn_with_state(
            timeout_state,
            timeout_middleware,
        ))
        .layer(DefaultBodyLimit::max(max_body_bytes))
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

async fn timeout_middleware(
    State(state): State<ApiState>,
    request: Request,
    next: Next,
) -> Response {
    match tokio::time::timeout(state.request_timeout, next.run(request)).await {
        Ok(response) => response,
        Err(_) => ApiError::request_timeout("request exceeded configured deadline").into_response(),
    }
}

fn required_role(method: &Method, path: &str) -> Option<ApiRole> {
    if matches!(path, "/v1/health" | "/v1/openapi.json") {
        return None;
    }
    if path.starts_with("/v1/admin/") {
        return Some(ApiRole::Admin);
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
    if state.is_reader() {
        return Ok(Json(
            state
                .proxy_json_to_writer("/v1/entities", &headers, &request)
                .await?,
        ));
    }
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
    if state.is_reader() {
        return Ok(Json(
            state
                .proxy_json_to_writer("/v1/sources", &headers, &request)
                .await?,
        ));
    }
    let source_id = source_id_from_request(&request);
    let command = add_source_command(request, source_id.clone())?;
    let event = state.execute(GraphCommand::AddSource(command), idempotency_key(&headers)?)?;
    let storage = state.storage_snapshot()?;
    let source = storage
        .source(&source_id)
        .ok_or_else(|| ApiError::internal("source was not materialized"))?;
    let mut response = SourceResponse::from(source);
    response.event_type = Some(event_type_name(&event).to_owned());
    Ok(Json(response))
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
    if state.is_reader() {
        return Ok(Json(
            state
                .proxy_json_to_writer("/v1/assertions", &headers, &request)
                .await?,
        ));
    }
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
    if state.is_reader() {
        return Ok(Json(
            state
                .proxy_json_to_writer("/v1/events", &headers, &request)
                .await?,
        ));
    }
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
    let results = state
        .filter_query_results_for_governance(
            &context,
            state.execute_graph_query(graph_query_from_request(
                state.apply_query_limits(request)?,
                context.tenant_context(),
            )?)?,
        )?
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
    state.validate_path_depth(&request)?;
    let storage = state.storage_snapshot()?;
    let engine = QueryEngine::from_storage(storage);
    let tenant_context = context.tenant_context();
    let mut paths = engine.execute_path(request.try_into()?);
    retain_paths_for_tenant(&mut paths, tenant_context.as_ref());
    if state.governance.is_some() {
        paths.retain(|path| {
            path.hops.iter().all(|hop| {
                state
                    .governance_allows_sources(&context, &hop.source_ids)
                    .unwrap_or(false)
            })
        });
    }
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
        graph_query: graph_query_from_request(
            state.apply_query_limits(request.graph_query)?,
            tenant_context.clone(),
        )?,
        path_query: request
            .path_query
            .map(|request| {
                state.validate_path_depth(&request)?;
                request.try_into()
            })
            .transpose()?,
        generated_at,
    });
    let governed_pack = if let (Some(governance), Some(principal)) =
        (&state.governance, state.governance_principal(&context))
    {
        governance
            .lock()
            .map_err(|_| ApiError::internal("governance lock poisoned"))?
            .enforce_evidence_pack_mut(principal, &pack, AuditReason::AiContextPack)
            .pack
    } else {
        pack
    };
    let mut response = EvidencePackResponse::from(&governed_pack);
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
    let limit = state.normalize_query_limit(intent.limit)?;
    let graph_query = GraphQuery {
        subject: intent.subject.clone().map(EntityPattern::Id),
        predicate: intent.predicate.clone().map(PredicatePattern::Id),
        object: None,
        valid_at: intent.valid_at,
        known_at: intent.known_at,
        context: intent.context,
        min_confidence: intent.min_confidence,
        limit: Some(limit),
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
    let governed_pack = if let (Some(governance), Some(principal)) =
        (&state.governance, state.governance_principal(&context))
    {
        governance
            .lock()
            .map_err(|_| ApiError::internal("governance lock poisoned"))?
            .enforce_evidence_pack_mut(principal, &pack, AuditReason::AiContextPack)
            .pack
    } else {
        pack
    };
    let mut response = EvidencePackResponse::from(&governed_pack);
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
            && state
                .governance_allows_sources(&context, &assertion.source_ids)
                .unwrap_or(false)
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
    if !state.governance_allows_sources(&context, &assertion.source_ids)? {
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
    Extension(context): Extension<RequestContext>,
    Path(id): Path<String>,
) -> Result<Json<SourceResponse>, ApiError> {
    let storage = state.storage_snapshot()?;
    let source_id = SourceId::new(id.clone());
    if state.source_access_denial(&context, &source_id)?.is_some() {
        return Err(ApiError::not_found(format!("source not found: {id}")));
    }
    let source = storage
        .source(&source_id)
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

async fn replication_events(
    State(state): State<ApiState>,
    Query(query): Query<ReplicationEventsQuery>,
) -> Result<Json<ReplicationBatch>, ApiError> {
    Ok(Json(state.replication_events_after(
        query.after_lsn.unwrap_or(0),
        query.limit.unwrap_or(1000),
    )?))
}

async fn replication_apply(
    State(state): State<ApiState>,
    Json(batch): Json<ReplicationBatch>,
) -> Result<Json<ReplicationStatusResponse>, ApiError> {
    Ok(Json(state.apply_replication_batch(&batch, None)?))
}

async fn replication_catch_up(
    State(state): State<ApiState>,
) -> Result<Json<ReplicationStatusResponse>, ApiError> {
    Ok(Json(state.catch_up_from_writer().await?))
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

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct ReplicationEventsQuery {
    pub after_lsn: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct ReplicationStatusResponse {
    pub leader_last_lsn: u64,
    pub follower_applied_lsn: u64,
    pub replay_lag: u64,
}

impl From<FollowerStatus> for ReplicationStatusResponse {
    fn from(status: FollowerStatus) -> Self {
        Self {
            leader_last_lsn: status.leader_last_lsn,
            follower_applied_lsn: status.follower_applied_lsn,
            replay_lag: status.replay_lag,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct EntityResponse {
    pub id: String,
    pub entity_type: String,
    pub canonical_name: Option<String>,
    pub created_tx: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct SourceResponse {
    pub id: String,
    pub source_type: String,
    pub uri: Option<String>,
    pub content_hash: String,
    pub observed_at: i64,
    pub trust_score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
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

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct GraphValueResponse {
    pub entity_id: Option<String>,
    pub text: Option<String>,
    pub integer: Option<i64>,
    pub decimal: Option<f64>,
    pub boolean: Option<bool>,
    pub time: Option<i64>,
    pub null: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
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

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct QueryResponse {
    pub results: Vec<QueryResultResponse>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct PathResultResponse {
    pub start: String,
    pub end: String,
    pub hops: Vec<QueryResultResponse>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct PathResponse {
    pub paths: Vec<PathResultResponse>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct SourceExcerptResponse {
    pub source_id: String,
    pub source_type: String,
    pub uri: Option<String>,
    pub content_hash: String,
    pub snippet: String,
    pub trust_score: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct ContradictionResponse {
    pub id: String,
    pub assertion_a: String,
    pub assertion_b: String,
    pub contradiction_type: String,
    pub severity: String,
    pub explanation: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct EvidencePackResponse {
    pub query: String,
    pub entities: Vec<EntityResponse>,
    pub assertions: Vec<AssertionResponse>,
    pub sources: Vec<SourceExcerptResponse>,
    pub paths: Vec<PathResultResponse>,
    pub contradictions: Vec<ContradictionResponse>,
    pub generated_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
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

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct IngestDocumentResponse {
    pub document_id: String,
    pub candidates: Vec<CandidateAssertionResponse>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct EntityStateResponse {
    pub entity: EntityResponse,
    pub assertions: Vec<AssertionResponse>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct EventResponse {
    pub event_id: String,
    pub transaction_time: i64,
    pub event_type: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub event_log: String,
    pub index_health: IndexHealthResponse,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct IndexHealthResponse {
    pub status: String,
    pub entities: usize,
    pub assertions: usize,
    pub sources: usize,
    pub events: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct MetricsResponse {
    pub entities: usize,
    pub assertions: usize,
    pub sources: usize,
    pub events: usize,
    pub agent_memories: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub code: String,
    pub error: String,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: message.into(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "forbidden",
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "conflict",
            message: message.into(),
        }
    }

    fn request_timeout(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::REQUEST_TIMEOUT,
            code: "request_timeout",
            message: message.into(),
        }
    }

    fn writer_required(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "writer_required",
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                code: self.code.to_owned(),
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

fn durable_query_from_graph_query(query: &GraphQuery) -> DurableAssertionQuery {
    DurableAssertionQuery {
        subject: query.subject.as_ref().map(|pattern| match pattern {
            EntityPattern::Id(id) => id.clone(),
        }),
        predicate: query.predicate.as_ref().map(|pattern| match pattern {
            PredicatePattern::Id(id) => id.clone(),
        }),
        object: query.object.as_ref().map(|pattern| match pattern {
            ObjectPattern::Entity(id) => GraphValue::Entity(id.clone()),
            ObjectPattern::Value(value) => value.clone(),
        }),
        source: None,
        valid_at: query.valid_at.map(ValidTime::new),
        known_at: query.known_at.map(TxTime::new),
        context: query.context.clone(),
        min_confidence: query.min_confidence,
        limit: query.limit,
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
            event_type: None,
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

    let embedding_provider = FixtureQuestionEmbeddingProvider;
    let model_provider = FixtureContextPackIntentProvider;
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
    let metric_operation = match operation {
        "graph_query" => "query",
        "path_query" => "path",
        "evidence_pack" => "evidence_pack",
        "ai_context_pack" => "ai_context_pack",
        other => other,
    };
    state.record_operation(metric_operation, elapsed);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_redb_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        path.push(format!(
            "hotgraph-api-{name}-{}-{nanos}.redb",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn api_redb_state_reloads_events_and_idempotency_after_restart() {
        let path = temp_redb_path("idempotent-restart");
        let command = GraphCommand::AddSource(AddSource {
            id: SourceId::new("source-redb-1"),
            source_type: SourceType::Document,
            uri: Some("file://redb-source.md".to_owned()),
            content_hash: rg_core::ContentHash::new("sha256:redb-source"),
            trust_score: Some(0.8),
        });

        {
            let state = ApiState::from_redb_graph_store(&path).expect("create redb api state");
            let first = state
                .execute(command.clone(), Some("source-redb-1-once".to_owned()))
                .expect("first write");
            let second = state
                .execute(command.clone(), Some("source-redb-1-once".to_owned()))
                .expect("idempotent replay before restart");

            assert_eq!(first, second);
            assert_eq!(state.metrics_snapshot().expect("metrics").events, 1);
        }

        let restarted = ApiState::from_redb_graph_store(&path).expect("reload redb api state");
        assert_eq!(restarted.metrics_snapshot().expect("metrics").events, 1);
        let after_restart = restarted
            .execute(command, Some("source-redb-1-once".to_owned()))
            .expect("idempotent replay after restart");

        assert_eq!(
            after_restart.event_id().as_str(),
            "evt-000000000000000001-source-added"
        );
        assert_eq!(restarted.metrics_snapshot().expect("metrics").events, 1);

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn production_reader_role_requires_writer_url() {
        let previous_keys = std::env::var("RG_API_KEYS").ok();
        let previous_dev = std::env::var("HOTGRAPH_DEV_AUTH_DISABLED").ok();
        let previous_role = std::env::var("HOTGRAPH_NODE_ROLE").ok();
        let previous_writer_url = std::env::var("HOTGRAPH_WRITER_URL").ok();

        std::env::set_var(
            "RG_API_KEYS",
            "reader-secret:reader-service:tenant-default:reader",
        );
        std::env::remove_var("HOTGRAPH_DEV_AUTH_DISABLED");
        std::env::set_var("HOTGRAPH_NODE_ROLE", "reader");
        std::env::remove_var("HOTGRAPH_WRITER_URL");

        let error = ApiState::from_env().expect_err("reader without writer URL must fail");

        assert!(error
            .to_string()
            .contains("HOTGRAPH_WRITER_URL is required for reader nodes"));

        restore_env("RG_API_KEYS", previous_keys);
        restore_env("HOTGRAPH_DEV_AUTH_DISABLED", previous_dev);
        restore_env("HOTGRAPH_NODE_ROLE", previous_role);
        restore_env("HOTGRAPH_WRITER_URL", previous_writer_url);
    }

    #[test]
    fn reader_role_rejects_local_writes_with_stable_error_code() {
        let mut state = ApiState::new_in_memory();
        state.node_role = NodeRole::Reader {
            writer_url: "http://hotgraph-writer:8080".to_owned(),
            max_lag_lsn: Some(10),
        };
        let command = GraphCommand::AddSource(AddSource {
            id: SourceId::new("reader-source"),
            source_type: SourceType::Document,
            uri: None,
            content_hash: rg_core::ContentHash::new("sha256:reader-source"),
            trust_score: None,
        });

        let error = state
            .execute(command, Some("reader-source-once".to_owned()))
            .expect_err("reader must not acknowledge local write");

        assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error.code, "writer_required");
        assert!(error.message.contains("http://hotgraph-writer:8080"));
    }

    fn restore_env(name: &str, value: Option<String>) {
        if let Some(value) = value {
            std::env::set_var(name, value);
        } else {
            std::env::remove_var(name);
        }
    }
}

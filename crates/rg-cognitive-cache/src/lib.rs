//! Ultra-low-latency cognitive cache primitives for Reality Graph.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rg_ai::EvidencePack;
use rg_core::{AgentId, AssertionId, EntityId, MemoryId, SourceId, TenantId, TxTime, ValidTime};
use rg_query::PathResult;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PermissionScope {
    Tenant {
        tenant_id: TenantId,
    },
    Agent {
        tenant_id: TenantId,
        agent_id: AgentId,
    },
    User {
        tenant_id: TenantId,
        user_id: String,
    },
}

impl PermissionScope {
    pub fn tenant(tenant_id: TenantId) -> Self {
        Self::Tenant { tenant_id }
    }

    pub fn agent(tenant_id: TenantId, agent_id: AgentId) -> Self {
        Self::Agent {
            tenant_id,
            agent_id,
        }
    }

    pub fn user(tenant_id: TenantId, user_id: impl Into<String>) -> Self {
        Self::User {
            tenant_id,
            user_id: user_id.into(),
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        match self {
            Self::Tenant { tenant_id }
            | Self::Agent { tenant_id, .. }
            | Self::User { tenant_id, .. } => tenant_id,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PermissionAwareCacheKey {
    namespace: CacheNamespace,
    permission_scope: PermissionScope,
    subject: String,
    valid_at: Option<ValidTime>,
    known_at: Option<TxTime>,
    policy_epoch: u64,
}

impl PermissionAwareCacheKey {
    pub fn for_entity_state(
        permission_scope: PermissionScope,
        entity_id: EntityId,
        valid_at: Option<ValidTime>,
        known_at: Option<TxTime>,
    ) -> Self {
        Self::new(
            CacheNamespace::EntityState,
            permission_scope,
            entity_id.to_string(),
            valid_at,
            known_at,
        )
    }

    pub fn for_evidence_pack(
        permission_scope: PermissionScope,
        task_or_query_hash: impl Into<String>,
        valid_at: Option<ValidTime>,
        known_at: Option<TxTime>,
    ) -> Self {
        Self::new(
            CacheNamespace::EvidencePack,
            permission_scope,
            task_or_query_hash.into(),
            valid_at,
            known_at,
        )
    }

    pub fn for_path_query(
        permission_scope: PermissionScope,
        start: EntityId,
        end: Option<EntityId>,
        valid_at: Option<ValidTime>,
        known_at: Option<TxTime>,
    ) -> Self {
        let end = end.map_or_else(|| "*".to_string(), |entity| entity.to_string());
        Self::new(
            CacheNamespace::PathQuery,
            permission_scope,
            format!("{start}->{end}"),
            valid_at,
            known_at,
        )
    }

    pub fn with_policy_epoch(mut self, policy_epoch: u64) -> Self {
        self.policy_epoch = policy_epoch;
        self
    }

    pub fn can_share_with(&self, other: &Self) -> bool {
        self == other
            && self.permission_scope.tenant_id() == other.permission_scope.tenant_id()
            && self.policy_epoch == other.policy_epoch
    }

    fn new(
        namespace: CacheNamespace,
        permission_scope: PermissionScope,
        subject: String,
        valid_at: Option<ValidTime>,
        known_at: Option<TxTime>,
    ) -> Self {
        Self {
            namespace,
            permission_scope,
            subject,
            valid_at,
            known_at,
            policy_epoch: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum CacheNamespace {
    EntityState,
    EvidencePack,
    PathQuery,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentWorkingSet {
    tenant_id: TenantId,
    agent_id: AgentId,
    user_id: Option<String>,
    capacity: usize,
    hot_entities: Vec<EntityId>,
    hot_memories: Vec<MemoryId>,
    recent_tasks: Vec<String>,
}

impl AgentWorkingSet {
    pub fn new(tenant_id: TenantId, agent_id: AgentId, capacity: usize) -> Self {
        Self {
            tenant_id,
            agent_id,
            user_id: None,
            capacity,
            hot_entities: Vec::new(),
            hot_memories: Vec::new(),
            recent_tasks: Vec::new(),
        }
    }

    pub fn for_user(
        tenant_id: TenantId,
        agent_id: AgentId,
        user_id: impl Into<String>,
        capacity: usize,
    ) -> Self {
        let mut working_set = Self::new(tenant_id, agent_id, capacity);
        working_set.user_id = Some(user_id.into());
        working_set
    }

    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    pub fn remember_entity(&mut self, entity_id: EntityId) {
        remember_recent(&mut self.hot_entities, entity_id, self.capacity);
    }

    pub fn remember_memory(&mut self, memory_id: MemoryId) {
        remember_recent(&mut self.hot_memories, memory_id, self.capacity);
    }

    pub fn remember_task(&mut self, task: impl Into<String>) {
        remember_recent(&mut self.recent_tasks, task.into(), self.capacity);
    }

    pub fn hot_entities(&self) -> &[EntityId] {
        &self.hot_entities
    }

    pub fn hot_memories(&self) -> &[MemoryId] {
        &self.hot_memories
    }

    pub fn recent_tasks(&self) -> &[String] {
        &self.recent_tasks
    }

    pub fn contains_memory(&self, memory_id: &MemoryId) -> bool {
        self.hot_memories.contains(memory_id)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CachedEntityState {
    pub entity_id: EntityId,
    pub assertion_ids: Vec<AssertionId>,
    pub source_ids: Vec<SourceId>,
    pub valid_at: Option<ValidTime>,
    pub known_at: Option<TxTime>,
    pub cached_at: TxTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CachedEvidencePack {
    pub pack: EvidencePack,
    pub assertion_ids: Vec<AssertionId>,
    pub source_ids: Vec<SourceId>,
    pub compressed_tokens: usize,
    pub cached_at: TxTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CachedPathSet {
    pub paths: Vec<PathResult>,
    pub assertion_ids: Vec<AssertionId>,
    pub cached_at: TxTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CachedMemoryRecall {
    pub memory_ids: Vec<MemoryId>,
    pub related_entity_ids: Vec<EntityId>,
    pub source_ids: Vec<SourceId>,
    pub cached_at: TxTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticNeighborhood {
    pub seed: String,
    pub entity_ids: Vec<EntityId>,
    pub memory_ids: Vec<MemoryId>,
    pub score: f32,
    pub cached_at: TxTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompressedContext {
    pub text: String,
    pub token_count: usize,
    pub assertion_ids: Vec<AssertionId>,
    pub source_ids: Vec<SourceId>,
    pub cached_at: TxTime,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityHotCache {
    entries: BTreeMap<PermissionAwareCacheKey, CachedEntityState>,
}

impl EntityHotCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: PermissionAwareCacheKey, state: CachedEntityState) {
        self.entries.insert(key, state);
    }

    pub fn get(&self, key: &PermissionAwareCacheKey) -> Option<&CachedEntityState> {
        self.entries.get(key)
    }

    pub fn invalidate_keys(&mut self, keys: &BTreeSet<PermissionAwareCacheKey>) -> usize {
        invalidate_entries(&mut self.entries, keys)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EvidencePackCache {
    entries: BTreeMap<PermissionAwareCacheKey, CachedEvidencePack>,
}

impl EvidencePackCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: PermissionAwareCacheKey, pack: CachedEvidencePack) {
        self.entries.insert(key, pack);
    }

    pub fn get(&self, key: &PermissionAwareCacheKey) -> Option<&CachedEvidencePack> {
        self.entries.get(key)
    }

    pub fn invalidate_keys(&mut self, keys: &BTreeSet<PermissionAwareCacheKey>) -> usize {
        invalidate_entries(&mut self.entries, keys)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathQueryCache {
    entries: BTreeMap<PermissionAwareCacheKey, CachedPathSet>,
}

impl PathQueryCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: PermissionAwareCacheKey, paths: CachedPathSet) {
        self.entries.insert(key, paths);
    }

    pub fn get(&self, key: &PermissionAwareCacheKey) -> Option<&CachedPathSet> {
        self.entries.get(key)
    }

    pub fn invalidate_keys(&mut self, keys: &BTreeSet<PermissionAwareCacheKey>) -> usize {
        invalidate_entries(&mut self.entries, keys)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemoryHotCache {
    entries: BTreeMap<PermissionAwareCacheKey, CachedMemoryRecall>,
}

impl MemoryHotCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: PermissionAwareCacheKey, recall: CachedMemoryRecall) {
        self.entries.insert(key, recall);
    }

    pub fn get(&self, key: &PermissionAwareCacheKey) -> Option<&CachedMemoryRecall> {
        self.entries.get(key)
    }

    pub fn invalidate_keys(&mut self, keys: &BTreeSet<PermissionAwareCacheKey>) -> usize {
        invalidate_entries(&mut self.entries, keys)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SemanticLocalityCache {
    entries: BTreeMap<PermissionAwareCacheKey, SemanticNeighborhood>,
}

impl SemanticLocalityCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: PermissionAwareCacheKey, neighborhood: SemanticNeighborhood) {
        self.entries.insert(key, neighborhood);
    }

    pub fn get(&self, key: &PermissionAwareCacheKey) -> Option<&SemanticNeighborhood> {
        self.entries.get(key)
    }

    pub fn invalidate_keys(&mut self, keys: &BTreeSet<PermissionAwareCacheKey>) -> usize {
        invalidate_entries(&mut self.entries, keys)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompressedContextCache {
    entries: BTreeMap<PermissionAwareCacheKey, CompressedContext>,
}

impl CompressedContextCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: PermissionAwareCacheKey, context: CompressedContext) {
        self.entries.insert(key, context);
    }

    pub fn get(&self, key: &PermissionAwareCacheKey) -> Option<&CompressedContext> {
        self.entries.get(key)
    }

    pub fn invalidate_keys(&mut self, keys: &BTreeSet<PermissionAwareCacheKey>) -> usize {
        invalidate_entries(&mut self.entries, keys)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CacheDependencySet {
    entity_ids: BTreeSet<EntityId>,
    assertion_ids: BTreeSet<AssertionId>,
    source_ids: BTreeSet<SourceId>,
    valid_window: Option<TemporalWindow<ValidTime>>,
    known_at: Option<TxTime>,
}

impl CacheDependencySet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_entity(mut self, entity_id: EntityId) -> Self {
        self.entity_ids.insert(entity_id);
        self
    }

    pub fn with_assertion(mut self, assertion_id: AssertionId) -> Self {
        self.assertion_ids.insert(assertion_id);
        self
    }

    pub fn with_source(mut self, source_id: SourceId) -> Self {
        self.source_ids.insert(source_id);
        self
    }

    pub fn with_valid_window(mut self, start: ValidTime, end: Option<ValidTime>) -> Self {
        self.valid_window = Some(TemporalWindow { start, end });
        self
    }

    pub fn with_known_at(mut self, known_at: TxTime) -> Self {
        self.known_at = Some(known_at);
        self
    }

    fn contains_valid_time(&self, instant: ValidTime) -> bool {
        self.valid_window
            .as_ref()
            .is_some_and(|window| window.contains(instant))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TemporalWindow<T> {
    start: T,
    end: Option<T>,
}

impl<T: Copy + Ord> TemporalWindow<T> {
    fn contains(self, instant: T) -> bool {
        instant >= self.start && self.end.map_or(true, |end| instant < end)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TemporalInvalidationIndex {
    dependencies: BTreeMap<PermissionAwareCacheKey, CacheDependencySet>,
    by_entity: BTreeMap<EntityId, BTreeSet<PermissionAwareCacheKey>>,
    by_assertion: BTreeMap<AssertionId, BTreeSet<PermissionAwareCacheKey>>,
    by_source: BTreeMap<SourceId, BTreeSet<PermissionAwareCacheKey>>,
}

impl TemporalInvalidationIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn track(&mut self, key: PermissionAwareCacheKey, dependencies: CacheDependencySet) {
        for entity_id in &dependencies.entity_ids {
            self.by_entity
                .entry(entity_id.clone())
                .or_default()
                .insert(key.clone());
        }
        for assertion_id in &dependencies.assertion_ids {
            self.by_assertion
                .entry(assertion_id.clone())
                .or_default()
                .insert(key.clone());
        }
        for source_id in &dependencies.source_ids {
            self.by_source
                .entry(source_id.clone())
                .or_default()
                .insert(key.clone());
        }
        self.dependencies.insert(key, dependencies);
    }

    pub fn keys_for_entity(&self, entity_id: &EntityId) -> BTreeSet<PermissionAwareCacheKey> {
        self.by_entity.get(entity_id).cloned().unwrap_or_default()
    }

    pub fn keys_for_assertion(
        &self,
        assertion_id: &AssertionId,
    ) -> BTreeSet<PermissionAwareCacheKey> {
        self.by_assertion
            .get(assertion_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn keys_for_source(&self, source_id: &SourceId) -> BTreeSet<PermissionAwareCacheKey> {
        self.by_source.get(source_id).cloned().unwrap_or_default()
    }

    pub fn keys_valid_at(&self, instant: ValidTime) -> BTreeSet<PermissionAwareCacheKey> {
        self.dependencies
            .iter()
            .filter(|(_, dependencies)| dependencies.contains_valid_time(instant))
            .map(|(key, _)| key.clone())
            .collect()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContradictionAwareInvalidation;

impl ContradictionAwareInvalidation {
    pub fn affected_keys(
        index: &TemporalInvalidationIndex,
        assertion_a: &AssertionId,
        assertion_b: &AssertionId,
    ) -> BTreeSet<PermissionAwareCacheKey> {
        let mut keys = index.keys_for_assertion(assertion_a);
        keys.extend(index.keys_for_assertion(assertion_b));
        keys
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SummaryId(String);

impl SummaryId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SummaryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SummaryStalenessTracker {
    summaries: BTreeMap<SummaryId, SummaryDependencies>,
    by_assertion: BTreeMap<AssertionId, BTreeSet<SummaryId>>,
    by_source: BTreeMap<SourceId, BTreeSet<SummaryId>>,
}

impl SummaryStalenessTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn track_summary(
        &mut self,
        summary_id: SummaryId,
        assertion_ids: Vec<AssertionId>,
        source_ids: Vec<SourceId>,
    ) {
        for assertion_id in &assertion_ids {
            self.by_assertion
                .entry(assertion_id.clone())
                .or_default()
                .insert(summary_id.clone());
        }
        for source_id in &source_ids {
            self.by_source
                .entry(source_id.clone())
                .or_default()
                .insert(summary_id.clone());
        }
        self.summaries.insert(
            summary_id,
            SummaryDependencies {
                assertion_ids,
                source_ids,
                stale_reason: None,
            },
        );
    }

    pub fn mark_assertion_changed(
        &mut self,
        assertion_id: &AssertionId,
        reason: impl Into<String>,
    ) {
        let reason = reason.into();
        if let Some(summary_ids) = self.by_assertion.get(assertion_id) {
            for summary_id in summary_ids {
                if let Some(summary) = self.summaries.get_mut(summary_id) {
                    summary.stale_reason = Some(reason.clone());
                }
            }
        }
    }

    pub fn is_stale(&self, summary_id: &SummaryId) -> bool {
        self.summaries
            .get(summary_id)
            .and_then(|summary| summary.stale_reason.as_ref())
            .is_some()
    }

    pub fn stale_reason(&self, summary_id: &SummaryId) -> Option<&str> {
        self.summaries
            .get(summary_id)
            .and_then(|summary| summary.stale_reason.as_deref())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SummaryDependencies {
    assertion_ids: Vec<AssertionId>,
    source_ids: Vec<SourceId>,
    stale_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CacheOperation {
    HotMemoryRecall,
    HotEntityState,
    CachedEvidencePack,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CachePerformanceTargets {
    pub hot_memory_recall_p95_ms: u64,
    pub hot_entity_state_p95_ms: u64,
    pub cached_evidence_pack_p95_ms: u64,
}

impl CachePerformanceTargets {
    pub fn single_node_mvp() -> Self {
        Self {
            hot_memory_recall_p95_ms: 30,
            hot_entity_state_p95_ms: 20,
            cached_evidence_pack_p95_ms: 300,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CognitiveCacheMetrics {
    latencies: BTreeMap<CacheOperation, Vec<u64>>,
}

impl CognitiveCacheMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_latency(&mut self, operation: CacheOperation, latency_ms: u64) {
        self.latencies
            .entry(operation)
            .or_default()
            .push(latency_ms);
    }

    pub fn p95_ms(&self, operation: CacheOperation) -> Option<u64> {
        let mut values = self.latencies.get(&operation)?.clone();
        if values.is_empty() {
            return None;
        }
        values.sort_unstable();
        let rank = ((values.len() as f64) * 0.95).ceil() as usize;
        values.get(rank.saturating_sub(1)).copied()
    }

    pub fn meets_targets(&self, targets: &CachePerformanceTargets) -> bool {
        self.p95_ms(CacheOperation::HotMemoryRecall)
            .is_some_and(|latency| latency <= targets.hot_memory_recall_p95_ms)
            && self
                .p95_ms(CacheOperation::HotEntityState)
                .is_some_and(|latency| latency <= targets.hot_entity_state_p95_ms)
            && self
                .p95_ms(CacheOperation::CachedEvidencePack)
                .is_some_and(|latency| latency <= targets.cached_evidence_pack_p95_ms)
    }
}

fn remember_recent<T: Eq>(items: &mut Vec<T>, item: T, capacity: usize) {
    if let Some(position) = items.iter().position(|existing| existing == &item) {
        items.remove(position);
    }
    if capacity == 0 {
        return;
    }
    items.push(item);
    while items.len() > capacity {
        items.remove(0);
    }
}

fn invalidate_entries<T>(
    entries: &mut BTreeMap<PermissionAwareCacheKey, T>,
    keys: &BTreeSet<PermissionAwareCacheKey>,
) -> usize {
    keys.iter()
        .filter(|key| entries.remove(*key).is_some())
        .count()
}

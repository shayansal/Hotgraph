use rg_ai::EvidencePack;
use rg_cognitive_cache::{
    AgentWorkingSet, CacheDependencySet, CacheOperation, CachePerformanceTargets,
    CachedEntityState, CachedEvidencePack, CachedMemoryRecall, CachedPathSet,
    CognitiveCacheMetrics, CompressedContext, CompressedContextCache,
    ContradictionAwareInvalidation, EntityHotCache, EvidencePackCache, MemoryHotCache,
    PathQueryCache, PermissionAwareCacheKey, PermissionScope, SemanticLocalityCache,
    SemanticNeighborhood, SummaryId, SummaryStalenessTracker, TemporalInvalidationIndex,
};
use rg_core::{AgentId, AssertionId, EntityId, MemoryId, SourceId, TenantId, TxTime, ValidTime};
use rg_query::PathResult;

#[test]
fn permission_aware_keys_do_not_cross_tenant_agent_or_policy_epoch() {
    let tenant = TenantId::new("tenant-a");
    let other_tenant = TenantId::new("tenant-b");
    let agent = AgentId::new("agent-a");
    let other_agent = AgentId::new("agent-b");
    let entity = EntityId::new("entity-1");

    let scope = PermissionScope::agent(tenant.clone(), agent.clone());
    let key = PermissionAwareCacheKey::for_entity_state(
        scope.clone(),
        entity.clone(),
        Some(ValidTime::new(100)),
        Some(TxTime::new(200)),
    )
    .with_policy_epoch(7);

    let same = PermissionAwareCacheKey::for_entity_state(
        scope,
        entity.clone(),
        Some(ValidTime::new(100)),
        Some(TxTime::new(200)),
    )
    .with_policy_epoch(7);
    let different_policy = same.clone().with_policy_epoch(8);
    let different_agent = PermissionAwareCacheKey::for_entity_state(
        PermissionScope::agent(tenant, other_agent),
        entity.clone(),
        Some(ValidTime::new(100)),
        Some(TxTime::new(200)),
    )
    .with_policy_epoch(7);
    let different_tenant = PermissionAwareCacheKey::for_entity_state(
        PermissionScope::tenant(other_tenant),
        entity,
        Some(ValidTime::new(100)),
        Some(TxTime::new(200)),
    )
    .with_policy_epoch(7);

    assert_eq!(key, same);
    assert_ne!(key, different_policy);
    assert!(!key.can_share_with(&different_policy));
    assert!(!key.can_share_with(&different_agent));
    assert!(!key.can_share_with(&different_tenant));

    let user_scope = PermissionScope::user(TenantId::new("tenant-a"), "user-a");
    assert_eq!(user_scope.tenant_id(), &TenantId::new("tenant-a"));
}

#[test]
fn agent_working_set_keeps_hot_items_and_recent_tasks_bounded() {
    let mut working_set = AgentWorkingSet::for_user(
        TenantId::new("tenant-a"),
        AgentId::new("agent-a"),
        "user-a",
        2,
    );

    working_set.remember_entity(EntityId::new("entity-1"));
    working_set.remember_entity(EntityId::new("entity-2"));
    working_set.remember_entity(EntityId::new("entity-3"));
    working_set.remember_memory(MemoryId::new("memory-1"));
    working_set.remember_memory(MemoryId::new("memory-2"));
    working_set.remember_memory(MemoryId::new("memory-3"));
    working_set.remember_task("draft-risk-brief");
    working_set.remember_task("check-supplier");
    working_set.remember_task("prepare-call");

    assert_eq!(working_set.user_id(), Some("user-a"));
    assert_eq!(
        working_set.hot_entities(),
        &[EntityId::new("entity-2"), EntityId::new("entity-3")]
    );
    assert_eq!(
        working_set.hot_memories(),
        &[MemoryId::new("memory-2"), MemoryId::new("memory-3")]
    );
    assert_eq!(
        working_set.recent_tasks(),
        &["check-supplier".to_string(), "prepare-call".to_string()]
    );
    assert!(working_set.contains_memory(&MemoryId::new("memory-3")));
    assert!(!working_set.contains_memory(&MemoryId::new("memory-1")));
}

#[test]
fn hot_caches_round_trip_only_for_matching_permission_keys() {
    let key = entity_key("tenant-a", "agent-a", "entity-1");
    let wrong_agent_key = entity_key("tenant-a", "agent-b", "entity-1");

    let mut entity_cache = EntityHotCache::new();
    entity_cache.insert(
        key.clone(),
        CachedEntityState {
            entity_id: EntityId::new("entity-1"),
            assertion_ids: vec![AssertionId::new("assertion-1")],
            source_ids: vec![SourceId::new("source-1")],
            valid_at: Some(ValidTime::new(10)),
            known_at: Some(TxTime::new(20)),
            cached_at: TxTime::new(30),
        },
    );

    assert_eq!(
        entity_cache
            .get(&key)
            .expect("entity cache hit")
            .assertion_ids,
        vec![AssertionId::new("assertion-1")]
    );
    assert!(entity_cache.get(&wrong_agent_key).is_none());

    let evidence_key = PermissionAwareCacheKey::for_evidence_pack(
        PermissionScope::agent(TenantId::new("tenant-a"), AgentId::new("agent-a")),
        "task:supplier-risk",
        Some(ValidTime::new(10)),
        Some(TxTime::new(20)),
    );
    let mut evidence_cache = EvidencePackCache::new();
    evidence_cache.insert(
        evidence_key.clone(),
        CachedEvidencePack {
            pack: empty_pack("supplier risk"),
            assertion_ids: vec![AssertionId::new("assertion-1")],
            source_ids: vec![SourceId::new("source-1")],
            compressed_tokens: 128,
            cached_at: TxTime::new(31),
        },
    );
    assert_eq!(
        evidence_cache
            .get(&evidence_key)
            .expect("evidence cache hit")
            .pack
            .query,
        "supplier risk"
    );

    let path_key = PermissionAwareCacheKey::for_path_query(
        PermissionScope::agent(TenantId::new("tenant-a"), AgentId::new("agent-a")),
        EntityId::new("entity-1"),
        Some(EntityId::new("entity-2")),
        Some(ValidTime::new(10)),
        Some(TxTime::new(20)),
    );
    let mut path_cache = PathQueryCache::new();
    path_cache.insert(
        path_key.clone(),
        CachedPathSet {
            paths: vec![PathResult {
                start: EntityId::new("entity-1"),
                end: EntityId::new("entity-2"),
                hops: Vec::new(),
            }],
            assertion_ids: vec![AssertionId::new("assertion-1")],
            cached_at: TxTime::new(32),
        },
    );
    assert_eq!(
        path_cache
            .get(&path_key)
            .expect("path cache hit")
            .paths
            .len(),
        1
    );

    let mut semantic_cache = SemanticLocalityCache::new();
    semantic_cache.insert(
        evidence_key.clone(),
        SemanticNeighborhood {
            seed: "supplier risk".to_string(),
            entity_ids: vec![EntityId::new("entity-1")],
            memory_ids: vec![MemoryId::new("memory-1")],
            score: 0.91,
            cached_at: TxTime::new(33),
        },
    );
    assert_eq!(
        semantic_cache
            .get(&evidence_key)
            .expect("semantic cache hit")
            .memory_ids,
        vec![MemoryId::new("memory-1")]
    );

    let mut context_cache = CompressedContextCache::new();
    context_cache.insert(
        evidence_key.clone(),
        CompressedContext {
            text: "source-backed context".to_string(),
            token_count: 64,
            assertion_ids: vec![AssertionId::new("assertion-1")],
            source_ids: vec![SourceId::new("source-1")],
            cached_at: TxTime::new(34),
        },
    );
    assert_eq!(
        context_cache
            .get(&evidence_key)
            .expect("compressed context cache hit")
            .token_count,
        64
    );

    let mut memory_cache = MemoryHotCache::new();
    memory_cache.insert(
        evidence_key.clone(),
        CachedMemoryRecall {
            memory_ids: vec![MemoryId::new("memory-1")],
            related_entity_ids: vec![EntityId::new("entity-1")],
            source_ids: vec![SourceId::new("source-1")],
            cached_at: TxTime::new(35),
        },
    );
    assert_eq!(
        memory_cache
            .get(&evidence_key)
            .expect("memory cache hit")
            .memory_ids,
        vec![MemoryId::new("memory-1")]
    );
}

#[test]
fn temporal_contradiction_and_summary_changes_invalidate_dependent_entries() {
    let key = entity_key("tenant-a", "agent-a", "entity-1");
    let dependency = CacheDependencySet::new()
        .with_entity(EntityId::new("entity-1"))
        .with_assertion(AssertionId::new("assertion-a"))
        .with_source(SourceId::new("source-a"))
        .with_valid_window(ValidTime::new(100), Some(ValidTime::new(200)))
        .with_known_at(TxTime::new(500));

    let mut index = TemporalInvalidationIndex::new();
    index.track(key.clone(), dependency);

    assert!(index
        .keys_for_entity(&EntityId::new("entity-1"))
        .contains(&key));
    assert!(index
        .keys_for_source(&SourceId::new("source-a"))
        .contains(&key));
    assert!(index.keys_valid_at(ValidTime::new(150)).contains(&key));
    assert!(!index.keys_valid_at(ValidTime::new(250)).contains(&key));

    let contradiction_keys = ContradictionAwareInvalidation::affected_keys(
        &index,
        &AssertionId::new("assertion-a"),
        &AssertionId::new("assertion-b"),
    );
    assert!(contradiction_keys.contains(&key));

    let mut entity_cache = EntityHotCache::new();
    entity_cache.insert(
        key.clone(),
        CachedEntityState {
            entity_id: EntityId::new("entity-1"),
            assertion_ids: vec![AssertionId::new("assertion-a")],
            source_ids: vec![SourceId::new("source-a")],
            valid_at: Some(ValidTime::new(150)),
            known_at: Some(TxTime::new(500)),
            cached_at: TxTime::new(600),
        },
    );
    assert_eq!(entity_cache.invalidate_keys(&contradiction_keys), 1);
    assert!(entity_cache.get(&key).is_none());

    let mut evidence_cache = EvidencePackCache::new();
    evidence_cache.insert(
        key.clone(),
        CachedEvidencePack {
            pack: empty_pack("cached recurring task"),
            assertion_ids: vec![AssertionId::new("assertion-a")],
            source_ids: vec![SourceId::new("source-a")],
            compressed_tokens: 42,
            cached_at: TxTime::new(600),
        },
    );
    let mut path_cache = PathQueryCache::new();
    path_cache.insert(
        key.clone(),
        CachedPathSet {
            paths: Vec::new(),
            assertion_ids: vec![AssertionId::new("assertion-a")],
            cached_at: TxTime::new(600),
        },
    );
    let mut semantic_cache = SemanticLocalityCache::new();
    semantic_cache.insert(
        key.clone(),
        SemanticNeighborhood {
            seed: "risk".to_string(),
            entity_ids: vec![EntityId::new("entity-1")],
            memory_ids: vec![MemoryId::new("memory-a")],
            score: 1.0,
            cached_at: TxTime::new(600),
        },
    );
    let mut context_cache = CompressedContextCache::new();
    context_cache.insert(
        key.clone(),
        CompressedContext {
            text: "compressed".to_string(),
            token_count: 10,
            assertion_ids: vec![AssertionId::new("assertion-a")],
            source_ids: vec![SourceId::new("source-a")],
            cached_at: TxTime::new(600),
        },
    );
    let mut memory_cache = MemoryHotCache::new();
    memory_cache.insert(
        key.clone(),
        CachedMemoryRecall {
            memory_ids: vec![MemoryId::new("memory-a")],
            related_entity_ids: vec![EntityId::new("entity-1")],
            source_ids: vec![SourceId::new("source-a")],
            cached_at: TxTime::new(600),
        },
    );
    assert_eq!(evidence_cache.invalidate_keys(&contradiction_keys), 1);
    assert_eq!(path_cache.invalidate_keys(&contradiction_keys), 1);
    assert_eq!(semantic_cache.invalidate_keys(&contradiction_keys), 1);
    assert_eq!(context_cache.invalidate_keys(&contradiction_keys), 1);
    assert_eq!(memory_cache.invalidate_keys(&contradiction_keys), 1);

    let mut summaries = SummaryStalenessTracker::new();
    summaries.track_summary(
        SummaryId::new("community:energy"),
        vec![AssertionId::new("assertion-a")],
        vec![SourceId::new("source-a")],
    );
    assert_eq!(
        SummaryId::new("community:energy").as_str(),
        "community:energy"
    );
    summaries.mark_assertion_changed(&AssertionId::new("assertion-a"), "new contradiction");
    assert!(summaries.is_stale(&SummaryId::new("community:energy")));
    assert_eq!(
        summaries.stale_reason(&SummaryId::new("community:energy")),
        Some("new contradiction")
    );
}

#[test]
fn cache_metrics_report_whether_hot_latency_targets_are_met() {
    let targets = CachePerformanceTargets::single_node_mvp();
    let mut metrics = CognitiveCacheMetrics::new();

    for latency in [12, 28, 29] {
        metrics.record_latency(CacheOperation::HotMemoryRecall, latency);
    }
    for latency in [9, 18, 19] {
        metrics.record_latency(CacheOperation::HotEntityState, latency);
    }
    for latency in [120, 280, 299] {
        metrics.record_latency(CacheOperation::CachedEvidencePack, latency);
    }

    assert_eq!(metrics.p95_ms(CacheOperation::HotMemoryRecall), Some(29));
    assert!(metrics.meets_targets(&targets));

    metrics.record_latency(CacheOperation::CachedEvidencePack, 350);
    assert!(!metrics.meets_targets(&targets));
}

fn entity_key(tenant: &str, agent: &str, entity: &str) -> PermissionAwareCacheKey {
    PermissionAwareCacheKey::for_entity_state(
        PermissionScope::agent(TenantId::new(tenant), AgentId::new(agent)),
        EntityId::new(entity),
        Some(ValidTime::new(10)),
        Some(TxTime::new(20)),
    )
}

fn empty_pack(query: &str) -> EvidencePack {
    EvidencePack {
        query: query.to_string(),
        entities: Vec::new(),
        assertions: Vec::new(),
        sources: Vec::new(),
        paths: Vec::new(),
        contradictions: Vec::new(),
        generated_at: TxTime::new(30),
    }
}

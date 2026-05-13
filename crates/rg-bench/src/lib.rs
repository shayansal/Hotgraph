//! Benchmark helpers for Hotgraph.

use rg_ai::{
    EmbeddingKind, InMemoryVectorIndex, VectorId, VectorIndex, VectorIndexError, VectorRecord,
};
use rg_core::{
    AgentId, AgentMemory, AssertionId, Confidence, ContentHash, ContextScope, EntityId, EntityType,
    GraphValue, MemoryId, MemoryStatus, MemoryType, PredicateId, PropertyMap, SourceId, SourceType,
    TimeInterval, TxTime, ValidTime,
};
use rg_events::{AddAssertion, AddSource, CreateEntity, EventLog, GraphCommand, RecordAgentMemory};
use rg_index::TemporalIndex;
use rg_storage::{InMemoryStorage, StorageError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntheticGraphConfig {
    pub entity_count: usize,
    pub assertion_count: usize,
    pub memory_count: usize,
    pub branch_factor: usize,
}

impl SyntheticGraphConfig {
    pub fn smoke() -> Self {
        Self {
            entity_count: 32,
            assertion_count: 128,
            memory_count: 16,
            branch_factor: 4,
        }
    }

    pub fn standard() -> Self {
        Self {
            entity_count: 2_500,
            assertion_count: 10_000,
            memory_count: 1_000,
            branch_factor: 8,
        }
    }

    pub fn mvp_target() -> Self {
        Self {
            entity_count: 100_000,
            assertion_count: 1_000_000,
            memory_count: 25_000,
            branch_factor: 16,
        }
    }

    fn with_minimum_entities(self, minimum: usize) -> Self {
        Self {
            entity_count: self.entity_count.max(minimum),
            branch_factor: self.branch_factor.max(1),
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BenchmarkTargets {
    pub assertions_loaded: usize,
    pub point_query_p95_ms: u64,
    pub two_hop_traversal_p95_ms: u64,
    pub replay_events_per_sec: u64,
    pub batched_ingest_assertions_per_sec: u64,
}

pub const SINGLE_NODE_MVP_TARGETS: BenchmarkTargets = BenchmarkTargets {
    assertions_loaded: 1_000_000,
    point_query_p95_ms: 100,
    two_hop_traversal_p95_ms: 250,
    replay_events_per_sec: 50_000,
    batched_ingest_assertions_per_sec: 5_000,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductionScaleEnvelope {
    PrivateBeta10M,
    PaidPilot50M,
    GeneralProduction100M,
}

impl ProductionScaleEnvelope {
    pub fn required_assertions(self) -> usize {
        match self {
            Self::PrivateBeta10M => 10_000_000,
            Self::PaidPilot50M => 50_000_000,
            Self::GeneralProduction100M => 100_000_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchmarkArtifact {
    pub scale: ProductionScaleEnvelope,
    pub commit_sha: String,
    pub image_digest: String,
    pub hardware_profile: String,
    pub dataset_seed: u64,
    pub assertion_count: usize,
    pub write_p50_ms: f64,
    pub write_p95_ms: f64,
    pub write_p99_ms: f64,
    pub query_p50_ms: f64,
    pub query_p95_ms: f64,
    pub query_p99_ms: f64,
    pub evidence_pack_p95_ms: f64,
    pub replay_time_ms: u64,
    pub restore_time_ms: u64,
    pub rss_mb: u64,
    pub disk_amplification: f64,
    pub compaction_pause_ms: u64,
}

impl BenchmarkArtifact {
    pub fn missing_release_fields(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.commit_sha.trim().len() < 7 {
            missing.push("commit_sha");
        }
        if !self.image_digest.starts_with("sha256:") {
            missing.push("image_digest");
        }
        if self.hardware_profile.trim().is_empty() {
            missing.push("hardware_profile");
        }
        if self.assertion_count < self.scale.required_assertions() {
            missing.push("assertion_count");
        }
        if self.write_p95_ms <= 0.0 {
            missing.push("write_p95_ms");
        }
        if self.query_p95_ms <= 0.0 {
            missing.push("query_p95_ms");
        }
        if self.evidence_pack_p95_ms <= 0.0 {
            missing.push("evidence_pack_p95_ms");
        }
        if self.restore_time_ms == 0 {
            missing.push("restore_time_ms");
        }
        if self.rss_mb == 0 {
            missing.push("rss_mb");
        }
        if self.disk_amplification <= 0.0 {
            missing.push("disk_amplification");
        }
        missing
    }

    pub fn passes_release_gate(&self) -> bool {
        self.missing_release_fields().is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchmarkReleaseSet {
    pub artifacts: Vec<BenchmarkArtifact>,
}

impl BenchmarkReleaseSet {
    pub fn missing_scales(&self) -> Vec<ProductionScaleEnvelope> {
        [
            ProductionScaleEnvelope::PrivateBeta10M,
            ProductionScaleEnvelope::PaidPilot50M,
            ProductionScaleEnvelope::GeneralProduction100M,
        ]
        .into_iter()
        .filter(|scale| {
            !self
                .artifacts
                .iter()
                .any(|artifact| artifact.scale == *scale && artifact.passes_release_gate())
        })
        .collect()
    }

    pub fn passes_production_release_gate(&self) -> bool {
        self.missing_scales().is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntheticGraphKind {
    Social,
    CompanyOwnership,
    SupplyChain,
    AgentMemory,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SyntheticGraph {
    pub kind: SyntheticGraphKind,
    pub events: Vec<rg_events::GraphEvent>,
    pub entity_ids: Vec<EntityId>,
    pub assertion_ids: Vec<AssertionId>,
    pub memory_ids: Vec<MemoryId>,
    pub source_id: SourceId,
    pub anchor_entity: EntityId,
    pub terminal_entity: EntityId,
    pub point_in_time: ValidTime,
    pub known_at: TxTime,
    pub path_predicates: Vec<PredicateId>,
    pub vector_records: Vec<VectorRecord>,
}

pub fn workspace_ready() -> bool {
    true
}

pub fn social_graph(config: SyntheticGraphConfig) -> SyntheticGraph {
    let config = config.with_minimum_entities(2);
    let source_id = SourceId::new("source-social");
    let mut log = EventLog::new(TxTime::new(0));
    add_source(&mut log, &source_id, "social");

    let entity_ids = (0..config.entity_count)
        .map(|index| person_id("person", index))
        .collect::<Vec<_>>();
    for entity_id in &entity_ids {
        add_entity(&mut log, entity_id, EntityType::Person);
    }

    let mut assertion_ids = Vec::with_capacity(config.assertion_count);
    for index in 0..config.assertion_count {
        let subject_index = index % entity_ids.len();
        let offset = 1 + index % config.branch_factor;
        let mut object_index = (subject_index + offset) % entity_ids.len();
        if object_index == subject_index {
            object_index = (object_index + 1) % entity_ids.len();
        }
        let assertion_id = AssertionId::new(format!("assertion-social-{index:06}"));
        add_assertion(
            &mut log,
            assertion_id.clone(),
            entity_ids[subject_index].clone(),
            PredicateId::new("knows"),
            GraphValue::Entity(entity_ids[object_index].clone()),
            &source_id,
            ContextScope::Named("social".to_owned()),
            0.72,
            2_000 + (index % 25) as i64,
            None,
        );
        assertion_ids.push(assertion_id);
    }

    graph_from_log(SyntheticGraphParts {
        kind: SyntheticGraphKind::Social,
        log,
        entity_ids,
        assertion_ids,
        memory_ids: Vec::new(),
        source_id,
        anchor_entity: EntityId::new("person-000000"),
        terminal_entity: EntityId::new("person-000001"),
        path_predicates: vec![PredicateId::new("knows")],
        vector_records: Vec::new(),
    })
}

pub fn company_ownership_graph(config: SyntheticGraphConfig) -> SyntheticGraph {
    let config = config.with_minimum_entities(3);
    let source_id = SourceId::new("source-ownership");
    let mut log = EventLog::new(TxTime::new(0));
    add_source(&mut log, &source_id, "ownership");

    let entity_ids = (0..config.entity_count)
        .map(|index| company_id("company", index))
        .collect::<Vec<_>>();
    for entity_id in &entity_ids {
        add_entity(&mut log, entity_id, EntityType::Organization);
    }

    let mut assertion_ids = Vec::with_capacity(config.assertion_count.max(2));
    let fixed_edges = [(0, 1), (1, 2)];
    for (index, (subject_index, object_index)) in fixed_edges.into_iter().enumerate() {
        let assertion_id = AssertionId::new(format!("assertion-ownership-{index:06}"));
        add_assertion(
            &mut log,
            assertion_id.clone(),
            entity_ids[subject_index].clone(),
            PredicateId::new("owns"),
            GraphValue::Entity(entity_ids[object_index].clone()),
            &source_id,
            ContextScope::Named("ownership".to_owned()),
            0.87,
            2_020,
            None,
        );
        assertion_ids.push(assertion_id);
    }
    for index in 2..config.assertion_count {
        let subject_index = ownership_filler_subject(index, entity_ids.len());
        let object_index = (subject_index + 1 + index % config.branch_factor) % entity_ids.len();
        let assertion_id = AssertionId::new(format!("assertion-ownership-{index:06}"));
        add_assertion(
            &mut log,
            assertion_id.clone(),
            entity_ids[subject_index].clone(),
            PredicateId::new("owns"),
            GraphValue::Entity(entity_ids[object_index].clone()),
            &source_id,
            ContextScope::Named("ownership".to_owned()),
            0.8,
            2_010 + (index % 10) as i64,
            None,
        );
        assertion_ids.push(assertion_id);
    }

    graph_from_log(SyntheticGraphParts {
        kind: SyntheticGraphKind::CompanyOwnership,
        log,
        entity_ids,
        assertion_ids,
        memory_ids: Vec::new(),
        source_id,
        anchor_entity: EntityId::new("company-000000"),
        terminal_entity: EntityId::new("company-000002"),
        path_predicates: vec![PredicateId::new("owns"), PredicateId::new("owns")],
        vector_records: Vec::new(),
    })
}

pub fn supply_chain_graph(config: SyntheticGraphConfig) -> SyntheticGraph {
    let config = config.with_minimum_entities(3);
    let source_id = SourceId::new("source-supply-chain");
    let mut log = EventLog::new(TxTime::new(0));
    add_source(&mut log, &source_id, "supply-chain");

    let entity_ids = (0..config.entity_count)
        .map(|index| company_id("supplier", index))
        .collect::<Vec<_>>();
    for entity_id in &entity_ids {
        add_entity(&mut log, entity_id, EntityType::Organization);
    }

    let mut assertion_ids = Vec::with_capacity(config.assertion_count.max(2));
    for (index, object_index) in [1_usize, 2].into_iter().enumerate() {
        let assertion_id = AssertionId::new(format!("assertion-supply-{index:06}"));
        add_assertion(
            &mut log,
            assertion_id.clone(),
            entity_ids[0].clone(),
            PredicateId::new("supplies"),
            GraphValue::Entity(entity_ids[object_index].clone()),
            &source_id,
            ContextScope::Named("supply-chain".to_owned()),
            0.81 + index as f32 * 0.01,
            2_020,
            Some(2_030),
        );
        assertion_ids.push(assertion_id);
    }
    for index in 2..config.assertion_count {
        let subject_index = (index - 1) % entity_ids.len();
        let object_index = (subject_index + 1 + index % config.branch_factor) % entity_ids.len();
        let assertion_id = AssertionId::new(format!("assertion-supply-{index:06}"));
        add_assertion(
            &mut log,
            assertion_id.clone(),
            entity_ids[subject_index].clone(),
            PredicateId::new("supplies"),
            GraphValue::Entity(entity_ids[object_index].clone()),
            &source_id,
            ContextScope::Named("supply-chain".to_owned()),
            0.74,
            2_018 + (index % 8) as i64,
            None,
        );
        assertion_ids.push(assertion_id);
    }

    graph_from_log(SyntheticGraphParts {
        kind: SyntheticGraphKind::SupplyChain,
        log,
        entity_ids,
        assertion_ids,
        memory_ids: Vec::new(),
        source_id,
        anchor_entity: EntityId::new("supplier-000000"),
        terminal_entity: EntityId::new("supplier-000002"),
        path_predicates: vec![PredicateId::new("supplies"), PredicateId::new("supplies")],
        vector_records: Vec::new(),
    })
}

pub fn agent_memory_graph(config: SyntheticGraphConfig) -> SyntheticGraph {
    let config = config.with_minimum_entities(1);
    let source_id = SourceId::new("source-agent-memory");
    let mut log = EventLog::new(TxTime::new(0));
    add_source(&mut log, &source_id, "agent-memory");

    let entity_ids = (0..config.entity_count)
        .map(|index| person_id("memory-entity", index))
        .collect::<Vec<_>>();
    for entity_id in &entity_ids {
        add_entity(&mut log, entity_id, EntityType::Person);
    }

    let mut memory_ids = Vec::with_capacity(config.memory_count);
    let mut vector_records = Vec::with_capacity(config.memory_count);
    for index in 0..config.memory_count {
        let memory_id = MemoryId::new(format!("memory-agent-{index:06}"));
        let supersedes = if index > 0 && memory_type(index) == MemoryType::Correction {
            vec![MemoryId::new(format!("memory-agent-{:06}", index - 1))]
        } else {
            Vec::new()
        };
        let memory = AgentMemory {
            id: memory_id.clone(),
            agent_id: AgentId::new(format!("agent-{:03}", index % config.branch_factor)),
            memory_type: memory_type(index),
            content: format!("Synthetic agent memory {index} about graph state."),
            valid_time: TimeInterval::new(ValidTime::new(2_020 + (index % 8) as i64), None)
                .expect("valid memory interval"),
            confidence: Confidence::new(0.65 + (index % 30) as f32 / 100.0)
                .expect("valid confidence"),
            source_ids: vec![source_id.clone()],
            related_entities: vec![entity_ids[index % entity_ids.len()].clone()],
            supersedes,
            status: MemoryStatus::Active,
        };
        execute(
            &mut log,
            GraphCommand::RecordAgentMemory(RecordAgentMemory { memory }),
        );
        vector_records.push(VectorRecord {
            id: VectorId::new(format!("vector-memory-{index:06}")),
            kind: EmbeddingKind::AgentMemory,
            embedding: deterministic_embedding(index),
            source_id: Some(source_id.clone()),
            entity_id: Some(entity_ids[index % entity_ids.len()].clone()),
            assertion_id: None,
            event_id: None,
            memory_id: Some(memory_id.clone()),
            text: Some(format!("Synthetic agent memory {index}")),
        });
        memory_ids.push(memory_id);
    }

    let anchor_entity = entity_ids[0].clone();
    let terminal_entity = entity_ids[entity_ids.len().saturating_sub(1)].clone();
    graph_from_log(SyntheticGraphParts {
        kind: SyntheticGraphKind::AgentMemory,
        log,
        entity_ids,
        assertion_ids: Vec::new(),
        memory_ids,
        source_id,
        anchor_entity,
        terminal_entity,
        path_predicates: Vec::new(),
        vector_records,
    })
}

pub fn build_storage(graph: &SyntheticGraph) -> Result<InMemoryStorage, StorageError> {
    InMemoryStorage::replay(&graph.events)
}

pub fn build_temporal_index(graph: &SyntheticGraph) -> Result<TemporalIndex, StorageError> {
    let storage = build_storage(graph)?;
    let mut index = TemporalIndex::new();
    for assertion in storage.graph_state().assertions.values() {
        index.insert_assertion(assertion.clone());
    }
    Ok(index)
}

pub fn build_vector_index(graph: &SyntheticGraph) -> Result<InMemoryVectorIndex, VectorIndexError> {
    let mut index = InMemoryVectorIndex::new();
    for record in &graph.vector_records {
        index.upsert_embedding(record.clone())?;
    }
    Ok(index)
}

struct SyntheticGraphParts {
    kind: SyntheticGraphKind,
    log: EventLog,
    entity_ids: Vec<EntityId>,
    assertion_ids: Vec<AssertionId>,
    memory_ids: Vec<MemoryId>,
    source_id: SourceId,
    anchor_entity: EntityId,
    terminal_entity: EntityId,
    path_predicates: Vec<PredicateId>,
    vector_records: Vec<VectorRecord>,
}

fn graph_from_log(parts: SyntheticGraphParts) -> SyntheticGraph {
    SyntheticGraph {
        kind: parts.kind,
        events: parts.log.events().to_vec(),
        entity_ids: parts.entity_ids,
        assertion_ids: parts.assertion_ids,
        memory_ids: parts.memory_ids,
        source_id: parts.source_id,
        anchor_entity: parts.anchor_entity,
        terminal_entity: parts.terminal_entity,
        point_in_time: ValidTime::new(2_024),
        known_at: TxTime::new(i64::MAX),
        path_predicates: parts.path_predicates,
        vector_records: parts.vector_records,
    }
}

fn add_source(log: &mut EventLog, source_id: &SourceId, label: &str) {
    execute(
        log,
        GraphCommand::AddSource(AddSource {
            id: source_id.clone(),
            source_type: SourceType::Document,
            uri: Some(format!("synthetic://{label}")),
            content_hash: ContentHash::new(format!("sha256:{label}")),
            trust_score: Some(0.9),
        }),
    );
}

fn add_entity(log: &mut EventLog, entity_id: &EntityId, entity_type: EntityType) {
    execute(
        log,
        GraphCommand::CreateEntity(CreateEntity {
            id: entity_id.clone(),
            entity_type,
            canonical_name: Some(entity_id.as_str().to_owned()),
            properties: PropertyMap::default(),
        }),
    );
}

#[allow(clippy::too_many_arguments)]
fn add_assertion(
    log: &mut EventLog,
    assertion_id: AssertionId,
    subject: EntityId,
    predicate: PredicateId,
    object: GraphValue,
    source_id: &SourceId,
    context: ContextScope,
    confidence: f32,
    valid_start: i64,
    valid_end: Option<i64>,
) {
    execute(
        log,
        GraphCommand::AddAssertion(AddAssertion {
            id: assertion_id,
            subject,
            predicate,
            object,
            valid_time: TimeInterval::new(
                ValidTime::new(valid_start),
                valid_end.map(ValidTime::new),
            )
            .expect("valid assertion interval"),
            confidence: Confidence::new(confidence).expect("valid confidence"),
            source_ids: vec![source_id.clone()],
            context,
        }),
    );
}

fn execute(log: &mut EventLog, command: GraphCommand) {
    log.execute(command)
        .expect("synthetic benchmark command is valid");
}

fn person_id(prefix: &str, index: usize) -> EntityId {
    EntityId::new(format!("{prefix}-{index:06}"))
}

fn company_id(prefix: &str, index: usize) -> EntityId {
    EntityId::new(format!("{prefix}-{index:06}"))
}

fn ownership_filler_subject(index: usize, entity_count: usize) -> usize {
    if entity_count <= 3 {
        2
    } else {
        3 + (index - 2) % (entity_count - 3)
    }
}

fn memory_type(index: usize) -> MemoryType {
    match index % 9 {
        0 => MemoryType::Observation,
        1 => MemoryType::Decision,
        2 => MemoryType::Action,
        3 => MemoryType::ToolCall,
        4 => MemoryType::Outcome,
        5 => MemoryType::Preference,
        6 => MemoryType::Plan,
        7 => MemoryType::Reflection,
        _ => MemoryType::Correction,
    }
}

fn deterministic_embedding(index: usize) -> Vec<f32> {
    let mut embedding = vec![0.0, 0.0, 0.0, 0.0];
    embedding[index % 4] = 1.0;
    embedding
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_core::{EntityId, MemoryId, PredicateId, ValidTime};
    use rg_query::{PathQuery, QueryEngine};
    use rg_storage::InMemoryStorage;

    #[test]
    fn social_graph_generator_produces_replayable_requested_shape() {
        let graph = social_graph(SyntheticGraphConfig {
            entity_count: 12,
            assertion_count: 24,
            memory_count: 0,
            branch_factor: 3,
        });

        assert_eq!(graph.kind, SyntheticGraphKind::Social);
        assert_eq!(graph.entity_ids.len(), 12);
        assert_eq!(graph.assertion_ids.len(), 24);

        let storage = build_storage(&graph).expect("social graph replays");
        assert_eq!(storage.graph_state().entities.len(), 12);
        assert_eq!(storage.graph_state().assertions.len(), 24);
    }

    #[test]
    fn company_ownership_graph_supports_two_hop_path_queries() {
        let graph = company_ownership_graph(SyntheticGraphConfig {
            entity_count: 10,
            assertion_count: 20,
            memory_count: 0,
            branch_factor: 2,
        });
        let storage = build_storage(&graph).expect("ownership graph replays");
        let engine = QueryEngine::from_storage(storage);

        let paths = engine.execute_path(PathQuery {
            start: graph.anchor_entity.clone(),
            end: Some(graph.terminal_entity.clone()),
            predicates: graph.path_predicates.clone(),
            valid_at: Some(graph.point_in_time.as_i64()),
            max_depth: 2,
            min_confidence: Some(0.5),
        });

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].hops.len(), 2);
    }

    #[test]
    fn supply_chain_graph_contains_overlapping_contradictions() {
        let graph = supply_chain_graph(SyntheticGraphConfig {
            entity_count: 8,
            assertion_count: 16,
            memory_count: 0,
            branch_factor: 2,
        });
        let index = build_temporal_index(&graph).expect("supply graph indexes");

        assert!(!index.contradictions().is_empty());
    }

    #[test]
    fn agent_memory_graph_links_vectors_to_memory_ids() {
        let graph = agent_memory_graph(SyntheticGraphConfig {
            entity_count: 6,
            assertion_count: 0,
            memory_count: 5,
            branch_factor: 2,
        });

        assert_eq!(graph.kind, SyntheticGraphKind::AgentMemory);
        assert_eq!(graph.memory_ids.len(), 5);
        assert_eq!(graph.vector_records.len(), 5);
        assert_eq!(
            graph.vector_records[0].memory_id,
            Some(MemoryId::new("memory-agent-000000"))
        );

        let vectors = build_vector_index(&graph).expect("memory vectors index");
        let hits = rg_ai::VectorIndex::search(&vectors, &[1.0, 0.0, 0.0, 0.0], 1)
            .expect("vector search succeeds");
        assert_eq!(
            hits[0].memory_id,
            Some(MemoryId::new("memory-agent-000000"))
        );
    }

    #[test]
    fn benchmark_targets_capture_single_node_mvp_goals() {
        assert_eq!(SINGLE_NODE_MVP_TARGETS.assertions_loaded, 1_000_000);
        assert_eq!(SINGLE_NODE_MVP_TARGETS.point_query_p95_ms, 100);
        assert_eq!(SINGLE_NODE_MVP_TARGETS.two_hop_traversal_p95_ms, 250);
        assert_eq!(SINGLE_NODE_MVP_TARGETS.replay_events_per_sec, 50_000);
        assert_eq!(
            SINGLE_NODE_MVP_TARGETS.batched_ingest_assertions_per_sec,
            5_000
        );
    }

    #[test]
    fn benchmark_scale_presets_are_explicit() {
        assert_eq!(SyntheticGraphConfig::smoke().assertion_count, 128);
        assert_eq!(SyntheticGraphConfig::standard().assertion_count, 10_000);
        assert_eq!(
            SyntheticGraphConfig::mvp_target().assertion_count,
            1_000_000
        );
    }

    #[test]
    fn production_benchmark_release_gate_requires_10m_50m_and_100m_artifacts() {
        let incomplete_artifact = BenchmarkArtifact {
            scale: ProductionScaleEnvelope::GeneralProduction100M,
            commit_sha: "abc".to_owned(),
            image_digest: "not-a-digest".to_owned(),
            hardware_profile: String::new(),
            dataset_seed: 42,
            assertion_count: 1_000,
            write_p50_ms: 1.0,
            write_p95_ms: 0.0,
            write_p99_ms: 2.0,
            query_p50_ms: 1.0,
            query_p95_ms: 0.0,
            query_p99_ms: 2.0,
            evidence_pack_p95_ms: 0.0,
            replay_time_ms: 1,
            restore_time_ms: 0,
            rss_mb: 0,
            disk_amplification: 0.0,
            compaction_pause_ms: 0,
        };
        assert!(incomplete_artifact
            .missing_release_fields()
            .contains(&"assertion_count"));
        assert!(!incomplete_artifact.passes_release_gate());

        let artifacts = [
            ProductionScaleEnvelope::PrivateBeta10M,
            ProductionScaleEnvelope::PaidPilot50M,
            ProductionScaleEnvelope::GeneralProduction100M,
        ]
        .into_iter()
        .map(|scale| BenchmarkArtifact {
            scale,
            commit_sha: "abcdef1".to_owned(),
            image_digest: "sha256:benchmark-image".to_owned(),
            hardware_profile: "c7i.4xlarge-32gb".to_owned(),
            dataset_seed: 42,
            assertion_count: scale.required_assertions(),
            write_p50_ms: 10.0,
            write_p95_ms: 20.0,
            write_p99_ms: 40.0,
            query_p50_ms: 5.0,
            query_p95_ms: 30.0,
            query_p99_ms: 60.0,
            evidence_pack_p95_ms: 250.0,
            replay_time_ms: 1_000,
            restore_time_ms: 2_000,
            rss_mb: 8_192,
            disk_amplification: 1.4,
            compaction_pause_ms: 100,
        })
        .collect();
        let release = BenchmarkReleaseSet { artifacts };

        assert!(release.passes_production_release_gate());
        assert!(release.missing_scales().is_empty());
    }

    #[test]
    fn graph_helpers_are_deterministic() {
        let config = SyntheticGraphConfig::standard();

        let first = social_graph(config);
        let second = social_graph(config);

        assert_eq!(first.events, second.events);
        assert_eq!(first.anchor_entity, EntityId::new("person-000000"));
        assert_eq!(first.point_in_time, ValidTime::new(2_024));
        assert_eq!(first.path_predicates, vec![PredicateId::new("knows")]);
        assert!(workspace_ready());
        assert!(InMemoryStorage::replay(&first.events).is_ok());
    }
}

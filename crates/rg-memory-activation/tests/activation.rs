use rg_core::{
    AgentId, AgentMemory, AssertionId, Confidence, ContentHash, ContextScope, EntityId, EntityType,
    GraphValue, MemoryId, MemoryStatus, MemoryType, PredicateId, PropertyMap, SourceId,
    TimeInterval, TxTime, ValidTime,
};
use rg_events::{
    AddAssertion, AddSource, CreateEntity, EventLog, GraphCommand, RecordAgentMemory, SourceType,
};
use rg_memory_activation::{
    ActivationGraph, ActivationNode, ActivationSeed, AgentSpecificMemoryProfile,
    PersonalizedRanker, TemporalDecay, TrustWeightedScoring,
};
use rg_storage::InMemoryStorage;

#[test]
fn activation_graph_links_entities_assertions_sources_and_agent_memories() {
    let storage = fixture_storage();
    let graph = ActivationGraph::from_storage(&storage);

    assert_eq!(graph.entity_count(), 4);
    assert_eq!(graph.assertion_count(), 3);
    assert_eq!(graph.source_count(), 4);
    assert_eq!(graph.memory_count(), 6);
    assert!(graph
        .neighbors(&ActivationNode::Entity(EntityId::new("person-a")))
        .contains(&ActivationNode::Assertion(AssertionId::new(
            "assertion-person-company"
        ))));
    assert!(graph
        .neighbors(&ActivationNode::Memory(MemoryId::new(
            "memory-project-risk"
        )))
        .contains(&ActivationNode::Entity(EntityId::new("project-x"))));
    assert!(graph
        .neighbors(&ActivationNode::Source(SourceId::new("source-memory-high")))
        .contains(&ActivationNode::Memory(MemoryId::new(
            "memory-project-risk"
        ))));
}

#[test]
fn multi_hop_memory_activation_improves_over_vector_only_search() {
    let storage = fixture_storage();
    let graph = ActivationGraph::from_storage(&storage);
    let seed = ActivationSeed {
        query: "What should I remember from Person A's work network?".to_owned(),
        agent_id: Some(AgentId::new("agent-1")),
        entity_ids: vec![EntityId::new("person-a")],
        memory_ids: Vec::new(),
        valid_at: Some(ValidTime::new(20260101)),
        include_superseded: true,
        limit: Some(3),
    };
    let ranker = PersonalizedRanker::default();

    let activated = ranker.activate(
        &graph,
        &seed,
        &AgentSpecificMemoryProfile::for_agent("agent-1"),
    );
    let baseline = ranker.vector_only(&graph, &seed, 1);

    assert_eq!(
        activated.activated_memories[0].memory.id,
        MemoryId::new("memory-project-risk")
    );
    assert_ne!(
        baseline.activated_memories[0].memory.id,
        MemoryId::new("memory-project-risk")
    );
    assert!(activated
        .activated_entities
        .iter()
        .any(|entity| entity.entity.id == EntityId::new("project-x")));
}

#[test]
fn activated_memories_explain_paths_from_seed_to_memory() {
    let storage = fixture_storage();
    let graph = ActivationGraph::from_storage(&storage);
    let seed = ActivationSeed {
        query: "What should I remember from Person A's work network?".to_owned(),
        agent_id: Some(AgentId::new("agent-1")),
        entity_ids: vec![EntityId::new("person-a")],
        memory_ids: Vec::new(),
        valid_at: Some(ValidTime::new(20260101)),
        include_superseded: true,
        limit: Some(3),
    };

    let activated = PersonalizedRanker::default().activate(
        &graph,
        &seed,
        &AgentSpecificMemoryProfile::for_agent("agent-1"),
    );
    let memory = activated
        .memory(&MemoryId::new("memory-project-risk"))
        .expect("project risk memory activated");

    assert!(memory.explanation.contains("person-a"));
    assert!(memory.explanation.contains("project-x"));
    assert!(memory.current_truth);
    assert!(memory.paths.iter().any(|path| {
        path.nodes
            .contains(&ActivationNode::Entity(EntityId::new("person-a")))
            && path.nodes.contains(&ActivationNode::Memory(MemoryId::new(
                "memory-project-risk",
            )))
    }));
}

#[test]
fn old_memories_decay_unless_reinforced_by_agent_profile() {
    let storage = fixture_storage();
    let graph = ActivationGraph::from_storage(&storage);
    let seed = ActivationSeed {
        query: "What API rollout memory matters for Company B?".to_owned(),
        agent_id: Some(AgentId::new("agent-1")),
        entity_ids: vec![EntityId::new("company-b")],
        memory_ids: Vec::new(),
        valid_at: Some(ValidTime::new(20260101)),
        include_superseded: true,
        limit: Some(5),
    };
    let ranker = PersonalizedRanker::default();

    let normal = ranker.activate(
        &graph,
        &seed,
        &AgentSpecificMemoryProfile::for_agent("agent-1"),
    );
    assert!(
        normal.memory_score("memory-recent-api") > normal.memory_score("memory-old-api"),
        "recent memory should outrank old memory without reinforcement"
    );

    let mut reinforced = AgentSpecificMemoryProfile::for_agent("agent-1");
    reinforced.reinforced_memory_ids = vec![MemoryId::new("memory-old-api")];
    let reinforced_result = ranker.activate(&graph, &seed, &reinforced);
    assert!(
        reinforced_result.memory_score("memory-old-api")
            > reinforced_result.memory_score("memory-recent-api"),
        "reinforced old memory should overcome decay"
    );
}

#[test]
fn superseded_memories_remain_visible_but_not_current_truth() {
    let storage = fixture_storage();
    let graph = ActivationGraph::from_storage(&storage);
    let seed = ActivationSeed {
        query: "What rollout preference did Agent 1 have for Company B?".to_owned(),
        agent_id: Some(AgentId::new("agent-1")),
        entity_ids: vec![EntityId::new("company-b")],
        memory_ids: Vec::new(),
        valid_at: Some(ValidTime::new(20260101)),
        include_superseded: true,
        limit: Some(6),
    };

    let activated = PersonalizedRanker::default().activate(
        &graph,
        &seed,
        &AgentSpecificMemoryProfile::for_agent("agent-1"),
    );
    let superseded = activated
        .memory(&MemoryId::new("memory-manual-rollout"))
        .expect("superseded memory is visible");
    let correction = activated
        .memory(&MemoryId::new("memory-blue-green-correction"))
        .expect("correction memory is visible");

    assert!(!superseded.current_truth);
    assert!(correction.current_truth);
    assert!(correction.score > superseded.score);
    assert!(superseded.explanation.contains("superseded"));
}

#[test]
fn temporal_decay_and_source_trust_weights_are_public_policy_objects() {
    let decay = TemporalDecay {
        half_life_days: 365,
        now: ValidTime::new(20260101),
    };
    assert!(decay.weight(ValidTime::new(20250101)) > decay.weight(ValidTime::new(20200101)));

    let trust = TrustWeightedScoring::default();
    assert!(trust.source_weight(Some(0.95)) > trust.source_weight(Some(0.25)));
    assert!(trust.source_weight(None) > 0.0);
}

fn fixture_storage() -> InMemoryStorage {
    let mut log = EventLog::new(TxTime::new(0));
    add_source(&mut log, "source-graph", 0.9);
    add_source(&mut log, "source-memory-high", 0.95);
    add_source(&mut log, "source-memory-low", 0.25);
    add_source(&mut log, "source-correction", 0.98);

    add_entity(&mut log, "person-a", EntityType::Person);
    add_entity(&mut log, "company-b", EntityType::Organization);
    add_entity(&mut log, "project-x", EntityType::Concept);
    add_entity(&mut log, "api-y", EntityType::Concept);

    add_assertion(
        &mut log,
        "assertion-person-company",
        "person-a",
        "WORKED_AT",
        "company-b",
    );
    add_assertion(
        &mut log,
        "assertion-company-project",
        "company-b",
        "RUNS",
        "project-x",
    );
    add_assertion(
        &mut log,
        "assertion-company-api",
        "company-b",
        "OWNS",
        "api-y",
    );

    record_memory(
        &mut log,
        MemorySpec {
            id: "memory-project-risk",
            agent: "agent-1",
            memory_type: MemoryType::Observation,
            content: "Project X has a latent rollback blocker.",
            related_entities: vec!["project-x"],
            source: "source-memory-high",
            valid_from: 20250101,
            confidence: 0.96,
            supersedes: Vec::new(),
        },
    );
    record_memory(
        &mut log,
        MemorySpec {
            id: "memory-person-dashboard",
            agent: "agent-1",
            memory_type: MemoryType::Observation,
            content: "Person A likes dashboard summaries.",
            related_entities: vec!["person-a"],
            source: "source-memory-low",
            valid_from: 20250101,
            confidence: 0.65,
            supersedes: Vec::new(),
        },
    );
    record_memory(
        &mut log,
        MemorySpec {
            id: "memory-old-api",
            agent: "agent-1",
            memory_type: MemoryType::Plan,
            content: "API Y rollout relied on an old fallback path.",
            related_entities: vec!["api-y"],
            source: "source-memory-high",
            valid_from: 20200101,
            confidence: 0.9,
            supersedes: Vec::new(),
        },
    );
    record_memory(
        &mut log,
        MemorySpec {
            id: "memory-recent-api",
            agent: "agent-1",
            memory_type: MemoryType::Plan,
            content: "API Y rollout now uses the current canary path.",
            related_entities: vec!["api-y"],
            source: "source-memory-high",
            valid_from: 20250101,
            confidence: 0.84,
            supersedes: Vec::new(),
        },
    );
    record_memory(
        &mut log,
        MemorySpec {
            id: "memory-manual-rollout",
            agent: "agent-1",
            memory_type: MemoryType::Preference,
            content: "Company B rollout preference was manual approval.",
            related_entities: vec!["company-b"],
            source: "source-memory-low",
            valid_from: 20240101,
            confidence: 0.78,
            supersedes: Vec::new(),
        },
    );
    record_memory(
        &mut log,
        MemorySpec {
            id: "memory-blue-green-correction",
            agent: "agent-1",
            memory_type: MemoryType::Correction,
            content: "Company B rollout preference is blue-green deployment.",
            related_entities: vec!["company-b"],
            source: "source-correction",
            valid_from: 20250101,
            confidence: 0.94,
            supersedes: vec!["memory-manual-rollout"],
        },
    );

    InMemoryStorage::replay(log.events()).expect("fixture storage")
}

fn add_source(log: &mut EventLog, id: &str, trust_score: f32) {
    log.execute(GraphCommand::AddSource(AddSource {
        id: SourceId::new(id),
        source_type: SourceType::Document,
        uri: Some(format!("file://{id}.md")),
        content_hash: ContentHash::new(format!("sha256:{id}")),
        trust_score: Some(trust_score),
    }))
    .expect("source added");
}

fn add_entity(log: &mut EventLog, id: &str, entity_type: EntityType) {
    log.execute(GraphCommand::CreateEntity(CreateEntity {
        id: EntityId::new(id),
        entity_type,
        canonical_name: Some(id.replace('-', " ")),
        properties: PropertyMap::default(),
    }))
    .expect("entity created");
}

fn add_assertion(log: &mut EventLog, id: &str, subject: &str, predicate: &str, object: &str) {
    log.execute(GraphCommand::AddAssertion(AddAssertion {
        id: AssertionId::new(id),
        subject: EntityId::new(subject),
        predicate: PredicateId::new(predicate),
        object: GraphValue::Entity(EntityId::new(object)),
        valid_time: TimeInterval::new(ValidTime::new(20200101), None).expect("valid interval"),
        confidence: Confidence::new(0.9).expect("confidence"),
        source_ids: vec![SourceId::new("source-graph")],
        context: ContextScope::Global,
    }))
    .expect("assertion added");
}

struct MemorySpec<'a> {
    id: &'a str,
    agent: &'a str,
    memory_type: MemoryType,
    content: &'a str,
    related_entities: Vec<&'a str>,
    source: &'a str,
    valid_from: i64,
    confidence: f32,
    supersedes: Vec<&'a str>,
}

fn record_memory(log: &mut EventLog, spec: MemorySpec<'_>) {
    log.execute(GraphCommand::RecordAgentMemory(RecordAgentMemory {
        memory: AgentMemory {
            id: MemoryId::new(spec.id),
            agent_id: AgentId::new(spec.agent),
            memory_type: spec.memory_type,
            content: spec.content.to_owned(),
            valid_time: TimeInterval::new(ValidTime::new(spec.valid_from), None)
                .expect("memory interval"),
            confidence: Confidence::new(spec.confidence).expect("confidence"),
            source_ids: vec![SourceId::new(spec.source)],
            related_entities: spec
                .related_entities
                .into_iter()
                .map(EntityId::new)
                .collect(),
            supersedes: spec.supersedes.into_iter().map(MemoryId::new).collect(),
            status: MemoryStatus::Active,
        },
    }))
    .expect("memory recorded");
}

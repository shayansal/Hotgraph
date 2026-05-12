use rg_agent_memory::{AgentMemoryKind, AgentMemoryService, MemoryPermissions, WriteMemory};
use rg_agent_sim::{ActionSensitivity, ProposedAction};
use rg_causal::{CausalEvent, CausalGraph, CausalLink, CausalRelation, Mechanism};
use rg_core::{
    AgentId, AssertionId, Confidence, ContentHash, ContextScope, EntityId, EntityType, EventId,
    GraphValue, MemoryId, MemoryStatus, PredicateId, SourceId, TenantId, TimeInterval, TxTime,
    ValidTime,
};
use rg_events::{AddAssertion, AddSource, CreateEntity, EventLog, GraphCommand, SourceType};
use rg_governance::{GovernanceEngine, PermissionPolicy, PermissionScope, Principal, PrincipalId};
use rg_reality_api::{
    ContextRequest, ExplainRequest, RealityApi, RealityApiContext, RecallRequest,
    RecommendedProductApi, RememberRequest, StateRequest, VerificationStatus, VerifyRequest,
};
use rg_retrieval_compiler::RetrievalOperator;
use rg_storage::InMemoryStorage;

#[test]
fn product_paths_are_ai_native_not_graph_database_verbs() {
    let paths = RealityApi::endpoint_paths();

    assert_eq!(
        paths,
        [
            "/remember",
            "/recall",
            "/verify",
            "/explain",
            "/timeline",
            "/simulate",
            "/context",
            "/contradictions",
            "/state",
        ]
    );
    assert!(paths.iter().all(|path| !path.contains("query_graph")));
    assert!(RealityApi::recommended_product_api().contains(&RecommendedProductApi::Memory));
}

#[test]
fn remember_and_recall_are_source_backed_and_use_retrieval_compiler() {
    let mut api = fixture_api();

    let remembered = api
        .remember(RememberRequest {
            agent_id: agent(),
            memory_id: MemoryId::new("memory-preference"),
            memory_type: AgentMemoryKind::Preference,
            content: "Person A prefers concise renewal summaries with evidence.".to_owned(),
            source_ids: vec![SourceId::new("source-memory")],
            related_entities: vec![EntityId::new("person-a")],
            confidence: Confidence::new(0.91).expect("confidence"),
            valid_time: TimeInterval::new(ValidTime::new(20260501), None).expect("time"),
        })
        .expect("memory is stored");

    assert!(remembered.evidence_backed);
    assert_eq!(
        remembered.data.memory_id,
        MemoryId::new("memory-preference")
    );
    assert!(remembered
        .retrieval_plan
        .operators
        .contains(&RetrievalOperator::Cite));

    let recalled = api.recall(RecallRequest {
        agent_id: agent(),
        task: "Prepare a renewal summary for Person A.".to_owned(),
        related_entities: vec![EntityId::new("person-a")],
        limit: Some(5),
    });

    assert!(recalled.evidence_backed);
    assert!(!recalled.data.memories.is_empty());
    assert!(recalled
        .data
        .memories
        .iter()
        .any(|memory| memory.id == MemoryId::new("memory-preference")));
    assert!(!recalled.retrieval_trace.steps.is_empty());
}

#[test]
fn verify_explain_context_and_state_return_evidence_backed_answers() {
    let api = fixture_api();

    let verified = api.verify(VerifyRequest {
        claim: "Person A worked at Company B in 2024.".to_owned(),
        subject: Some(EntityId::new("person-a")),
        predicate: Some(PredicateId::new("WORKED_AT")),
        valid_at: Some(ValidTime::new(20240101)),
        known_at: Some(TxTime::new(20260511)),
    });
    assert_eq!(verified.data.status, VerificationStatus::Supported);
    assert!(verified.evidence_backed);
    assert!(!verified.data.supporting_assertions.is_empty());

    let explained = api.explain(ExplainRequest {
        question: "Why do we think Person A worked at Company B?".to_owned(),
        entity_id: Some(EntityId::new("person-a")),
        memory_id: None,
    });
    assert!(explained.evidence_backed);
    assert!(explained.data.explanation.contains("evidence"));

    let context = api.context(ContextRequest {
        question: "Build context for Person A employment.".to_owned(),
        agent_id: Some(agent()),
        entity_ids: vec![EntityId::new("person-a")],
        max_evidence_items: Some(6),
    });
    assert!(context.evidence_backed);
    assert!(!context.data.context_pack.sources.is_empty());

    let state = api.state(StateRequest {
        question: "What was true about Person A in 2024?".to_owned(),
        entity_id: Some(EntityId::new("person-a")),
        valid_at: Some(ValidTime::new(20240101)),
        known_at: Some(TxTime::new(20260511)),
    });
    assert!(state.evidence_backed);
    assert!(state
        .data
        .assertions
        .iter()
        .any(|assertion| assertion.assertion_id == AssertionId::new("assertion-worked-at")));
}

#[test]
fn timeline_contradictions_and_simulate_feel_like_reasoning_infrastructure() {
    let api = fixture_api();

    let timeline = api.timeline(rg_reality_api::TimelineRequest {
        entity_id: EntityId::new("person-a"),
        valid_at: Some(ValidTime::new(20240101)),
        known_at: Some(TxTime::new(20260511)),
    });
    assert!(timeline.evidence_backed);
    assert!(timeline.data.items.len() >= 3);
    assert!(timeline.data.items[0].label.contains("Person A"));

    let contradictions = api.contradictions(rg_reality_api::ContradictionsRequest {
        question: "Are there unresolved conflicts about Person A?".to_owned(),
    });
    assert!(contradictions.evidence_backed);
    assert!(!contradictions.data.unresolved.is_empty());

    let simulated = api.simulate(rg_reality_api::SimulateRequest {
        action: ProposedAction {
            id: "action-email-person-a".to_owned(),
            actor_agent_id: agent(),
            description: "Email Person A about renewal timing.".to_owned(),
            action_type: "email_customer".to_owned(),
            target_entities: vec![EntityId::new("person-a")],
            related_event: Some(EventId::new("event-email-sent")),
            required_source_ids: vec![SourceId::new("source-memory")],
            sensitivity: ActionSensitivity::Medium,
        },
    });

    assert!(simulated.evidence_backed);
    assert!(simulated.data.prediction_not_fact);
    assert!(!simulated.data.outcomes.is_empty());
    assert!(simulated
        .safety_notes
        .iter()
        .any(|note| note.contains("prediction")));
}

fn fixture_api() -> RealityApi {
    let storage = fixture_storage();
    let mut memory_service = AgentMemoryService::new(TxTime::new(20260512));
    memory_service
        .write_memory(WriteMemory {
            id: MemoryId::new("memory-open-commitment"),
            agent_id: agent(),
            memory_type: AgentMemoryKind::Plan,
            content: "Person A has an open renewal commitment that needs evidence.".to_owned(),
            valid_time: TimeInterval::new(ValidTime::new(20260501), None).expect("memory time"),
            confidence: Confidence::new(0.9).expect("confidence"),
            source_ids: vec![SourceId::new("source-memory")],
            related_entities: vec![EntityId::new("person-a")],
            supersedes: Vec::new(),
            contradicts: Vec::new(),
            lifecycle: MemoryStatus::Active,
            permissions: MemoryPermissions::private(agent()),
        })
        .expect("memory written");

    RealityApi::new(RealityApiContext {
        storage,
        memory_service,
        causal_graph: causal_graph(),
        governance: GovernanceEngine::new(
            PermissionPolicy::new(TenantId::new("tenant-a"))
                .allow_read(PermissionScope::Tenant(TenantId::new("tenant-a"))),
        ),
        principal: Principal {
            id: PrincipalId::new("agent-principal"),
            tenant_id: TenantId::new("tenant-a"),
            agent_id: Some(agent()),
        },
        valid_at: ValidTime::new(20260512),
        known_at: TxTime::new(20260512),
    })
}

fn fixture_storage() -> InMemoryStorage {
    let mut log = EventLog::new(TxTime::new(0));
    add_source(&mut log, "source-employment", "employment");
    add_source(&mut log, "source-conflict", "conflict");
    add_source(&mut log, "source-memory", "memory");
    add_entity(&mut log, "person-a", EntityType::Person, "Person A");
    add_entity(&mut log, "company-b", EntityType::Organization, "Company B");
    add_entity(&mut log, "company-c", EntityType::Organization, "Company C");
    add_assertion(
        &mut log,
        AssertionFixture {
            id: "assertion-worked-at",
            subject: "person-a",
            predicate: "WORKED_AT",
            object: GraphValue::Entity(EntityId::new("company-b")),
            source: "source-employment",
            confidence: 0.94,
        },
    );
    add_assertion(
        &mut log,
        AssertionFixture {
            id: "assertion-ceo-b",
            subject: "person-a",
            predicate: "CEO_OF",
            object: GraphValue::Entity(EntityId::new("company-b")),
            source: "source-employment",
            confidence: 0.86,
        },
    );
    add_assertion(
        &mut log,
        AssertionFixture {
            id: "assertion-ceo-c",
            subject: "person-a",
            predicate: "CEO_OF",
            object: GraphValue::Entity(EntityId::new("company-c")),
            source: "source-conflict",
            confidence: 0.82,
        },
    );
    InMemoryStorage::replay(log.events()).expect("storage replays")
}

fn causal_graph() -> CausalGraph {
    let mut graph = CausalGraph::new();
    graph.insert_event(CausalEvent {
        id: EventId::new("event-email-sent"),
        description: "Agent sends a renewal email.".to_owned(),
        occurred_at: None,
        related_entities: vec![EntityId::new("person-a")],
        related_assertions: vec![AssertionId::new("assertion-worked-at")],
        source_ids: vec![SourceId::new("source-memory")],
        context: ContextScope::Global,
    });
    graph.insert_event(CausalEvent {
        id: EventId::new("event-context-gap"),
        description: "Recipient misunderstands unresolved context.".to_owned(),
        occurred_at: None,
        related_entities: vec![EntityId::new("person-a")],
        related_assertions: vec![AssertionId::new("assertion-ceo-b")],
        source_ids: vec![SourceId::new("source-conflict")],
        context: ContextScope::Global,
    });
    graph.insert_link(CausalLink {
        id: rg_core::CausalLinkId::new("link-email-gap"),
        cause_event: EventId::new("event-email-sent"),
        effect_event: EventId::new("event-context-gap"),
        relation: CausalRelation::Influenced,
        mechanism: Mechanism {
            label: "missing context".to_owned(),
            description: Some(
                "An email before context review can create misunderstanding.".to_owned(),
            ),
        },
        confidence: Confidence::new(0.72).expect("confidence"),
        source_ids: vec![SourceId::new("source-memory")],
        context: ContextScope::Global,
    });
    graph
}

fn add_source(log: &mut EventLog, id: &str, hash: &str) {
    log.execute(GraphCommand::AddSource(AddSource {
        id: SourceId::new(id),
        source_type: SourceType::Document,
        uri: Some(format!("file://{hash}.md")),
        content_hash: ContentHash::new(format!("sha256:{hash}")),
        trust_score: Some(0.9),
    }))
    .expect("source added");
}

fn add_entity(log: &mut EventLog, id: &str, entity_type: EntityType, name: &str) {
    log.execute(GraphCommand::CreateEntity(CreateEntity {
        id: EntityId::new(id),
        entity_type,
        canonical_name: Some(name.to_owned()),
        properties: rg_core::PropertyMap::default(),
    }))
    .expect("entity added");
}

struct AssertionFixture<'a> {
    id: &'a str,
    subject: &'a str,
    predicate: &'a str,
    object: GraphValue,
    source: &'a str,
    confidence: f32,
}

fn add_assertion(log: &mut EventLog, fixture: AssertionFixture<'_>) {
    log.execute(GraphCommand::AddAssertion(AddAssertion {
        id: AssertionId::new(fixture.id),
        subject: EntityId::new(fixture.subject),
        predicate: PredicateId::new(fixture.predicate),
        object: fixture.object,
        valid_time: TimeInterval::new(ValidTime::new(20210101), Some(ValidTime::new(20250101)))
            .expect("valid time"),
        confidence: Confidence::new(fixture.confidence).expect("confidence"),
        source_ids: vec![SourceId::new(fixture.source)],
        context: ContextScope::Global,
    }))
    .expect("assertion added");
}

fn agent() -> AgentId {
    AgentId::new("agent-alpha")
}

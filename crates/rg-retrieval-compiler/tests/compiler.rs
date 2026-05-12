use rg_core::{
    AssertionId, Confidence, ContentHash, ContextScope, EntityId, EntityType, GraphValue,
    PredicateId, PropertyMap, SourceId, TimeInterval, TxTime, ValidTime,
};
use rg_events::{AddAssertion, AddSource, CreateEntity, EventLog, GraphCommand, SourceType};
use rg_retrieval_compiler::{
    AgentState, CompilationRequest, EvidencePackCompiler, QueryIntent, RetrievalBudget,
    RetrievalOperator, RetrievalTool, TrustPolicy,
};
use rg_storage::InMemoryStorage;

#[test]
fn multi_hop_questions_compile_to_path_search_and_beat_vector_only() {
    let storage = fixture_storage();
    let compiler = EvidencePackCompiler::new(storage);
    let request = CompilationRequest {
        question: "How does Company A control Company C through ownership?".to_owned(),
        temporal_constraints: Some((20240101, 20260511)),
        budget: RetrievalBudget {
            max_latency_micros: 2_000,
            max_cost_units: 5.0,
            max_evidence_items: 8,
            max_path_depth: 3,
        },
        available_tools: RetrievalTool::all(),
        ..CompilationRequest::default()
    };

    let compiled = compiler.compile(request.clone());
    let baseline = compiler.vector_only_baseline(&request);

    assert_eq!(compiled.plan.intent, QueryIntent::MultiHop);
    assert!(compiled
        .plan
        .operators
        .contains(&RetrievalOperator::PathSearch));
    assert!(compiled
        .plan
        .operators
        .contains(&RetrievalOperator::GraphExpansion));
    assert!(compiled.plan.operators.contains(&RetrievalOperator::Cite));
    assert!(compiled.evidence_pack.paths.len() > baseline.paths.len());
    assert!(compiled
        .evidence_pack
        .assertions
        .iter()
        .any(|assertion| assertion.id.as_str() == "assertion-owns-a-b"));
    assert!(compiled
        .evidence_pack
        .assertions
        .iter()
        .any(|assertion| assertion.id.as_str() == "assertion-owns-b-c"));
    assert_trace_covers_plan(&compiled.plan.operators, &compiled.trace);
}

#[test]
fn simple_fact_questions_do_not_lose_to_vector_only_rag() {
    let storage = fixture_storage();
    let compiler = EvidencePackCompiler::new(storage);
    let request = CompilationRequest {
        question: "Where did Person A work?".to_owned(),
        budget: RetrievalBudget::default(),
        available_tools: RetrievalTool::all(),
        ..CompilationRequest::default()
    };

    let compiled = compiler.compile(request.clone());
    let baseline = compiler.vector_only_baseline(&request);

    assert_eq!(compiled.plan.intent, QueryIntent::SimpleFact);
    assert!(compiled.plan.operators.starts_with(&[
        RetrievalOperator::KeywordSearch,
        RetrievalOperator::VectorSearch
    ]));
    assert!(compiled.evidence_pack.assertions.len() >= baseline.assertions.len());
    assert!(compiled
        .evidence_pack
        .assertions
        .iter()
        .any(|assertion| assertion.id.as_str() == "assertion-worked-at"));
    assert!(!compiled.evidence_pack.sources.is_empty());
    assert_trace_covers_plan(&compiled.plan.operators, &compiled.trace);
}

#[test]
fn historical_questions_apply_bitemporal_filtering() {
    let storage = fixture_storage();
    let compiler = EvidencePackCompiler::new(storage);
    let request = CompilationRequest {
        question: "Where did Person A work in 2024?".to_owned(),
        temporal_constraints: Some((20240101, 20260511)),
        trust_policy: TrustPolicy {
            min_confidence: Some(0.8),
            required_source_ids: Vec::new(),
        },
        available_tools: RetrievalTool::all(),
        ..CompilationRequest::default()
    };

    let compiled = compiler.compile(request);

    assert_eq!(compiled.plan.intent, QueryIntent::Historical);
    assert!(compiled
        .plan
        .operators
        .contains(&RetrievalOperator::TemporalFilter));
    assert_eq!(compiled.evidence_pack.assertions.len(), 1);
    assert_eq!(
        compiled.evidence_pack.assertions[0].id.as_str(),
        "assertion-worked-at"
    );
    assert_trace_covers_plan(&compiled.plan.operators, &compiled.trace);
}

#[test]
fn contradiction_questions_retrieve_both_sides_with_citations() {
    let storage = fixture_storage();
    let compiler = EvidencePackCompiler::new(storage);
    let request = CompilationRequest {
        question: "Is there contradictory evidence about Person A being CEO in 2024?".to_owned(),
        temporal_constraints: Some((20240601, 20260511)),
        available_tools: RetrievalTool::all(),
        ..CompilationRequest::default()
    };

    let compiled = compiler.compile(request);

    assert_eq!(compiled.plan.intent, QueryIntent::ContradictoryEvidence);
    assert!(compiled
        .plan
        .operators
        .contains(&RetrievalOperator::ContradictionCheck));
    assert!(!compiled.evidence_pack.contradictions.is_empty());
    assert!(compiled.evidence_pack.sources.len() >= 2);
    assert!(compiled.citation_coverage() >= 1.0);
    assert_trace_covers_plan(&compiled.plan.operators, &compiled.trace);
}

#[test]
fn agent_memory_questions_include_state_and_semantic_memory_operators() {
    let storage = fixture_storage();
    let compiler = EvidencePackCompiler::new(storage);
    let request = CompilationRequest {
        question: "What does Agent Alpha remember about deployment preferences?".to_owned(),
        agent_state: Some(AgentState {
            agent_id: Some("agent-alpha".to_owned()),
            user_id: Some("user-1".to_owned()),
            active_entity_ids: vec!["person-a".to_owned()],
        }),
        available_tools: RetrievalTool::all(),
        ..CompilationRequest::default()
    };

    let compiled = compiler.compile(request);

    assert_eq!(compiled.plan.intent, QueryIntent::AgentMemory);
    assert!(compiled
        .plan
        .operators
        .contains(&RetrievalOperator::VectorSearch));
    assert!(compiled
        .trace
        .steps
        .iter()
        .any(|step| step.reason.contains("agent state")));
    assert_trace_covers_plan(&compiled.plan.operators, &compiled.trace);
}

#[test]
fn broad_global_questions_choose_temporal_community_retrieval() {
    let storage = fixture_storage();
    let compiler = EvidencePackCompiler::new(storage);
    let request = CompilationRequest {
        question: "Give me a broad global overview of the company communities.".to_owned(),
        temporal_constraints: Some((20240101, 20260511)),
        available_tools: RetrievalTool::all(),
        ..CompilationRequest::default()
    };

    let compiled = compiler.compile(request);

    assert_eq!(compiled.plan.intent, QueryIntent::BroadGlobal);
    assert!(compiled
        .plan
        .operators
        .contains(&RetrievalOperator::CommunitySearch));
    assert!(compiled
        .trace
        .steps
        .iter()
        .any(|step| step.operator == RetrievalOperator::CommunitySearch
            && step.reason.contains("temporal community")));
    assert_trace_covers_plan(&compiled.plan.operators, &compiled.trace);
}

fn assert_trace_covers_plan(
    operators: &[RetrievalOperator],
    trace: &rg_retrieval_compiler::RetrievalTrace,
) {
    assert_eq!(trace.steps.len(), operators.len());
    for operator in operators {
        assert!(trace
            .steps
            .iter()
            .any(|step| { &step.operator == operator && !step.reason.trim().is_empty() }));
    }
}

fn fixture_storage() -> InMemoryStorage {
    let mut log = EventLog::new(TxTime::new(0));
    add_source(&mut log, "source-employment", "employment");
    add_source(&mut log, "source-ownership", "ownership");
    add_source(&mut log, "source-ceo-a", "ceo-a");
    add_source(&mut log, "source-ceo-b", "ceo-b");

    add_entity(&mut log, "person-a", EntityType::Person, "Person A");
    add_entity(&mut log, "company-a", EntityType::Organization, "Company A");
    add_entity(&mut log, "company-b", EntityType::Organization, "Company B");
    add_entity(&mut log, "company-c", EntityType::Organization, "Company C");

    add_assertion(
        &mut log,
        "assertion-worked-at",
        "person-a",
        "WORKED_AT",
        "company-b",
        "source-employment",
        20210101,
        Some(20250101),
        0.92,
    );
    add_assertion(
        &mut log,
        "assertion-old-employment",
        "person-a",
        "WORKED_AT",
        "company-c",
        "source-employment",
        20180101,
        Some(20200101),
        0.91,
    );
    add_assertion(
        &mut log,
        "assertion-owns-a-b",
        "company-a",
        "OWNS",
        "company-b",
        "source-ownership",
        20200101,
        None,
        0.9,
    );
    add_assertion(
        &mut log,
        "assertion-owns-b-c",
        "company-b",
        "OWNS",
        "company-c",
        "source-ownership",
        20200101,
        None,
        0.88,
    );
    add_assertion(
        &mut log,
        "assertion-ceo-b",
        "person-a",
        "CEO_OF",
        "company-b",
        "source-ceo-a",
        20240101,
        Some(20250101),
        0.82,
    );
    add_assertion(
        &mut log,
        "assertion-ceo-c",
        "person-a",
        "CEO_OF",
        "company-c",
        "source-ceo-b",
        20240101,
        Some(20250101),
        0.86,
    );

    InMemoryStorage::replay(log.events()).expect("fixture storage")
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
        properties: PropertyMap::default(),
    }))
    .expect("entity created");
}

#[allow(clippy::too_many_arguments)]
fn add_assertion(
    log: &mut EventLog,
    id: &str,
    subject: &str,
    predicate: &str,
    object: &str,
    source: &str,
    valid_from: i64,
    valid_to: Option<i64>,
    confidence: f32,
) {
    log.execute(GraphCommand::AddAssertion(AddAssertion {
        id: AssertionId::new(id),
        subject: EntityId::new(subject),
        predicate: PredicateId::new(predicate),
        object: GraphValue::Entity(EntityId::new(object)),
        valid_time: TimeInterval::new(ValidTime::new(valid_from), valid_to.map(ValidTime::new))
            .expect("valid interval"),
        confidence: Confidence::new(confidence).expect("valid confidence"),
        source_ids: vec![SourceId::new(source)],
        context: ContextScope::Global,
    }))
    .expect("assertion added");
}

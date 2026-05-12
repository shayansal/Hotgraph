use rg_agent_memory::{AgentMemoryKind, AgentMemoryService, MemoryPermissions, WriteMemory};
use rg_agent_sim::{
    ActionSensitivity, AgentSimulationLab, MissingInformationKind, ProposedAction,
    RecommendedActionKind, SimulationContext,
};
use rg_causal::{CausalEvent, CausalGraph, CausalLink, CausalRelation, Mechanism};
use rg_core::{
    AgentId, AssertionId, Confidence, ContentHash, ContextScope, EntityId, EntityType, EventId,
    GraphValue, MemoryId, MemoryStatus, PredicateId, SourceId, TenantId, TimeInterval, TxTime,
    ValidTime,
};
use rg_events::{AddAssertion, AddSource, CreateEntity, EventLog, GraphCommand, SourceType};
use rg_governance::{
    GovernanceEngine, PermissionPolicy, PermissionScope, Principal, PrincipalId, SourceAccessPolicy,
};
use rg_retrieval_compiler::{EvidencePackCompiler, RetrievalOperator};
use rg_storage::InMemoryStorage;

#[test]
fn agents_can_ask_what_happens_if_i_do_x() {
    let lab = AgentSimulationLab::new(simulation_context(false));
    let report = lab.simulate(email_customer_action(ActionSensitivity::Medium));

    assert!(report.prediction_not_fact);
    assert!(report
        .outcomes
        .iter()
        .all(|outcome| outcome.prediction_not_fact));
    assert!(report
        .outcomes
        .iter()
        .any(|outcome| outcome.description.contains("customer confusion")));
    assert!(report
        .affected_entities
        .contains(&EntityId::new("customer-acme")));
    assert!(report
        .causal_paths
        .iter()
        .any(|path| path.event_ids().contains(&EventId::new("event-email-sent"))));
}

#[test]
fn simulation_returns_risks_missing_context_and_recommended_next_action() {
    let lab = AgentSimulationLab::new(simulation_context(false));
    let mut action = email_customer_action(ActionSensitivity::Medium);
    action
        .required_source_ids
        .push(SourceId::new("source-legal"));

    let report = lab.simulate(action);

    assert!(report.risk.score >= 0.5);
    assert!(report
        .risk
        .factors
        .iter()
        .any(|factor| factor.contains("unresolved commitments")));
    assert!(report.missing_information.iter().any(|missing| {
        missing.kind == MissingInformationKind::RequiredSource
            && missing.description.contains("source-legal")
    }));
    assert_eq!(
        report.recommended_action.kind,
        RecommendedActionKind::GatherMissingInformation
    );
    assert!(report
        .evidence_trace
        .operators
        .contains(&RetrievalOperator::KeywordSearch));
}

#[test]
fn high_risk_actions_trigger_policy_checks() {
    let lab = AgentSimulationLab::new(simulation_context(true));
    let report = lab.simulate(email_customer_action(ActionSensitivity::High));

    assert!(report.policy_checked);
    assert!(!report.policy_violations.is_empty());
    assert!(report.policy_violations.iter().any(|violation| {
        violation.resource_id == "source-customer"
            && violation.description.contains("source policy denies")
    }));
    assert!(report.risk.score >= 0.8);
    assert_eq!(
        report.recommended_action.kind,
        RecommendedActionKind::EscalateForReview
    );
}

#[test]
fn low_risk_actions_can_skip_expensive_policy_checks_but_still_return_predictions() {
    let lab = AgentSimulationLab::new(simulation_context(true));
    let report = lab.simulate(email_customer_action(ActionSensitivity::Low));

    assert!(!report.policy_checked);
    assert!(report.policy_violations.is_empty());
    assert!(report.prediction_not_fact);
    assert_eq!(
        report.recommended_action.kind,
        RecommendedActionKind::ProceedWithCaution
    );
}

fn simulation_context(restrict_customer_source: bool) -> SimulationContext {
    let storage = fixture_storage();
    let graph_state = storage.graph_state();
    let mut memory = AgentMemoryService::new(TxTime::new(20260512));
    memory
        .write_memory(WriteMemory {
            id: MemoryId::new("memory-commitment"),
            agent_id: AgentId::new("agent-sales"),
            memory_type: AgentMemoryKind::Plan,
            content: "ACME asked not to receive pricing promises until legal approves the DPA."
                .to_owned(),
            valid_time: TimeInterval::new(ValidTime::new(20260501), None).expect("memory time"),
            confidence: Confidence::new(0.95).expect("confidence"),
            source_ids: vec![SourceId::new("source-customer")],
            related_entities: vec![EntityId::new("customer-acme")],
            supersedes: Vec::new(),
            contradicts: Vec::new(),
            lifecycle: MemoryStatus::Active,
            permissions: MemoryPermissions::private(AgentId::new("agent-sales")),
        })
        .expect("memory is written");

    let mut policy = PermissionPolicy::new(TenantId::new("tenant-a"))
        .allow_read(PermissionScope::Tenant(TenantId::new("tenant-a")));
    if restrict_customer_source {
        policy = policy.with_source_policy(SourceAccessPolicy::restricted(
            SourceId::new("source-customer"),
            vec![PrincipalId::new("legal-reviewer")],
        ));
    }

    SimulationContext {
        current_state_label: "tenant-a customer success graph at 2026-05-12".to_owned(),
        valid_at: ValidTime::new(20260512),
        known_at: TxTime::new(20260512),
        principal: Principal {
            id: PrincipalId::new("sales-agent-principal"),
            tenant_id: TenantId::new("tenant-a"),
            agent_id: Some(AgentId::new("agent-sales")),
        },
        causal_graph: causal_graph(),
        graph_state: graph_state.clone(),
        memory_service: memory,
        governance: GovernanceEngine::new(policy),
        evidence_compiler: EvidencePackCompiler::new(storage),
    }
}

fn email_customer_action(sensitivity: ActionSensitivity) -> ProposedAction {
    ProposedAction {
        id: "action-email-acme".to_owned(),
        actor_agent_id: AgentId::new("agent-sales"),
        description:
            "Email ACME customer about renewal pricing, implementation timing, and the DPA."
                .to_owned(),
        action_type: "email_customer".to_owned(),
        target_entities: vec![EntityId::new("customer-acme")],
        related_event: Some(EventId::new("event-email-sent")),
        required_source_ids: vec![SourceId::new("source-customer")],
        sensitivity,
    }
}

fn fixture_storage() -> InMemoryStorage {
    let mut log = EventLog::new(TxTime::new(0));
    add_source(&mut log, "source-customer", "acme-customer");
    add_source(&mut log, "source-task", "open-task");
    add_entity(&mut log, "agent-sales", EntityType::Person, "Sales Agent");
    add_entity(&mut log, "customer-acme", EntityType::Organization, "ACME");
    add_entity(&mut log, "legal-team", EntityType::Organization, "Legal");
    add_assertion(
        &mut log,
        "assertion-relationship",
        "agent-sales",
        "MANAGES_RELATIONSHIP",
        "customer-acme",
        "source-customer",
        0.93,
    );
    add_assertion(
        &mut log,
        "assertion-open-task",
        "customer-acme",
        "HAS_OPEN_TASK",
        "legal-team",
        "source-task",
        0.89,
    );
    InMemoryStorage::replay(log.events()).expect("storage replays")
}

fn causal_graph() -> CausalGraph {
    let mut graph = CausalGraph::new();
    graph.insert_event(CausalEvent {
        id: EventId::new("event-email-sent"),
        description: "Agent sends customer email with renewal claims.".to_owned(),
        occurred_at: None,
        related_entities: vec![EntityId::new("customer-acme")],
        related_assertions: vec![AssertionId::new("assertion-relationship")],
        source_ids: vec![SourceId::new("source-customer")],
        context: tenant(),
    });
    graph.insert_event(CausalEvent {
        id: EventId::new("event-customer-confusion"),
        description: "Customer misunderstands legal approval status.".to_owned(),
        occurred_at: None,
        related_entities: vec![EntityId::new("customer-acme"), EntityId::new("legal-team")],
        related_assertions: vec![AssertionId::new("assertion-open-task")],
        source_ids: vec![SourceId::new("source-task")],
        context: tenant(),
    });
    graph.insert_link(CausalLink {
        id: rg_core::CausalLinkId::new("link-email-confusion"),
        cause_event: EventId::new("event-email-sent"),
        effect_event: EventId::new("event-customer-confusion"),
        relation: CausalRelation::Influenced,
        mechanism: Mechanism {
            label: "ambiguous legal status".to_owned(),
            description: Some(
                "Email before DPA approval can create customer confusion.".to_owned(),
            ),
        },
        confidence: Confidence::new(0.74).expect("confidence"),
        source_ids: vec![SourceId::new("source-customer")],
        context: tenant(),
    });
    graph
}

fn add_source(log: &mut EventLog, id: &str, hash: &str) {
    log.execute(GraphCommand::AddSource(AddSource {
        id: SourceId::new(id),
        source_type: SourceType::Document,
        uri: Some(format!("file://{hash}.md")),
        content_hash: ContentHash::new(format!("sha256:{hash}")),
        trust_score: Some(0.91),
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

fn add_assertion(
    log: &mut EventLog,
    id: &str,
    subject: &str,
    predicate: &str,
    object: &str,
    source: &str,
    confidence: f32,
) {
    log.execute(GraphCommand::AddAssertion(AddAssertion {
        id: AssertionId::new(id),
        subject: EntityId::new(subject),
        predicate: PredicateId::new(predicate),
        object: GraphValue::Entity(EntityId::new(object)),
        valid_time: TimeInterval::new(ValidTime::new(20250101), None).expect("valid time"),
        confidence: Confidence::new(confidence).expect("confidence"),
        source_ids: vec![SourceId::new(source)],
        context: tenant(),
    }))
    .expect("assertion added");
}

fn tenant() -> ContextScope {
    ContextScope::Named("tenant:tenant-a".to_owned())
}

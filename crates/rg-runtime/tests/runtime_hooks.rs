use rg_agent_memory::AgentMemoryService;
use rg_belief::{Claim, ClaimId};
use rg_core::{
    AgentId, AssertionId, Confidence, ContextScope, EntityId, GraphValue, MemoryId, PredicateId,
    SourceId, TimeInterval, TxTime, ValidTime,
};
use rg_events::{
    AddAssertion, AddSource, CreateEntity, EntityType, EventLog, GraphCommand, SourceType,
};
use rg_retrieval_compiler::RetrievalOperator;
use rg_runtime::{
    AgentActionOutcome, AgentLoopState, BeliefObservation, IntegrationKind, ModelRuntimeBridge,
    RuntimeIntegrationCatalog, RuntimePhase, RuntimeProfile, SpeculativeDecodeHint,
};
use rg_storage::InMemoryStorage;

#[test]
fn prefill_context_pack_injects_source_backed_context_before_attention() {
    let bridge = fixture_bridge();
    let prefill = bridge.prefill_context_pack(
        "Who did Person A work for in 2024?",
        RuntimeProfile::prefill("open-source-inference-server"),
    );

    assert_eq!(prefill.phase, RuntimePhase::PreAttentionContextInjection);
    assert!(prefill.prompt_prefix.contains("Reality Graph context pack"));
    assert!(!prefill.context_pack.assertions.is_empty());
    assert_eq!(prefill.citation_coverage, 1.0);
    assert!(prefill
        .recommended_integration
        .contains("open-source inference server"));
}

#[test]
fn refresh_context_during_agent_loop_uses_long_context_refresh_hooks() {
    let bridge = fixture_bridge();
    let mut state = AgentLoopState::new(AgentId::new("agent-1"), "Investigate employment")
        .with_turn(12)
        .with_active_entity(EntityId::new("person-a"));

    let refresh = bridge.refresh_context_during_agent_loop(&mut state);

    assert_eq!(refresh.phase, RuntimePhase::LongContextRefresh);
    assert_eq!(state.last_refresh_turn, Some(12));
    assert!(refresh.context_delta.token_estimate > 0);
    assert!(refresh
        .hook_trace
        .iter()
        .any(|step| step.contains("agent loop")));
}

#[test]
fn retrieve_before_tool_choice_biases_tools_with_evidence_and_speculative_hint() {
    let bridge = fixture_bridge();
    let decision = bridge.retrieve_before_tool_choice(
        "Should I verify or write memory about Person A?",
        vec!["verify_claim", "write_memory", "send_email"],
    );

    assert_eq!(decision.phase, RuntimePhase::RetrievalDuringDecoding);
    assert_eq!(decision.selected_tool, Some("verify_claim".to_owned()));
    assert_eq!(
        decision.speculative_decode_hint,
        SpeculativeDecodeHint::PreferEvidenceGathering
    );
    assert!(decision
        .plan_operators
        .contains(&RetrievalOperator::TemporalFilter));
}

#[test]
fn verify_before_final_answer_blocks_unsupported_final_answers() {
    let bridge = fixture_bridge();

    let supported = bridge.verify_before_final_answer(
        "Person A worked at Company B in 2024.",
        Some(AssertionId::new("assertion-worked-at")),
    );
    let unsupported = bridge.verify_before_final_answer("Person A worked at Company Z.", None);

    assert!(supported.allowed_to_answer);
    assert!(!unsupported.allowed_to_answer);
    assert!(unsupported
        .final_answer_guardrail
        .contains("insufficient evidence"));
}

#[test]
fn write_memory_after_action_records_agent_loop_memory_with_provenance() {
    let mut bridge = fixture_bridge();
    let record = bridge
        .write_memory_after_action(AgentActionOutcome {
            memory_id: MemoryId::new("memory-after-action"),
            agent_id: AgentId::new("agent-1"),
            content: "Verified Person A employment before answering.".to_owned(),
            source_ids: vec![SourceId::new("source-employment")],
            related_entities: vec![EntityId::new("person-a")],
            valid_at: ValidTime::new(2024),
        })
        .expect("memory write");

    assert_eq!(record.phase, RuntimePhase::AgentLoopMemoryHook);
    assert_eq!(record.memory.id, MemoryId::new("memory-after-action"));
    assert_eq!(
        record.memory.source_ids,
        vec![SourceId::new("source-employment")]
    );
    assert!(record
        .hook_trace
        .iter()
        .any(|step| step.contains("action outcome")));
}

#[test]
fn update_belief_after_observation_records_claim_and_external_belief_state_cache() {
    let mut bridge = fixture_bridge();
    let update = bridge.update_belief_after_observation(BeliefObservation {
        claim: Claim {
            id: ClaimId::new("claim-observed-work"),
            subject: EntityId::new("person-a"),
            predicate: PredicateId::new("worked_at"),
            object: GraphValue::Entity(EntityId::new("company-b")),
            valid_time: TimeInterval::new(ValidTime::new(2024), None).expect("valid interval"),
            transaction_time: TxTime::new(30),
            confidence: Confidence::new(0.95).expect("confidence"),
            source_ids: vec![SourceId::new("source-employment")],
            evidence: "Observed in employment source.".to_owned(),
        },
        valid_at: ValidTime::new(2024),
        known_at: TxTime::new(30),
    });

    assert_eq!(update.phase, RuntimePhase::ExternalBeliefStateCache);
    assert_eq!(update.cache_key, "person-a|worked_at|2024|30");
    assert!(update.belief_state.preferred_claim.is_some());
    assert!(update
        .hook_trace
        .iter()
        .any(|step| step.contains("belief-state cache")));
}

#[test]
fn runtime_integration_catalog_covers_requested_reference_targets() {
    let catalog = RuntimeIntegrationCatalog::default_catalog();

    assert!(catalog
        .integrations_for(IntegrationKind::OpenSourceInferenceServer)
        .iter()
        .any(|integration| integration.name == "vLLM/TGI prefill adapter"));
    assert!(catalog
        .integrations_for(IntegrationKind::LocalAgentRuntime)
        .iter()
        .any(|integration| integration.name == "LangGraph-style loop hooks"));
    assert!(catalog
        .integrations_for(IntegrationKind::ResearchNotebook)
        .iter()
        .any(|integration| integration.name == "Notebook replay harness"));
}

fn fixture_bridge() -> ModelRuntimeBridge {
    let storage = fixture_storage();
    let memory_service = AgentMemoryService::new(TxTime::new(0));
    ModelRuntimeBridge::new(storage, memory_service)
}

fn fixture_storage() -> InMemoryStorage {
    let mut log = EventLog::new(TxTime::new(0));
    log.execute(GraphCommand::AddSource(AddSource {
        id: SourceId::new("source-employment"),
        source_type: SourceType::Document,
        uri: Some("file://employment.md".to_owned()),
        content_hash: rg_core::ContentHash::new("sha256:employment"),
        trust_score: Some(0.95),
    }))
    .expect("source added");
    for (id, entity_type, name) in [
        ("person-a", EntityType::Person, "Person A"),
        ("company-b", EntityType::Organization, "Company B"),
    ] {
        log.execute(GraphCommand::CreateEntity(CreateEntity {
            id: EntityId::new(id),
            entity_type,
            canonical_name: Some(name.to_owned()),
            properties: rg_core::PropertyMap::default(),
        }))
        .expect("entity added");
    }
    log.execute(GraphCommand::AddAssertion(AddAssertion {
        id: AssertionId::new("assertion-worked-at"),
        subject: EntityId::new("person-a"),
        predicate: PredicateId::new("worked_at"),
        object: GraphValue::Entity(EntityId::new("company-b")),
        valid_time: TimeInterval::new(ValidTime::new(2021), Some(ValidTime::new(2025)))
            .expect("valid interval"),
        confidence: Confidence::new(0.92).expect("confidence"),
        source_ids: vec![SourceId::new("source-employment")],
        context: ContextScope::Global,
    }))
    .expect("assertion added");
    InMemoryStorage::replay(log.events()).expect("storage replay")
}

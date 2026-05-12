use std::collections::BTreeSet;

use rg_core::{AssertionStatus, EntityType};
use rg_worldgen::{
    AgentTaskGenerator, AgentTaskType, CausalChainGenerator, ContradictionGenerator,
    DocumentGenerator, EntityGenerator, EventGenerator, GroundTruthOracle, SourceDocumentKind,
    WorldSchema,
};

#[test]
fn world_schema_generates_reproducible_hidden_truth_and_noisy_observations() {
    let schema = WorldSchema::controlled(42)
        .with_companies(4)
        .with_people(5)
        .with_documents(8)
        .with_agent_tasks(8);

    let first = schema.generate().expect("world generation");
    let second = schema.generate().expect("world generation");
    let different = WorldSchema::controlled(43)
        .generate()
        .expect("different world");

    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_ne!(first.fingerprint(), different.fingerprint());
    assert_eq!(first.companies().len(), 4);
    assert_eq!(first.people().len(), 5);
    assert!(first.hidden_true_state.assertions.len() >= 4);
    assert!(first.noisy_observed_state.assertions.len() > first.hidden_true_state.assertions.len());
    assert!(first
        .noisy_observed_state
        .assertions
        .iter()
        .all(|assertion| !assertion.source_ids.is_empty()));
}

#[test]
fn entity_event_and_document_generators_cover_required_world_artifacts() {
    let schema = WorldSchema::controlled(7)
        .with_companies(3)
        .with_people(4)
        .with_documents(12)
        .with_events(8);
    let entities = EntityGenerator::generate(&schema);
    let events = EventGenerator::generate(&schema, &entities);
    let documents = DocumentGenerator::generate(&schema, &events);

    assert_eq!(
        entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Organization)
            .count(),
        3
    );
    assert_eq!(
        entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Person)
            .count(),
        4
    );
    assert!(events.iter().any(|event| event.kind == "meeting"));
    assert!(events.iter().any(|event| event.kind == "policy_change"));
    assert_document_kind(&documents, SourceDocumentKind::Document);
    assert_document_kind(&documents, SourceDocumentKind::Email);
    assert_document_kind(&documents, SourceDocumentKind::Contract);
    assert_document_kind(&documents, SourceDocumentKind::MeetingNote);
    assert_document_kind(&documents, SourceDocumentKind::News);
    assert_document_kind(&documents, SourceDocumentKind::PolicyChange);
}

#[test]
fn contradiction_generator_adds_conflicting_noisy_claims_without_mutating_truth() {
    let world = WorldSchema::controlled(9)
        .with_contradictions(3)
        .generate()
        .expect("world");
    let noisy_ids = world
        .noisy_observed_state
        .assertions
        .iter()
        .map(|assertion| assertion.id.as_str().to_string())
        .collect::<BTreeSet<_>>();
    let truth_ids = world
        .hidden_true_state
        .assertions
        .iter()
        .map(|assertion| assertion.id.as_str().to_string())
        .collect::<BTreeSet<_>>();

    assert_eq!(world.noisy_observed_state.contradictions.len(), 3);
    assert!(!world.noisy_observed_state.rumors.is_empty());
    assert!(world
        .noisy_observed_state
        .contradictions
        .iter()
        .all(
            |pair| noisy_ids.contains(pair.observed_false_assertion.as_str())
                && truth_ids.contains(pair.hidden_true_assertion.as_str())
        ));
    assert!(world
        .hidden_true_state
        .assertions
        .iter()
        .all(|assertion| assertion.status == AssertionStatus::Active));
}

#[test]
fn causal_chain_generator_creates_ordered_multi_hop_causal_world_events() {
    let schema = WorldSchema::controlled(11)
        .with_causal_chains(2)
        .with_events(8);
    let entities = EntityGenerator::generate(&schema);
    let events = EventGenerator::generate(&schema, &entities);
    let chains = CausalChainGenerator::generate(&schema, &events);

    assert_eq!(chains.len(), 2);
    assert!(chains.iter().all(|chain| chain.links.len() >= 2));
    for chain in &chains {
        for link in &chain.links {
            assert!(link.confidence >= 0.5);
            assert!(link.lag_days >= 1);
            assert!(!link.mechanism.is_empty());
            assert!(!link.counterfactual_note.is_empty());
        }
    }
}

#[test]
fn agent_task_generator_covers_question_planning_memory_simulation_and_verification() {
    let world = WorldSchema::controlled(13)
        .with_agent_tasks(8)
        .generate()
        .expect("world");
    let task_types = world
        .benchmark_tasks
        .iter()
        .map(|task| task.task_type)
        .collect::<BTreeSet<_>>();

    assert_eq!(task_types, AgentTaskType::all().into_iter().collect());
    assert!(world
        .benchmark_tasks
        .iter()
        .all(|task| !task.evidence_assertion_ids.is_empty()
            && !task.hidden_truth_assertion_ids.is_empty()));

    let oracle = GroundTruthOracle::from_world(&world);
    let verify_task = world
        .benchmark_tasks
        .iter()
        .find(|task| task.task_type == AgentTaskType::VerifyClaims)
        .expect("verify task");
    assert_eq!(
        oracle.answer_task(&verify_task.id).expect("oracle answer"),
        verify_task.expected_answer
    );
    assert!(oracle.verify_claim(&verify_task.claim_under_test.clone().unwrap()));
}

#[test]
fn component_generators_can_be_composed_for_custom_benchmark_worlds() {
    let schema = WorldSchema::controlled(21)
        .with_companies(2)
        .with_people(2)
        .with_events(4)
        .with_documents(6)
        .with_contradictions(1)
        .with_causal_chains(1)
        .with_agent_tasks(8);
    let entities = EntityGenerator::generate(&schema);
    let events = EventGenerator::generate(&schema, &entities);
    let documents = DocumentGenerator::generate(&schema, &events);
    let truth = schema
        .hidden_truth_from(&entities, &documents)
        .expect("truth");
    let contradictions = ContradictionGenerator::generate(&schema, &truth, &documents);
    let causal_chains = CausalChainGenerator::generate(&schema, &events);
    let tasks = AgentTaskGenerator::generate(&schema, &truth, &contradictions, &causal_chains);

    assert_eq!(contradictions.len(), 1);
    assert_eq!(causal_chains.len(), 1);
    assert_eq!(tasks.len(), 8);
    assert!(tasks
        .iter()
        .any(|task| matches!(task.task_type, AgentTaskType::RecoverTimelines)));
}

fn assert_document_kind(documents: &[rg_worldgen::SourceDocument], kind: SourceDocumentKind) {
    assert!(
        documents.iter().any(|document| document.kind == kind),
        "missing {kind:?}"
    );
}

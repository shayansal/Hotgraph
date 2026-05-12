use rg_causal::{
    CausalEvent, CausalGraph, CausalLink, CausalPathQuery, CausalRelation, CounterfactualEngine,
    CounterfactualScenario, ImpactedCausalPath, Intervention, Mechanism,
};
use rg_core::{
    Assertion, AssertionId, AssertionStatus, CausalLinkId, Confidence, ContextScope, Entity,
    EntityId, EntityType, EventId, GraphValue, PredicateId, PropertyMap, SourceId, TimeInterval,
    TxTime, ValidTime,
};
use rg_events::GraphState;

#[test]
fn causal_paths_are_distinct_from_relationship_paths_and_propagate_confidence() {
    let graph = supply_chain_causal_graph();

    let paths = graph.downstream_paths(CausalPathQuery {
        start: EventId::new("supplier-disappears"),
        end: Some(EventId::new("contract-breach")),
        max_depth: 3,
        min_confidence: None,
    });

    assert_eq!(paths.len(), 1);
    assert_eq!(
        paths[0].event_ids(),
        vec![
            EventId::new("supplier-disappears"),
            EventId::new("factory-shutdown"),
            EventId::new("contract-breach")
        ]
    );
    assert_eq!(
        paths[0].link_ids(),
        vec![CausalLinkId::new("cause-1"), CausalLinkId::new("cause-2")]
    );
    assert_eq!(paths[0].confidence.as_f32(), 0.63);
    assert!(paths[0].normal_assertion_ids().is_empty());
    assert!(paths[0]
        .explanation
        .contains("causal path, not a normal relationship path"));
}

#[test]
fn upstream_cause_search_returns_explanation_trace() {
    let graph = supply_chain_causal_graph();

    let causes = graph.upstream_causes(EventId::new("contract-breach"), 3);

    assert_eq!(
        causes
            .iter()
            .map(|path| path.start.clone())
            .collect::<Vec<_>>(),
        vec![
            EventId::new("factory-shutdown"),
            EventId::new("supplier-disappears")
        ]
    );
    assert!(causes
        .iter()
        .any(|path| path.explanation.contains("supplier failed to deliver")));
}

#[test]
fn event_intervention_returns_dependency_cone_and_impacted_graph_objects() {
    let graph = supply_chain_causal_graph();
    let state = graph_state();

    let result = CounterfactualEngine::new(&graph, &state).simulate(CounterfactualScenario {
        intervention: Intervention::RemoveEvent(EventId::new("supplier-disappears")),
        valid_at: ValidTime::new(2026),
        max_depth: 3,
        assumptions: vec!["supplier outage did not occur".to_owned()],
    });

    assert_eq!(
        result.dependency_cone.downstream_events,
        vec![
            EventId::new("factory-shutdown"),
            EventId::new("contract-breach")
        ]
    );
    assert_eq!(
        result.affected_entities,
        vec![
            EntityId::new("company-a"),
            EntityId::new("company-b"),
            EntityId::new("contract-1"),
            EntityId::new("factory-1")
        ]
    );
    assert_eq!(
        result.affected_assertions,
        vec![
            AssertionId::new("assertion-contract"),
            AssertionId::new("assertion-supply")
        ]
    );
    assert_eq!(result.impact_paths.len(), 2);
    assert_eq!(result.propagated_confidence.as_f32(), 0.63);
    assert!(result
        .assumptions
        .contains(&"supplier outage did not occur".to_owned()));
    assert!(result.uncertainty.contains("counterfactual"));
    assert!(result.simulation_not_fact);
    assert!(result
        .explanation_trace
        .iter()
        .any(|line| line.contains("does not assert that reality changed")));
}

#[test]
fn relationship_intervention_reports_blast_radius_without_claiming_truth() {
    let graph = supply_chain_causal_graph();
    let state = graph_state();

    let result = CounterfactualEngine::new(&graph, &state).simulate(CounterfactualScenario {
        intervention: Intervention::RemoveAssertion(AssertionId::new("assertion-supply")),
        valid_at: ValidTime::new(2026),
        max_depth: 2,
        assumptions: vec!["Company A no longer supplies Company B".to_owned()],
    });

    assert_eq!(
        result.affected_entities,
        vec![EntityId::new("company-a"), EntityId::new("company-b")]
    );
    assert_eq!(
        result.affected_assertions,
        vec![AssertionId::new("assertion-supply")]
    );
    assert!(result.impact_paths.iter().any(|path| matches!(
        path,
        ImpactedCausalPath::NormalRelationshipBlastRadius { .. }
    )));
    assert!(result.uncertainty.contains("simulation"));
    assert!(result.simulation_not_fact);
}

fn supply_chain_causal_graph() -> CausalGraph {
    let mut graph = CausalGraph::new();
    graph.insert_event(event(
        "supplier-disappears",
        "Supplier stopped shipping",
        vec!["company-a"],
        vec!["assertion-supply"],
    ));
    graph.insert_event(event(
        "factory-shutdown",
        "Factory cannot assemble product",
        vec!["factory-1", "company-b"],
        vec!["assertion-supply"],
    ));
    graph.insert_event(event(
        "contract-breach",
        "Contract delivery obligation is missed",
        vec!["company-b", "contract-1"],
        vec!["assertion-contract"],
    ));
    graph.insert_link(link(
        "cause-1",
        "supplier-disappears",
        "factory-shutdown",
        0.9,
        "supplier failed to deliver critical input",
    ));
    graph.insert_link(link(
        "cause-2",
        "factory-shutdown",
        "contract-breach",
        0.7,
        "factory output loss prevents delivery",
    ));
    graph
}

fn event(id: &str, description: &str, entities: Vec<&str>, assertions: Vec<&str>) -> CausalEvent {
    CausalEvent {
        id: EventId::new(id),
        description: description.to_owned(),
        occurred_at: Some(ValidTime::new(2026)),
        related_entities: entities.into_iter().map(EntityId::new).collect(),
        related_assertions: assertions.into_iter().map(AssertionId::new).collect(),
        source_ids: vec![SourceId::new("source-1")],
        context: ContextScope::Global,
    }
}

fn link(id: &str, cause: &str, effect: &str, confidence: f32, mechanism: &str) -> CausalLink {
    CausalLink {
        id: CausalLinkId::new(id),
        cause_event: EventId::new(cause),
        effect_event: EventId::new(effect),
        relation: CausalRelation::Caused,
        mechanism: Mechanism {
            label: mechanism.to_owned(),
            description: Some(format!("{mechanism} under supply-chain disruption")),
        },
        confidence: Confidence::new(confidence).expect("confidence"),
        source_ids: vec![SourceId::new("source-1")],
        context: ContextScope::Global,
    }
}

fn graph_state() -> GraphState {
    let mut state = GraphState::new();
    for id in ["company-a", "company-b", "factory-1", "contract-1"] {
        state
            .entities
            .insert(EntityId::new(id), entity(id, EntityType::Organization));
    }
    state.assertions.insert(
        AssertionId::new("assertion-supply"),
        assertion(
            "assertion-supply",
            "company-a",
            "SUPPLIES",
            EntityId::new("company-b"),
        ),
    );
    state.assertions.insert(
        AssertionId::new("assertion-contract"),
        assertion(
            "assertion-contract",
            "company-b",
            "HAS_CONTRACT",
            EntityId::new("contract-1"),
        ),
    );
    state
}

fn entity(id: &str, entity_type: EntityType) -> Entity {
    Entity {
        id: EntityId::new(id),
        entity_type,
        canonical_name: Some(id.to_owned()),
        properties: PropertyMap::default(),
        created_tx: TxTime::new(1),
    }
}

fn assertion(id: &str, subject: &str, predicate: &str, object: EntityId) -> Assertion {
    Assertion {
        id: AssertionId::new(id),
        subject: EntityId::new(subject),
        predicate: PredicateId::new(predicate),
        object: GraphValue::Entity(object),
        valid_time: TimeInterval::new(ValidTime::new(2025), None).expect("valid interval"),
        transaction_time: TimeInterval::new(TxTime::new(1), None).expect("tx interval"),
        confidence: Confidence::new(0.9).expect("confidence"),
        source_ids: vec![SourceId::new("source-1")],
        context: ContextScope::Global,
        status: AssertionStatus::Active,
    }
}

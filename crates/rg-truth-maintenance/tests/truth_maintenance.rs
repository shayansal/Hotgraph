use rg_core::{
    Assertion, AssertionId, AssertionStatus, Confidence, ContextScope, EntityId, GraphValue,
    PredicateId, SourceId, TimeInterval, TxTime, ValidTime,
};
use rg_truth_maintenance::{
    AnswerId, AnswerRecord, Assumption, AssumptionId, AssumptionStatus, DependencyGraph,
    DependencyNode, DerivedAssertion, DerivedAssertionId, DerivedAssertionStatus, RetractionReason,
    TruthMaintenanceSystem,
};

#[test]
fn constructors_accessors_and_display_preserve_identity() {
    let assumption_id = AssumptionId::new("assumption-1");
    let derived_id = DerivedAssertionId::new("derived-1");
    let answer_id = AnswerId::new("answer-1");

    assert_eq!(assumption_id.as_str(), "assumption-1");
    assert_eq!(derived_id.as_str(), "derived-1");
    assert_eq!(answer_id.as_str(), "answer-1");
    assert_eq!(assumption_id.to_string(), "assumption-1");
    assert_eq!(derived_id.to_string(), "derived-1");
    assert_eq!(answer_id.to_string(), "answer-1");
}

#[test]
fn dependency_graph_deduplicates_orders_and_explains_dependencies() {
    let mut graph = DependencyGraph::new();
    let source = DependencyNode::Source(SourceId::new("source-1"));
    let assertion = DependencyNode::Assertion(AssertionId::new("assertion-1"));
    let derived = DependencyNode::DerivedAssertion(DerivedAssertionId::new("derived-1"));

    graph.add_dependency(
        source.clone(),
        assertion.clone(),
        "source supports assertion",
    );
    graph.add_dependency(source.clone(), assertion.clone(), "duplicate edge");
    graph.add_dependency(
        assertion.clone(),
        derived.clone(),
        "assertion derives belief",
    );

    assert_eq!(graph.direct_dependents(&source), vec![assertion.clone()]);
    assert_eq!(
        graph.transitive_dependents(&source),
        vec![assertion.clone(), derived.clone()]
    );

    let tree = graph.explanation_dependency_tree(&derived);

    assert_eq!(tree.node, derived);
    assert_eq!(tree.children[0].node, assertion);
    assert_eq!(tree.children[0].children[0].node, source);
    assert!(tree.children[0].children[0]
        .explanation
        .contains("source supports assertion"));
}

#[test]
fn source_invalidation_propagates_to_downstream_beliefs_and_answers() {
    let mut tms = fixture_tms();
    let source = SourceId::new("source-employment");

    let dependents = tms.what_depends_on_source(&source);

    assert!(
        dependents.contains(&DependencyNode::DerivedAssertion(DerivedAssertionId::new(
            "derived-employment"
        )))
    );
    assert!(dependents.contains(&DependencyNode::Answer(AnswerId::new("answer-employment"))));

    let propagation = tms.propagate_retraction(
        DependencyNode::Source(source),
        RetractionReason::SourceInvalidated("source content hash was revoked".to_owned()),
    );

    assert_eq!(
        tms.derived_assertion(&DerivedAssertionId::new("derived-employment"))
            .expect("derived assertion exists")
            .status,
        DerivedAssertionStatus::Invalidated
    );
    assert_eq!(
        tms.answer(&AnswerId::new("answer-employment"))
            .expect("answer exists")
            .invalidated_by,
        Some(DependencyNode::Source(SourceId::new("source-employment")))
    );
    assert!(propagation
        .changed_beliefs
        .contains(&DerivedAssertionId::new("derived-employment")));
    assert!(propagation
        .invalidated_answers
        .contains(&AnswerId::new("answer-employment")));
    assert!(propagation
        .trace
        .explanation
        .contains("source content hash was revoked"));
}

#[test]
fn assertion_false_query_reports_changed_beliefs_without_mutating_state() {
    let tms = fixture_tms();

    let changed = tms.beliefs_changed_if_assertion_false(&AssertionId::new("assertion-worked-at"));

    assert_eq!(changed, vec![DerivedAssertionId::new("derived-employment")]);
    assert_eq!(
        tms.derived_assertion(&DerivedAssertionId::new("derived-employment"))
            .expect("derived assertion exists")
            .status,
        DerivedAssertionStatus::Supported
    );
}

#[test]
fn correction_invalidates_answers_and_dependency_tree_preserves_why_chain() {
    let tms = fixture_tms();

    let invalidated = tms.answers_invalidated_by_correction(&DependencyNode::Assertion(
        AssertionId::new("assertion-worked-at"),
    ));
    let tree =
        tms.explain_dependency_tree(&DependencyNode::Answer(AnswerId::new("answer-employment")));

    assert_eq!(invalidated, vec![AnswerId::new("answer-employment")]);
    assert_eq!(
        tree.node,
        DependencyNode::Answer(AnswerId::new("answer-employment"))
    );
    assert_eq!(
        tree.children[0].node,
        DependencyNode::DerivedAssertion(DerivedAssertionId::new("derived-employment"))
    );
    assert!(tree.children[0]
        .children
        .iter()
        .any(|child| child.node
            == DependencyNode::Assertion(AssertionId::new("assertion-worked-at"))));
    assert!(tree.children[0].children.iter().any(|child| child.node
        == DependencyNode::Assumption(AssumptionId::new("assumption-current-employment"))));
}

fn fixture_tms() -> TruthMaintenanceSystem {
    let mut tms = TruthMaintenanceSystem::new();
    tms.add_assumption(Assumption {
        id: AssumptionId::new("assumption-current-employment"),
        statement: "Employment records are current unless retracted.".to_owned(),
        source_ids: vec![SourceId::new("source-policy")],
        confidence: Confidence::new(0.7).expect("confidence"),
        valid_time: TimeInterval::new(ValidTime::new(2024), None).expect("valid interval"),
        transaction_time: TxTime::new(9),
        status: AssumptionStatus::Active,
    });
    tms.add_derived_assertion(DerivedAssertion {
        id: DerivedAssertionId::new("derived-employment"),
        assertion: assertion("assertion-derived-employment"),
        derived_from: vec![
            DependencyNode::Source(SourceId::new("source-employment")),
            DependencyNode::Assertion(AssertionId::new("assertion-worked-at")),
            DependencyNode::Assumption(AssumptionId::new("assumption-current-employment")),
        ],
        rule: "employment_resolution".to_owned(),
        explanation: "Resolved current employment from source-backed assertion.".to_owned(),
        status: DerivedAssertionStatus::Supported,
    });
    tms.record_answer(AnswerRecord {
        id: AnswerId::new("answer-employment"),
        question: "Where did Person A work?".to_owned(),
        answer_summary: "Person A worked at Company B.".to_owned(),
        depends_on: vec![DependencyNode::DerivedAssertion(DerivedAssertionId::new(
            "derived-employment",
        ))],
        generated_at: TxTime::new(10),
        invalidated_by: None,
    });
    tms
}

fn assertion(id: &str) -> Assertion {
    Assertion {
        id: AssertionId::new(id),
        subject: EntityId::new("person-a"),
        predicate: PredicateId::new("worked_at"),
        object: GraphValue::Entity(EntityId::new("company-b")),
        valid_time: TimeInterval::new(ValidTime::new(2021), Some(ValidTime::new(2025)))
            .expect("valid interval"),
        transaction_time: TimeInterval::new(TxTime::new(8), None).expect("tx interval"),
        confidence: Confidence::new(0.92).expect("confidence"),
        source_ids: vec![SourceId::new("source-employment")],
        context: ContextScope::Global,
        status: AssertionStatus::Active,
    }
}

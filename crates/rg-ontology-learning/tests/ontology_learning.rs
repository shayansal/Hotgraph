use std::collections::BTreeMap;

use rg_core::{
    Assertion, AssertionId, AssertionStatus, Confidence, ContextScope, Entity, EntityId,
    EntityType, GraphOntology, GraphValue, PredicateId, PropertyKey, PropertyMap, TimeInterval,
    TxTime, ValidTime,
};
use rg_ontology_learning::{
    CandidateStatus, ConstraintLearner, EntityTypeClusterer, HumanReviewDecision,
    HumanReviewWorkflow, OntologyChangeKind, OntologyDiscoveryInput, OntologyDriftReport,
    PredicateCandidateMiner, TemporalPatternMiner,
};

#[test]
fn predicate_candidate_miner_discovers_new_source_backed_predicates() {
    let input = fixture_input();

    let candidates = PredicateCandidateMiner::default().mine(&input);

    let shipment = candidates
        .iter()
        .find(|candidate| candidate.predicate == PredicateId::new("SUPPLIES_TO"))
        .unwrap();
    assert_eq!(shipment.support_count, 3);
    assert_eq!(shipment.subject_type.as_deref(), Some("Supplier"));
    assert_eq!(shipment.object_type.as_deref(), Some("Manufacturer"));
    assert!(shipment.temporal);
    assert_eq!(shipment.status, CandidateStatus::PendingReview);
    assert!(shipment
        .evidence_assertion_ids
        .contains(&AssertionId::new("a-supply-1")));

    assert!(!candidates
        .iter()
        .any(|candidate| candidate.predicate == PredicateId::new("WORKED_AT")));
}

#[test]
fn entity_type_clusterer_suggests_emerging_types_and_schema_properties() {
    let input = fixture_input();

    let clusters = EntityTypeClusterer::default().cluster(&input);

    let supplier = clusters
        .iter()
        .find(|cluster| cluster.suggested_type == "Supplier")
        .unwrap();
    assert_eq!(supplier.entity_count, 2);
    assert!(supplier.common_properties.contains_key("name"));
    assert!(supplier.common_properties.contains_key("risk_score"));
    assert_eq!(supplier.status, CandidateStatus::PendingReview);

    let manufacturer = clusters
        .iter()
        .find(|cluster| cluster.suggested_type == "Manufacturer")
        .unwrap();
    assert_eq!(manufacturer.entity_count, 2);
}

#[test]
fn constraint_learner_mines_cardinality_and_contradiction_candidates() {
    let input = fixture_input();

    let constraints = ConstraintLearner::default().learn(&input);

    let active_supplier = constraints
        .iter()
        .find(|constraint| constraint.predicate == PredicateId::new("ACTIVE_SUPPLIER_FOR"))
        .unwrap();
    assert_eq!(
        active_supplier.kind,
        OntologyChangeKind::RelationshipCardinality
    );
    assert_eq!(active_supplier.max_active_objects_per_subject, Some(1));
    assert!(active_supplier.confidence >= 0.8);
    assert_eq!(active_supplier.status, CandidateStatus::PendingReview);

    let status_conflict = constraints
        .iter()
        .find(|constraint| constraint.predicate == PredicateId::new("OPERATIONAL_STATUS"))
        .unwrap();
    assert_eq!(status_conflict.kind, OntologyChangeKind::ContradictionRule);
    assert!(status_conflict
        .description
        .contains("overlapping scalar values"));
}

#[test]
fn temporal_pattern_miner_detects_temporal_predicates_and_pattern_lags() {
    let input = fixture_input();

    let patterns = TemporalPatternMiner::default().mine(&input);

    let supply = patterns
        .iter()
        .find(|pattern| pattern.predicate == PredicateId::new("SUPPLIES_TO"))
        .unwrap();
    assert_eq!(supply.temporal_assertion_count, 3);
    assert_eq!(supply.open_ended_ratio, 0.0);
    assert!(supply.description.contains("bounded valid intervals"));

    let status = patterns
        .iter()
        .find(|pattern| pattern.predicate == PredicateId::new("OPERATIONAL_STATUS"))
        .unwrap();
    assert!(status.open_ended_ratio > 0.0);
}

#[test]
fn drift_report_collects_candidates_without_auto_promoting_changes() {
    let input = fixture_input();

    let report = OntologyDriftReport::generate("supply-chain-pack", &input);

    assert_eq!(report.domain_pack.name, "supply-chain-pack");
    assert!(report.drift_score > 0.0);
    assert!(!report.auto_promoted);
    assert!(report.requires_human_review);
    assert!(report
        .summary
        .contains("ontology changes require human review"));
    assert!(report
        .new_predicates
        .iter()
        .any(|candidate| candidate.predicate == PredicateId::new("SUPPLIES_TO")));
    assert!(report
        .new_entity_types
        .iter()
        .any(|cluster| cluster.suggested_type == "Supplier"));
    assert!(report
        .constraints
        .iter()
        .any(|constraint| constraint.kind == OntologyChangeKind::RelationshipCardinality));
    assert!(report
        .temporal_patterns
        .iter()
        .any(|pattern| pattern.predicate == PredicateId::new("SUPPLIES_TO")));
}

#[test]
fn human_review_workflow_never_auto_promotes_and_tracks_decisions() {
    let input = fixture_input();
    let report = OntologyDriftReport::generate("supply-chain-pack", &input);
    let mut workflow = HumanReviewWorkflow::from_report(report);

    assert!(!workflow.can_auto_promote());
    assert_eq!(workflow.pending_items().len(), workflow.items().len());

    let first_id = workflow.pending_items()[0].id.clone();
    workflow
        .record_decision(
            &first_id,
            HumanReviewDecision::Approve {
                reviewer: "ontology-admin".to_owned(),
                rationale: "matches repeated supply-chain evidence".to_owned(),
            },
        )
        .unwrap();

    let approved = workflow
        .items()
        .iter()
        .find(|item| item.id == first_id)
        .unwrap();
    assert_eq!(approved.status, CandidateStatus::Approved);
    assert!(approved.audit_trail[0].contains("ontology-admin"));
    assert!(workflow
        .approved_changes()
        .iter()
        .any(|item| item.id == first_id));
    assert!(!workflow.can_auto_promote());
}

fn fixture_input() -> OntologyDiscoveryInput {
    let ontology = GraphOntology::from_yaml_str(
        r#"
entity_types:
  Person:
    properties:
      name: string
  Company:
    properties:
      name: string
predicates:
  WORKED_AT:
    subject: Person
    object: Company
    temporal: true
"#,
    )
    .unwrap();

    OntologyDiscoveryInput {
        ontology,
        domain_hint: Some("supply-chain".to_owned()),
        entities: vec![
            entity(
                "supplier-a",
                EntityType::Custom("Supplier".to_owned()),
                &[("name", "Supplier A"), ("risk_score", "0.8")],
            ),
            entity(
                "supplier-b",
                EntityType::Custom("Supplier".to_owned()),
                &[("name", "Supplier B"), ("risk_score", "0.5")],
            ),
            entity(
                "manufacturer-a",
                EntityType::Custom("Manufacturer".to_owned()),
                &[("name", "Manufacturer A")],
            ),
            entity(
                "manufacturer-b",
                EntityType::Custom("Manufacturer".to_owned()),
                &[("name", "Manufacturer B")],
            ),
            entity("person-a", EntityType::Person, &[("name", "A")]),
            entity(
                "company-a",
                EntityType::Organization,
                &[("name", "Company A")],
            ),
        ],
        assertions: vec![
            assertion(
                "a-supply-1",
                "supplier-a",
                "SUPPLIES_TO",
                GraphValue::Entity(EntityId::new("manufacturer-a")),
                10,
                Some(20),
            ),
            assertion(
                "a-supply-2",
                "supplier-b",
                "SUPPLIES_TO",
                GraphValue::Entity(EntityId::new("manufacturer-b")),
                12,
                Some(22),
            ),
            assertion(
                "a-supply-3",
                "supplier-a",
                "SUPPLIES_TO",
                GraphValue::Entity(EntityId::new("manufacturer-b")),
                30,
                Some(40),
            ),
            assertion(
                "a-active-1",
                "manufacturer-a",
                "ACTIVE_SUPPLIER_FOR",
                GraphValue::Entity(EntityId::new("supplier-a")),
                0,
                Some(10),
            ),
            assertion(
                "a-active-2",
                "manufacturer-a",
                "ACTIVE_SUPPLIER_FOR",
                GraphValue::Entity(EntityId::new("supplier-b")),
                10,
                Some(20),
            ),
            assertion(
                "a-status-1",
                "supplier-a",
                "OPERATIONAL_STATUS",
                GraphValue::Text("active".to_owned()),
                0,
                None,
            ),
            assertion(
                "a-status-2",
                "supplier-a",
                "OPERATIONAL_STATUS",
                GraphValue::Text("suspended".to_owned()),
                5,
                None,
            ),
            assertion(
                "a-worked",
                "person-a",
                "WORKED_AT",
                GraphValue::Entity(EntityId::new("company-a")),
                0,
                Some(50),
            ),
        ],
    }
}

fn entity(id: &str, entity_type: EntityType, properties: &[(&str, &str)]) -> Entity {
    Entity {
        id: EntityId::new(id),
        entity_type,
        canonical_name: properties
            .iter()
            .find(|(key, _)| *key == "name")
            .map(|(_, value)| (*value).to_owned()),
        properties: PropertyMap(
            properties
                .iter()
                .map(|(key, value)| {
                    (
                        PropertyKey::new(*key),
                        if *key == "risk_score" {
                            GraphValue::Decimal(value.parse::<f64>().unwrap())
                        } else {
                            GraphValue::Text((*value).to_owned())
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        ),
        created_tx: TxTime::new(1),
    }
}

fn assertion(
    id: &str,
    subject: &str,
    predicate: &str,
    object: GraphValue,
    valid_from: i64,
    valid_to: Option<i64>,
) -> Assertion {
    Assertion {
        id: AssertionId::new(id),
        subject: EntityId::new(subject),
        predicate: PredicateId::new(predicate),
        object,
        valid_time: TimeInterval::new(ValidTime::new(valid_from), valid_to.map(ValidTime::new))
            .unwrap(),
        transaction_time: TimeInterval::new(TxTime::new(1), None).unwrap(),
        confidence: Confidence::new(0.9).unwrap(),
        source_ids: vec![rg_core::SourceId::new(format!("source-{id}"))],
        context: ContextScope::Global,
        status: AssertionStatus::Active,
    }
}

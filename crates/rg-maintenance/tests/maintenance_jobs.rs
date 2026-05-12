use rg_core::{
    Assertion, AssertionId, AssertionStatus, Confidence, ContentHash, ContextScope, Entity,
    EntityId, EntityType, GraphValue, PredicateId, PropertyMap, Source, SourceId, SourceType,
    TimeInterval, TxTime, ValidTime,
};
use rg_events::GraphState;
use rg_maintenance::{
    MaintenanceActionKind, MaintenanceCursor, MaintenanceEngine, MaintenanceJob, MaintenancePolicy,
    MaintenanceTarget, ReviewStatus,
};

#[test]
fn duplicate_entities_create_reviewable_merge_suggestions_without_auto_merging() {
    let state = duplicate_entity_state();
    let mut engine = MaintenanceEngine::new(MaintenancePolicy::review_only(TxTime::new(100)));

    let report =
        engine.detect_duplicate_entities(&state, MaintenanceCursor::from_tx(TxTime::new(0)));

    assert_eq!(report.job, MaintenanceJob::DetectDuplicateEntities);
    assert_eq!(report.review_status, ReviewStatus::Pending);
    assert!(report.incremental);
    assert_eq!(report.graph_health.duplicate_entity_candidates, 1);
    assert_eq!(report.actions.len(), 1);

    let action = &report.actions[0];
    assert_eq!(action.kind, MaintenanceActionKind::SuggestEntityMerge);
    assert_eq!(
        action.target,
        MaintenanceTarget::EntityPair {
            left: EntityId::new("company-oracle-a"),
            right: EntityId::new("company-oracle-b")
        }
    );
    assert!(action.requires_review);
    assert!(action.destructive_if_applied);
    assert!(!action.auto_applied);
    assert!(action
        .explanation
        .contains("No destructive merge was applied"));
    assert!(report
        .audit_log
        .iter()
        .any(|entry| entry.message.contains("operator review")));
    assert_eq!(engine.health_history().len(), 1);
}

#[test]
fn stale_assertion_detection_is_incremental_and_auditable() {
    let state = stale_assertion_state();
    let policy = MaintenancePolicy::review_only(TxTime::new(100)).with_stale_tx_lag(50);
    let mut engine = MaintenanceEngine::new(policy);

    let full_report =
        engine.detect_stale_assertions(&state, MaintenanceCursor::from_tx(TxTime::new(0)));
    assert_eq!(full_report.job, MaintenanceJob::DetectStaleAssertions);
    assert_eq!(full_report.graph_health.stale_assertion_count, 1);
    assert_eq!(full_report.actions.len(), 1);
    assert_eq!(
        full_report.actions[0].target,
        MaintenanceTarget::Assertion(AssertionId::new("assertion-stale"))
    );
    assert_eq!(
        full_report.actions[0].kind,
        MaintenanceActionKind::MarkAssertionStale
    );
    assert!(full_report.actions[0].requires_review);
    assert_eq!(
        full_report.next_cursor,
        MaintenanceCursor::from_tx(TxTime::new(100))
    );

    let incremental_report =
        engine.detect_stale_assertions(&state, MaintenanceCursor::from_tx(TxTime::new(80)));
    assert!(incremental_report.actions.is_empty());
    assert_eq!(incremental_report.graph_health.stale_assertion_count, 0);
    assert!(incremental_report
        .audit_log
        .iter()
        .any(|entry| entry.message.contains("incremental cursor")));
}

#[test]
fn contradiction_job_clusters_conflicts_and_updates_health_history() {
    let state = contradictory_state();
    let mut engine = MaintenanceEngine::new(MaintenancePolicy::review_only(TxTime::new(200)));

    let report = engine.detect_contradictions(&state, MaintenanceCursor::default());

    assert_eq!(report.job, MaintenanceJob::DetectContradictions);
    assert_eq!(report.graph_health.contradiction_count, 1);
    assert_eq!(report.actions.len(), 1);
    assert_eq!(
        report.actions[0].kind,
        MaintenanceActionKind::ClusterContradictions
    );
    assert!(matches!(
        &report.actions[0].target,
        MaintenanceTarget::ContradictionCluster { assertion_ids }
            if assertion_ids == &vec![
                AssertionId::new("assertion-status-a"),
                AssertionId::new("assertion-status-b")
            ]
    ));
    assert!(report.actions[0].requires_review);
    assert!(report.actions[0]
        .evidence
        .iter()
        .any(|item| item.contains("incompatible_scalar_values")));

    let history = engine.health_history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].recorded_at, TxTime::new(200));
    assert_eq!(history[0].contradiction_count, 1);
}

#[test]
fn every_maintenance_job_returns_reviewable_report_and_never_auto_applies_destructive_actions() {
    let state = operational_state();
    let mut engine = MaintenanceEngine::new(MaintenancePolicy::review_only(TxTime::new(300)));

    for job in MaintenanceJob::all() {
        let report = engine.run_job(job, &state, MaintenanceCursor::from_tx(TxTime::new(0)));

        assert_eq!(report.job, job);
        assert_eq!(report.review_status, ReviewStatus::Pending);
        assert!(!report.audit_log.is_empty(), "{job:?} must be auditable");
        assert_eq!(
            report.next_cursor,
            MaintenanceCursor::from_tx(TxTime::new(300))
        );
        assert!(
            report
                .actions
                .iter()
                .all(|action| !(action.auto_applied && action.destructive_if_applied)),
            "{job:?} auto-applied a destructive action"
        );
    }

    assert_eq!(engine.health_history().len(), MaintenanceJob::all().len());
}

fn duplicate_entity_state() -> GraphState {
    let mut state = GraphState::new();
    state.entities.insert(
        EntityId::new("company-oracle-a"),
        entity("company-oracle-a", EntityType::Organization, "Oracle", 10),
    );
    state.entities.insert(
        EntityId::new("company-oracle-b"),
        entity("company-oracle-b", EntityType::Organization, " oracle ", 12),
    );
    state.entities.insert(
        EntityId::new("person-alice"),
        entity("person-alice", EntityType::Person, "Alice", 14),
    );
    state
}

fn stale_assertion_state() -> GraphState {
    let mut state = duplicate_entity_state();
    state.assertions.insert(
        AssertionId::new("assertion-stale"),
        assertion(
            "assertion-stale",
            "company-oracle-a",
            "HAS_STATUS",
            GraphValue::Text("active".to_owned()),
            1,
            20,
        ),
    );
    state.assertions.insert(
        AssertionId::new("assertion-fresh"),
        assertion(
            "assertion-fresh",
            "company-oracle-b",
            "HAS_STATUS",
            GraphValue::Text("active".to_owned()),
            90,
            95,
        ),
    );
    state
}

fn contradictory_state() -> GraphState {
    let mut state = duplicate_entity_state();
    state.assertions.insert(
        AssertionId::new("assertion-status-a"),
        assertion(
            "assertion-status-a",
            "company-oracle-a",
            "HAS_STATUS",
            GraphValue::Text("independent".to_owned()),
            10,
            30,
        ),
    );
    state.assertions.insert(
        AssertionId::new("assertion-status-b"),
        assertion(
            "assertion-status-b",
            "company-oracle-a",
            "HAS_STATUS",
            GraphValue::Text("acquired".to_owned()),
            12,
            31,
        ),
    );
    state
}

fn operational_state() -> GraphState {
    let mut state = contradictory_state();
    state.sources.insert(
        SourceId::new("source-low-trust"),
        Source {
            id: SourceId::new("source-low-trust"),
            source_type: SourceType::HumanReport,
            uri: Some("memo://low-trust".to_owned()),
            content_hash: ContentHash::new("hash-low-trust"),
            observed_at: TxTime::new(4),
            trust_score: Some(0.2),
        },
    );
    state.assertions.insert(
        AssertionId::new("assertion-broken-edge"),
        assertion(
            "assertion-broken-edge",
            "company-oracle-a",
            "SUPPLIES",
            GraphValue::Entity(EntityId::new("missing-company")),
            100,
            100,
        ),
    );
    state
}

fn entity(id: &str, entity_type: EntityType, canonical_name: &str, created_tx: i64) -> Entity {
    Entity {
        id: EntityId::new(id),
        entity_type,
        canonical_name: Some(canonical_name.to_owned()),
        properties: PropertyMap::default(),
        created_tx: TxTime::new(created_tx),
    }
}

fn assertion(
    id: &str,
    subject: &str,
    predicate: &str,
    object: GraphValue,
    tx_start: i64,
    valid_start: i64,
) -> Assertion {
    Assertion {
        id: AssertionId::new(id),
        subject: EntityId::new(subject),
        predicate: PredicateId::new(predicate),
        object,
        valid_time: TimeInterval::new(ValidTime::new(valid_start), None).expect("valid interval"),
        transaction_time: TimeInterval::new(TxTime::new(tx_start), None).expect("tx interval"),
        confidence: Confidence::new(0.82).expect("confidence"),
        source_ids: vec![SourceId::new("source-low-trust")],
        context: ContextScope::Global,
        status: AssertionStatus::Active,
    }
}

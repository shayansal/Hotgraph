use proptest::prelude::*;
use proptest::test_runner::Config as ProptestConfig;
use rg_ai::{EvidencePackGenerator, EvidencePackRequest};
use rg_core::{
    AssertionId, Confidence, ContextScope, EntityId, EntityType, GraphValue, PredicateId,
    PropertyMap, SourceId, TimeInterval, TxTime, ValidTime,
};
use rg_events::{
    AddAssertion, AddSource, ContentHash, CreateEntity, EventLog, GraphCommand, RetractAssertion,
    SourceType,
};
use rg_query::{GraphQuery, PathQuery};
use rg_storage::InMemoryStorage;
use rg_temporal_reasoning::{
    active_during, after, before, changed_between, contains, during, equals, finishes, known_at,
    meets, overlaps, starts, superseded_after, valid_at, AllenRelation, TemporalEvidenceExplainer,
    TemporalPathReasoner, TemporalReasoner,
};

#[test]
fn allen_interval_algebra_classifies_named_relations() {
    let a = interval(10, 20);
    let b = interval(30, 40);
    let meeting = interval(20, 30);
    let overlap = interval(15, 30);
    let container = interval(0, 50);
    let same_start = interval(10, 30);
    let same_end = interval(0, 20);

    assert!(before(&a, &b));
    assert!(after(&b, &a));
    assert!(meets(&a, &meeting));
    assert!(overlaps(&a, &overlap));
    assert!(during(&a, &container));
    assert!(contains(&container, &a));
    assert!(starts(&a, &same_start));
    assert!(finishes(&a, &same_end));
    assert!(equals(&a, &interval(10, 20)));
    assert_eq!(AllenRelation::classify(&a, &b), Some(AllenRelation::Before));
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    #[test]
    fn before_and_after_are_inverses(a_start in 0i64..500, a_len in 1i64..50, gap in 1i64..50, b_len in 1i64..50) {
        let a = interval(a_start, a_start + a_len);
        let b = interval(a_start + a_len + gap, a_start + a_len + gap + b_len);

        prop_assert!(before(&a, &b));
        prop_assert!(after(&b, &a));
        prop_assert!(!before(&b, &a));
    }

    #[test]
    fn contains_and_during_are_inverses(start in 0i64..500, prefix in 1i64..20, inner_len in 1i64..50, suffix in 1i64..20) {
        let outer = interval(start, start + prefix + inner_len + suffix);
        let inner = interval(start + prefix, start + prefix + inner_len);

        prop_assert!(contains(&outer, &inner));
        prop_assert!(during(&inner, &outer));
    }

    #[test]
    fn equals_is_symmetric(start in 0i64..500, len in 1i64..50) {
        let left = interval(start, start + len);
        let right = interval(start, start + len);

        prop_assert!(equals(&left, &right));
        prop_assert!(equals(&right, &left));
    }
}

#[test]
fn bitemporal_queries_distinguish_world_truth_from_system_knowledge() {
    let storage = temporal_fixture();
    let reasoner = TemporalReasoner::new(&storage);

    let ceo_in_2024 = reasoner.valid_at(ValidTime::new(2024));
    assert!(ceo_in_2024
        .iter()
        .any(|assertion| assertion.id == AssertionId::new("ceo-before-acquisition")));

    let known_before_warning = reasoner.known_at(TxTime::new(8));
    assert!(known_before_warning
        .iter()
        .any(|assertion| assertion.id == AssertionId::new("ceo-before-acquisition")));
    assert!(!known_before_warning
        .iter()
        .any(|assertion| assertion.id == AssertionId::new("lawsuit-started")));

    assert!(valid_at(&storage, ValidTime::new(2026))
        .iter()
        .any(|assertion| assertion.id == AssertionId::new("contract-active")));
    assert!(known_at(&storage, TxTime::new(10))
        .iter()
        .any(|assertion| assertion.id == AssertionId::new("contract-active")));
}

#[test]
fn temporal_query_operators_find_changes_active_windows_and_supersession() {
    let storage = temporal_fixture();

    let changed = changed_between(&storage, TxTime::new(9), TxTime::new(11));
    assert_eq!(
        changed
            .iter()
            .map(|assertion| assertion.id.clone())
            .collect::<Vec<_>>(),
        vec![
            AssertionId::new("lawsuit-started"),
            AssertionId::new("contract-active"),
            AssertionId::new("warning-signal")
        ]
    );

    let active = active_during(&storage, &interval(2025, 2027));
    assert!(active
        .iter()
        .any(|assertion| assertion.id == AssertionId::new("contract-active")));

    let superseded = superseded_after(&storage, TxTime::new(7));
    assert_eq!(superseded.len(), 1);
    assert_eq!(superseded[0].id, AssertionId::new("warning-signal"));
}

#[test]
fn temporal_reasoning_filters_path_queries() {
    let storage = temporal_fixture();
    let path_reasoner = TemporalPathReasoner::new(&storage);

    let paths = path_reasoner.paths_active_during(
        PathQuery {
            start: EntityId::new("person-a"),
            end: Some(EntityId::new("lawsuit-1")),
            predicates: vec![PredicateId::new("CEO_OF"), PredicateId::new("HAS_LAWSUIT")],
            valid_at: None,
            max_depth: 2,
            min_confidence: None,
        },
        &interval(2024, 2025),
    );
    assert!(paths.is_empty());

    let paths = path_reasoner.paths_active_during(
        PathQuery {
            start: EntityId::new("person-a"),
            end: Some(EntityId::new("lawsuit-1")),
            predicates: vec![PredicateId::new("CEO_OF"), PredicateId::new("HAS_LAWSUIT")],
            valid_at: None,
            max_depth: 2,
            min_confidence: None,
        },
        &interval(2025, 2026),
    );
    assert_eq!(paths.len(), 1);
    assert!(paths[0]
        .temporal_explanation
        .contains("active during 2025..2026"));
}

#[test]
fn evidence_packs_include_temporal_explanations() {
    let storage = temporal_fixture();
    let generator = EvidencePackGenerator::new(&storage);
    let pack = generator.generate(EvidencePackRequest {
        query: "Which contracts were active when the lawsuit started?".to_owned(),
        graph_query: GraphQuery {
            subject: Some(rg_query::EntityPattern::Id(EntityId::new("company-x"))),
            predicate: None,
            object: None,
            valid_at: Some(2025),
            known_at: Some(10),
            context: None,
            min_confidence: None,
            limit: None,
        },
        path_query: None,
        generated_at: TxTime::new(10),
    });

    let explanations =
        TemporalEvidenceExplainer::new(ValidTime::new(2025), TxTime::new(10)).explain_pack(&pack);

    assert!(explanations.iter().any(|explanation| {
        explanation.assertion_id == AssertionId::new("contract-active")
            && explanation.explanation.contains("valid at 2025")
            && explanation.explanation.contains("known at tx 10")
    }));
}

fn temporal_fixture() -> InMemoryStorage {
    let mut log = EventLog::new(TxTime::new(0));
    add_source(&mut log);
    for (id, entity_type) in [
        ("person-a", EntityType::Person),
        ("company-x", EntityType::Organization),
        ("company-y", EntityType::Organization),
        ("lawsuit-1", EntityType::Event),
        ("contract-1", EntityType::Document),
        ("warning-1", EntityType::Event),
    ] {
        log.execute(GraphCommand::CreateEntity(CreateEntity {
            id: EntityId::new(id),
            entity_type,
            canonical_name: Some(id.to_owned()),
            properties: PropertyMap::default(),
        }))
        .expect("entity created");
    }

    add_assertion(
        &mut log,
        "ceo-before-acquisition",
        "person-a",
        "CEO_OF",
        GraphValue::Entity(EntityId::new("company-x")),
        2020,
        Some(2026),
    );
    add_assertion(
        &mut log,
        "lawsuit-started",
        "company-x",
        "HAS_LAWSUIT",
        GraphValue::Entity(EntityId::new("lawsuit-1")),
        2025,
        None,
    );
    add_assertion(
        &mut log,
        "contract-active",
        "company-x",
        "HAS_CONTRACT",
        GraphValue::Entity(EntityId::new("contract-1")),
        2024,
        Some(2027),
    );
    add_assertion(
        &mut log,
        "warning-signal",
        "warning-1",
        "WARNED_ABOUT",
        GraphValue::Entity(EntityId::new("company-x")),
        2024,
        Some(2026),
    );
    log.execute(GraphCommand::RetractAssertion(RetractAssertion {
        id: AssertionId::new("warning-signal"),
    }))
    .expect("warning retracted");

    InMemoryStorage::replay(log.events()).expect("storage replay")
}

fn add_source(log: &mut EventLog) {
    log.execute(GraphCommand::AddSource(AddSource {
        id: SourceId::new("source-1"),
        source_type: SourceType::Document,
        uri: Some("file://source.md".to_owned()),
        content_hash: ContentHash::new("sha256:temporal"),
        trust_score: Some(0.9),
    }))
    .expect("source added");
}

fn add_assertion(
    log: &mut EventLog,
    id: &str,
    subject: &str,
    predicate: &str,
    object: GraphValue,
    valid_from: i64,
    valid_to: Option<i64>,
) {
    log.execute(GraphCommand::AddAssertion(AddAssertion {
        id: AssertionId::new(id),
        subject: EntityId::new(subject),
        predicate: PredicateId::new(predicate),
        object,
        valid_time: TimeInterval::new(ValidTime::new(valid_from), valid_to.map(ValidTime::new))
            .expect("valid interval"),
        confidence: Confidence::new(0.9).expect("confidence"),
        source_ids: vec![SourceId::new("source-1")],
        context: ContextScope::Global,
    }))
    .expect("assertion added");
}

fn interval(start: i64, end: i64) -> TimeInterval<ValidTime> {
    TimeInterval::new(ValidTime::new(start), Some(ValidTime::new(end))).expect("valid interval")
}

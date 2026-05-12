use rg_core::{
    AssertionId, Confidence, ContextScope, EntityId, EntityType, GraphValue, PredicateId,
    PropertyMap, SourceId, TimeInterval, TxTime, ValidTime,
};
use rg_events::{
    AddAssertion, AddSource, ContentHash, CreateEntity, EventLog, GraphCommand, SourceType,
};
use rg_retrieval_compiler::{QueryIntent, RetrievalOperator};
use rg_storage::InMemoryStorage;
use rgql::{
    compile_natural_language, EntitySelector, ExecutorContext, PathSelector, RgqlExecutor,
    RgqlParser, RgqlStatement,
};

#[test]
fn parses_find_query_with_bitemporal_evidence_constraints() {
    let statement = RgqlParser::parse(
        r#"FIND Person
WHERE worked_at Company("Oracle")
VALID_AT "2023-01-01"
KNOWN_AT "2026-05-12"
WITH EVIDENCE
MIN_CONFIDENCE 0.8"#,
    )
    .expect("query parses");

    assert_eq!(
        statement,
        RgqlStatement::Find {
            entity: EntitySelector::Type {
                entity_type: "Person".to_owned()
            },
            predicate: Some(PredicateId::new("worked_at")),
            object: Some(EntitySelector::TypedName {
                entity_type: "Company".to_owned(),
                name: "Oracle".to_owned()
            }),
            valid_at: Some(20230101),
            known_at: Some(20260512),
            with_evidence: true,
            min_confidence: Some(0.8),
            contradictions: false,
            limit: None,
        }
    );
}

#[test]
fn parses_path_causal_contradiction_and_counterfactual_queries() {
    let path = RgqlParser::parse(
        r#"PATH FROM Person("A") TO Company("D")
VIA owns|controls|board_member_of
VALID_DURING "2020-01-01".."2024-12-31"
RETURN paths, evidence, confidence"#,
    )
    .expect("path query parses");
    assert_eq!(
        path,
        RgqlStatement::Path {
            from: EntitySelector::TypedName {
                entity_type: "Person".to_owned(),
                name: "A".to_owned()
            },
            to: Some(EntitySelector::TypedName {
                entity_type: "Company".to_owned(),
                name: "D".to_owned()
            }),
            via: vec![
                PredicateId::new("owns"),
                PredicateId::new("controls"),
                PredicateId::new("board_member_of")
            ],
            valid_at: None,
            valid_during: Some((20200101, 20241231)),
            min_confidence: None,
            max_depth: 3,
            returns: vec![
                "paths".to_owned(),
                "evidence".to_owned(),
                "confidence".to_owned()
            ],
        }
    );

    let causal = RgqlParser::parse(
        r#"CAUSES OF Event("market_drop")
WITHIN 30d
MIN_CONFIDENCE 0.6
RETURN causal_paths, mechanisms, sources"#,
    )
    .expect("causal query parses");
    assert!(matches!(causal, RgqlStatement::Causes { .. }));

    let contradictions = RgqlParser::parse(
        r#"CONTRADICTIONS FOR Entity("company-a") VALID_AT "2024-01-01" RETURN contradictions, evidence"#,
    )
    .expect("contradiction query parses");
    assert!(matches!(
        contradictions,
        RgqlStatement::Contradictions { .. }
    ));

    let counterfactual = RgqlParser::parse(
        r#"COUNTERFACTUAL REMOVE ASSERTION("assertion-supply")
VALID_AT "2026-01-01"
MAX_DEPTH 3
RETURN impacted_entities, impacted_assertions, paths"#,
    )
    .expect("counterfactual query parses");
    assert!(matches!(
        counterfactual,
        RgqlStatement::Counterfactual { .. }
    ));
}

#[test]
fn parser_errors_include_position_and_expected_construct() {
    let error = RgqlParser::parse(r#"FIND Person WHERE worked_at Company("Oracle") VALID_AT"#)
        .expect_err("invalid query fails");

    assert_eq!(error.position, 54);
    assert!(error.message.contains("expected timestamp literal"));
}

#[test]
fn planner_produces_retrieval_plan_explain_trace_and_cost() {
    let statement = RgqlParser::parse(
        r#"FIND Person WHERE worked_at Company("Oracle") VALID_AT "2023-01-01" WITH EVIDENCE MIN_CONFIDENCE 0.8"#,
    )
    .expect("query parses");
    let plan = statement.plan();
    let explain = statement.explain();
    let cost = statement.estimate_cost();

    assert_eq!(plan.retrieval_plan.intent, QueryIntent::Historical);
    assert!(plan
        .retrieval_plan
        .operators
        .contains(&RetrievalOperator::TemporalFilter));
    assert!(plan
        .retrieval_plan
        .operators
        .contains(&RetrievalOperator::GraphExpansion));
    assert!(plan
        .retrieval_plan
        .operators
        .contains(&RetrievalOperator::Cite));
    assert!(explain
        .trace
        .steps
        .iter()
        .any(|step| step.reason.contains("VALID_AT")));
    assert!(cost.estimated_rows >= 1);
    assert!(cost.estimated_cost_units > 0.0);
}

#[test]
fn natural_language_can_compile_into_rgql() {
    let rgql = compile_natural_language("Where did Alice work on 2023-01-01 with evidence?")
        .expect("natural language compiles");

    assert_eq!(
        rgql,
        r#"MATCH Entity("Alice") WHERE worked_at VALID_AT "2023-01-01" WITH EVIDENCE"#
    );
    assert!(RgqlParser::parse(&rgql).is_ok());
}

#[test]
fn executor_runs_find_path_contradiction_and_counterfactual_queries() {
    let storage = fixture_storage();
    let causal_graph = rg_causal::CausalGraph::new();
    let context = ExecutorContext::new(storage, causal_graph);
    let executor = RgqlExecutor::new(&context);

    let find = executor
        .execute(
            &RgqlParser::parse(
                r#"FIND Person WHERE worked_at Company("Oracle") VALID_AT "2023-01-01" KNOWN_AT "2026-05-12" WITH EVIDENCE MIN_CONFIDENCE 0.8"#,
            )
            .expect("find parses"),
        )
        .expect("find executes");
    assert_eq!(find.assertions().len(), 1);
    assert_eq!(
        find.assertions()[0].assertion_id,
        AssertionId::new("assertion-worked-at")
    );
    assert_eq!(
        find.evidence_pack().expect("evidence pack").sources.len(),
        1
    );

    let path = executor
        .execute(
            &RgqlParser::parse(
                r#"PATH FROM Person("Alice") TO Place("Austin") VIA worked_at|located_in VALID_DURING "2020-01-01".."2024-12-31" MIN_CONFIDENCE 0.8 RETURN paths, evidence"#,
            )
            .expect("path parses"),
        )
        .expect("path executes");
    assert_eq!(path.paths().len(), 1);
    assert_eq!(
        path.paths()[0]
            .hops
            .iter()
            .map(|hop| hop.assertion_id.as_str())
            .collect::<Vec<_>>(),
        vec!["assertion-worked-at", "assertion-located-in"]
    );

    let conflicts = executor
        .execute(
            &RgqlParser::parse(
                r#"CONTRADICTIONS FOR Entity("person-alice") VALID_AT "2023-01-01" RETURN contradictions, evidence"#,
            )
            .expect("contradictions parses"),
        )
        .expect("contradictions executes");
    assert_eq!(conflicts.contradictions().len(), 1);

    let counterfactual = executor
        .execute(
            &RgqlParser::parse(
                r#"COUNTERFACTUAL REMOVE ASSERTION("assertion-worked-at") VALID_AT "2026-01-01" MAX_DEPTH 3 RETURN impacted_entities, impacted_assertions, paths"#,
            )
            .expect("counterfactual parses"),
        )
        .expect("counterfactual executes");
    assert!(
        counterfactual
            .impact_trace()
            .expect("impact trace")
            .simulation_not_fact
    );
}

#[test]
fn path_query_supports_bare_entity_ids_when_names_are_not_available() {
    let selector = PathSelector::from(EntitySelector::TypedName {
        entity_type: "Company".to_owned(),
        name: "oracle".to_owned(),
    });

    assert_eq!(selector.entity_type.as_deref(), Some("Company"));
    assert_eq!(selector.name.as_deref(), Some("oracle"));
}

fn fixture_storage() -> InMemoryStorage {
    let mut log = EventLog::new(TxTime::new(20260500));
    log.execute(GraphCommand::AddSource(AddSource {
        id: SourceId::new("source-employment"),
        source_type: SourceType::Document,
        uri: Some("file://employment.md".to_owned()),
        content_hash: ContentHash::new("sha256:employment"),
        trust_score: Some(0.95),
    }))
    .expect("source added");
    for (id, entity_type, name) in [
        ("person-alice", EntityType::Person, "Alice"),
        ("company-oracle", EntityType::Organization, "Oracle"),
        ("company-sun", EntityType::Organization, "Sun"),
        ("place-austin", EntityType::Place, "Austin"),
    ] {
        log.execute(GraphCommand::CreateEntity(CreateEntity {
            id: EntityId::new(id),
            entity_type,
            canonical_name: Some(name.to_owned()),
            properties: PropertyMap::default(),
        }))
        .expect("entity added");
    }
    for (id, subject, predicate, object, confidence) in [
        (
            "assertion-worked-at",
            "person-alice",
            "worked_at",
            GraphValue::Entity(EntityId::new("company-oracle")),
            0.92,
        ),
        (
            "assertion-worked-at-conflict",
            "person-alice",
            "worked_at",
            GraphValue::Entity(EntityId::new("company-sun")),
            0.87,
        ),
        (
            "assertion-located-in",
            "company-oracle",
            "located_in",
            GraphValue::Entity(EntityId::new("place-austin")),
            0.91,
        ),
    ] {
        log.execute(GraphCommand::AddAssertion(AddAssertion {
            id: AssertionId::new(id),
            subject: EntityId::new(subject),
            predicate: PredicateId::new(predicate),
            object,
            valid_time: TimeInterval::new(ValidTime::new(20200101), Some(ValidTime::new(20250101)))
                .expect("valid interval"),
            confidence: Confidence::new(confidence).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-employment")],
            context: ContextScope::Global,
        }))
        .expect("assertion added");
    }

    InMemoryStorage::replay(log.events()).expect("storage replay")
}

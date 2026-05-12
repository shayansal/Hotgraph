use rg_core::{
    Assertion, AssertionId, Confidence, ContentHash, ContextScope, EntityId, EntityType,
    GraphValue, PredicateId, PropertyMap, SourceId, TimeInterval, TxTime, ValidTime,
};
use rg_events::{AddAssertion, AddSource, CreateEntity, EventLog, GraphCommand, SourceType};
use rg_graphrag::{
    CommunityId, CommunitySnapshot, CommunitySummary, SummaryInvalidationReason, SummarySourceSet,
    SummaryTxTime, SummaryValidTime, TemporalCommunitySummarizer,
};
use rg_storage::InMemoryStorage;

#[test]
fn temporal_community_summaries_are_source_backed_and_queryable_as_of_time() {
    let storage = fixture_storage();
    let summarizer = TemporalCommunitySummarizer::from_storage(&storage);
    let summaries = summarizer.summaries_at(ValidTime::new(20240101), TxTime::new(99));

    assert_eq!(summaries.len(), 2);
    let employment = summaries
        .iter()
        .find(|summary| summary.text.contains("WORKED_AT"))
        .expect("employment community");

    assert!(matches!(employment.community_id, CommunityId { .. }));
    assert!(employment
        .source_set
        .contains(&SourceId::new("source-employment")));
    assert!(employment
        .source_set
        .contains(&SourceId::new("source-employment-extra")));
    assert!(employment.text.contains("Person A"));
    assert!(employment.text.contains("Company B"));
    assert!(employment.valid_time.contains(ValidTime::new(20240101)));
    assert!(employment.transaction_time.contains(TxTime::new(99)));

    let queried = summarizer
        .summary_at(
            &employment.community_id,
            ValidTime::new(20240101),
            TxTime::new(99),
        )
        .expect("point-in-time summary");
    assert_eq!(queried.source_set, employment.source_set);
}

#[test]
fn summary_known_at_filters_out_evidence_not_yet_recorded() {
    let storage = fixture_storage();
    let summarizer = TemporalCommunitySummarizer::from_storage(&storage);

    let known_late = summarizer.summaries_at(ValidTime::new(20240101), TxTime::new(99));
    let known_early = summarizer.summaries_at(ValidTime::new(20240101), TxTime::new(5));

    assert!(known_late
        .iter()
        .any(|summary| summary.text.contains("WORKED_AT")));
    assert!(!known_early
        .iter()
        .any(|summary| summary.text.contains("WORKED_AT")));
}

#[test]
fn incremental_update_invalidates_only_affected_communities() {
    let storage = fixture_storage();
    let mut summarizer = TemporalCommunitySummarizer::from_storage(&storage);
    let before = summarizer.summaries_at(ValidTime::new(20240101), TxTime::new(99));
    assert_eq!(before.len(), 2);

    let updated_assertion = test_assertion(
        "assertion-person-a-located-in",
        "person-a",
        "LOCATED_IN",
        "city-z",
        "source-employment-extra",
        20240101,
        None,
        0.81,
        TxTime::new(100),
    );
    let affected = summarizer.invalidate_for_assertion(
        &updated_assertion,
        SummaryInvalidationReason::AssertionAdded(updated_assertion.id.clone()),
    );

    assert_eq!(affected.len(), 1);
    let stale = summarizer.stale_summaries();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].community_id, affected[0]);
    assert_eq!(
        stale[0].invalidation_reason,
        Some(SummaryInvalidationReason::AssertionAdded(AssertionId::new(
            "assertion-person-a-located-in"
        )))
    );

    let fresh_unaffected = summarizer
        .summaries()
        .iter()
        .filter(|summary| !summary.stale)
        .count();
    assert_eq!(fresh_unaffected, 1);

    let mut updated_storage = storage.clone();
    updated_storage
        .append_event(rg_events::GraphEvent::AssertionAdded(
            rg_events::AssertionAdded {
                event_id: rg_core::EventId::new("manual-assertion-added"),
                transaction_time: TxTime::new(100),
                assertion: updated_assertion,
            },
        ))
        .expect("append updated assertion");
    let recomputed = summarizer.recompute_affected(&updated_storage, &affected, TxTime::new(101));

    assert_eq!(recomputed.len(), 1);
    assert!(summarizer.stale_summaries().is_empty());
    assert!(summarizer
        .summary_at(&affected[0], ValidTime::new(20240101), TxTime::new(101))
        .expect("recomputed summary")
        .text
        .contains("LOCATED_IN"));
}

#[test]
fn public_summary_types_preserve_temporal_and_source_metadata() {
    let source_set = SummarySourceSet::from_sources(vec![SourceId::new("source-a")]);
    let valid_time = SummaryValidTime::new(ValidTime::new(20240101), None).expect("valid time");
    let tx_time = SummaryTxTime::new(TxTime::new(10), Some(TxTime::new(20))).expect("tx time");
    let snapshot = CommunitySnapshot {
        community_id: CommunityId::new("community-a"),
        entity_ids: vec![EntityId::new("entity-a")],
        assertion_ids: vec![AssertionId::new("assertion-a")],
        valid_time: valid_time.clone(),
        transaction_time: tx_time.clone(),
        source_set: source_set.clone(),
    };
    let summary = CommunitySummary {
        community_id: snapshot.community_id.clone(),
        snapshot,
        text: "Community summary".to_owned(),
        valid_time,
        transaction_time: tx_time,
        source_set,
        stale: false,
        invalidation_reason: None,
    };

    assert!(summary.source_set.contains(&SourceId::new("source-a")));
    assert!(summary.valid_time.contains(ValidTime::new(20240101)));
    assert!(!summary.transaction_time.contains(TxTime::new(20)));
}

fn fixture_storage() -> InMemoryStorage {
    let mut log = EventLog::new(TxTime::new(0));
    for (id, hash) in [
        ("source-employment", "employment"),
        ("source-employment-extra", "employment-extra"),
        ("source-ownership", "ownership"),
    ] {
        log.execute(GraphCommand::AddSource(AddSource {
            id: SourceId::new(id),
            source_type: SourceType::Document,
            uri: Some(format!("file://{hash}.md")),
            content_hash: ContentHash::new(format!("sha256:{hash}")),
            trust_score: Some(0.9),
        }))
        .expect("source added");
    }
    for (id, entity_type, name) in [
        ("person-a", EntityType::Person, "Person A"),
        ("company-b", EntityType::Organization, "Company B"),
        ("company-c", EntityType::Organization, "Company C"),
        ("company-d", EntityType::Organization, "Company D"),
        ("city-z", EntityType::Place, "City Z"),
    ] {
        log.execute(GraphCommand::CreateEntity(CreateEntity {
            id: EntityId::new(id),
            entity_type,
            canonical_name: Some(name.to_owned()),
            properties: PropertyMap::default(),
        }))
        .expect("entity created");
    }
    for assertion in [
        test_assertion(
            "assertion-worked-at",
            "person-a",
            "WORKED_AT",
            "company-b",
            "source-employment",
            20210101,
            Some(20250101),
            0.92,
            TxTime::new(10),
        ),
        test_assertion(
            "assertion-advised",
            "person-a",
            "ADVISED",
            "company-b",
            "source-employment-extra",
            20200101,
            None,
            0.86,
            TxTime::new(11),
        ),
        test_assertion(
            "assertion-owns",
            "company-c",
            "OWNS",
            "company-d",
            "source-ownership",
            20200101,
            None,
            0.88,
            TxTime::new(12),
        ),
    ] {
        log.execute(GraphCommand::AddAssertion(AddAssertion {
            id: assertion.id,
            subject: assertion.subject,
            predicate: assertion.predicate,
            object: assertion.object,
            valid_time: assertion.valid_time,
            confidence: assertion.confidence,
            source_ids: assertion.source_ids,
            context: assertion.context,
        }))
        .expect("assertion added");
    }

    InMemoryStorage::replay(log.events()).expect("fixture storage")
}

#[allow(clippy::too_many_arguments)]
fn test_assertion(
    id: &str,
    subject: &str,
    predicate: &str,
    object: &str,
    source: &str,
    valid_from: i64,
    valid_to: Option<i64>,
    confidence: f32,
    tx_from: TxTime,
) -> Assertion {
    Assertion {
        id: AssertionId::new(id),
        subject: EntityId::new(subject),
        predicate: PredicateId::new(predicate),
        object: GraphValue::Entity(EntityId::new(object)),
        valid_time: TimeInterval::new(ValidTime::new(valid_from), valid_to.map(ValidTime::new))
            .expect("valid interval"),
        transaction_time: TimeInterval::new(tx_from, None).expect("tx interval"),
        confidence: Confidence::new(confidence).expect("confidence"),
        source_ids: vec![SourceId::new(source)],
        context: ContextScope::Global,
        status: rg_core::AssertionStatus::Active,
    }
}

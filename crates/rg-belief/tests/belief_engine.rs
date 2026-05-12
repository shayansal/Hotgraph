use rg_belief::{
    BeliefEngine, BeliefQuery, Claim, ClaimId, ConflictType, ResolutionPolicy, ResolutionStatus,
    SourceTrustModel,
};
use rg_core::{
    Confidence, EntityId, GraphValue, PredicateId, SourceId, TimeInterval, TxTime, ValidTime,
};

#[test]
fn conflict_sets_return_both_sides_and_preferred_claim_with_reason() {
    let mut engine = engine_with_trust([("source-a", 0.62), ("source-b", 0.91)]);

    engine.ingest_claim(acquired_on_claim(
        "claim-announced-march",
        "source-a",
        0.86,
        20260301,
        10,
    ));
    engine.ingest_claim(acquired_on_claim(
        "claim-closed-june",
        "source-b",
        0.84,
        20260630,
        20,
    ));

    let conflicts = engine.conflict_sets();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].conflict_type, ConflictType::DateMismatch);
    assert_eq!(
        conflicts[0].claim_ids,
        vec![
            ClaimId::new("claim-announced-march"),
            ClaimId::new("claim-closed-june")
        ]
    );
    assert_eq!(
        conflicts[0].preferred_claim_id,
        Some(ClaimId::new("claim-closed-june"))
    );
    assert_eq!(conflicts[0].resolution_status, ResolutionStatus::Preferred);
    assert!(conflicts[0].explanation.contains("source trust"));

    let state = engine.belief_state(BeliefQuery {
        subject: company_x(),
        predicate: PredicateId::new("ACQUIRED_ON"),
        valid_at: ValidTime::new(20260501),
        known_at: TxTime::new(25),
    });

    assert_eq!(
        state.preferred_claim.expect("preferred claim").id,
        ClaimId::new("claim-closed-june")
    );
    assert_eq!(state.competing_claims.len(), 2);
    assert!(state.explanation.contains("claim-closed-june"));
}

#[test]
fn belief_state_distinguishes_what_we_believed_then_from_now() {
    let mut engine = engine_with_trust([("source-a", 0.7), ("source-c", 0.97)]);

    engine.ingest_claim(status_claim("claim-closed", "source-a", "closed", 0.83, 10));

    let then = engine.belief_state(BeliefQuery {
        subject: company_x(),
        predicate: PredicateId::new("DEAL_STATUS"),
        valid_at: ValidTime::new(20260701),
        known_at: TxTime::new(15),
    });
    assert_eq!(
        then.preferred_claim.expect("then preferred").id,
        ClaimId::new("claim-closed")
    );

    engine.ingest_claim(status_claim(
        "claim-regulator-blocked",
        "source-c",
        "blocked",
        0.9,
        30,
    ));

    let now = engine.belief_state(BeliefQuery {
        subject: company_x(),
        predicate: PredicateId::new("DEAL_STATUS"),
        valid_at: ValidTime::new(20260701),
        known_at: TxTime::new(35),
    });

    assert_eq!(
        now.preferred_claim.expect("now preferred").id,
        ClaimId::new("claim-regulator-blocked")
    );
    assert_eq!(
        now.conflict_sets[0].conflict_type,
        ConflictType::MutuallyExclusiveStatus
    );
    assert_eq!(now.competing_claims.len(), 2);
}

#[test]
fn belief_revisions_explain_changes_over_transaction_time() {
    let mut engine =
        engine_with_trust([("source-a", 0.62), ("source-b", 0.91), ("source-c", 0.97)]);

    engine.ingest_claim(acquired_on_claim(
        "claim-announced-march",
        "source-a",
        0.86,
        20260301,
        10,
    ));
    engine.ingest_claim(acquired_on_claim(
        "claim-closed-june",
        "source-b",
        0.84,
        20260630,
        20,
    ));
    engine.ingest_claim(status_claim(
        "claim-regulator-blocked",
        "source-c",
        "blocked",
        0.9,
        30,
    ));

    let revisions = engine.belief_revisions();
    assert!(revisions.iter().any(|revision| {
        revision.previous_belief == Some(ClaimId::new("claim-announced-march"))
            && revision.new_belief == Some(ClaimId::new("claim-closed-june"))
            && revision.reason.contains("source-b")
            && revision.transaction_time == TxTime::new(20)
    }));

    let explanation = engine.explain_belief_changes(BeliefQuery {
        subject: company_x(),
        predicate: PredicateId::new("ACQUIRED_ON"),
        valid_at: ValidTime::new(20260501),
        known_at: TxTime::new(40),
    });
    assert!(explanation.contains("claim-announced-march"));
    assert!(explanation.contains("claim-closed-june"));
    assert!(explanation.contains("what we believed changed"));
}

#[test]
fn all_conflict_types_are_classified_deterministically() {
    let mut engine = engine_with_trust([
        ("trusted", 0.95),
        ("low", 0.2),
        ("medium", 0.55),
        ("other", 0.6),
    ]);

    engine.ingest_claim(status_claim("status-open", "medium", "open", 0.7, 1));
    engine.ingest_claim(status_claim("status-closed", "trusted", "closed", 0.8, 2));
    engine.ingest_claim(scalar_claim(
        "price-a",
        "trusted",
        "PRICE",
        GraphValue::Integer(10),
        3,
    ));
    engine.ingest_claim(scalar_claim(
        "price-b",
        "other",
        "PRICE",
        GraphValue::Integer(12),
        4,
    ));
    engine.ingest_claim(scalar_claim(
        "same-as-a",
        "trusted",
        "SAME_AS",
        GraphValue::Entity(EntityId::new("company-y-legal")),
        5,
    ));
    engine.ingest_claim(scalar_claim(
        "same-as-b",
        "other",
        "SAME_AS",
        GraphValue::Entity(EntityId::new("company-y-rumor")),
        6,
    ));
    engine.ingest_claim(scalar_claim(
        "cause-a",
        "trusted",
        "CAUSED_BY",
        GraphValue::Text("regulatory block".to_owned()),
        7,
    ));
    engine.ingest_claim(scalar_claim(
        "cause-b",
        "other",
        "CAUSED_BY",
        GraphValue::Text("financing collapse".to_owned()),
        8,
    ));
    engine.ingest_claim(scalar_claim(
        "same-source-content-high",
        "trusted",
        "HEADQUARTERS",
        GraphValue::Text("Berlin".to_owned()),
        9,
    ));
    engine.ingest_claim(scalar_claim(
        "same-source-content-low",
        "low",
        "HEADQUARTERS",
        GraphValue::Text("Berlin".to_owned()),
        10,
    ));
    engine.ingest_claim(temporally_overlapping_claim(
        "ceo-a",
        "trusted",
        "CEO_OF",
        GraphValue::Entity(EntityId::new("company-a")),
        20240101,
        20250101,
        11,
    ));
    engine.ingest_claim(temporally_overlapping_claim(
        "ceo-b",
        "other",
        "CEO_OF",
        GraphValue::Entity(EntityId::new("company-b")),
        20240601,
        20260101,
        12,
    ));

    let conflict_types = engine
        .conflict_sets()
        .into_iter()
        .map(|set| set.conflict_type)
        .collect::<std::collections::BTreeSet<_>>();

    assert!(conflict_types.contains(&ConflictType::MutuallyExclusiveStatus));
    assert!(conflict_types.contains(&ConflictType::NumericMismatch));
    assert!(conflict_types.contains(&ConflictType::EntityIdentityMismatch));
    assert!(conflict_types.contains(&ConflictType::CausalDisagreement));
    assert!(conflict_types.contains(&ConflictType::SourceTrustDisagreement));
    assert!(conflict_types.contains(&ConflictType::ValidTimeOverlapConflict));
}

fn engine_with_trust<const N: usize>(sources: [(&str, f32); N]) -> BeliefEngine {
    let mut trust = SourceTrustModel::new(0.5);
    for (source_id, score) in sources {
        trust.set_trust(SourceId::new(source_id), score);
    }
    BeliefEngine::new(ResolutionPolicy::trust_weighted(), trust)
}

fn acquired_on_claim(id: &str, source: &str, confidence: f32, date: i64, tx: i64) -> Claim {
    scalar_claim(
        id,
        source,
        "ACQUIRED_ON",
        GraphValue::Time(ValidTime::new(date)),
        tx,
    )
    .with_confidence(Confidence::new(confidence).expect("confidence"))
}

fn status_claim(id: &str, source: &str, status: &str, confidence: f32, tx: i64) -> Claim {
    scalar_claim(
        id,
        source,
        "DEAL_STATUS",
        GraphValue::Text(status.to_owned()),
        tx,
    )
    .with_confidence(Confidence::new(confidence).expect("confidence"))
}

fn scalar_claim(id: &str, source: &str, predicate: &str, object: GraphValue, tx: i64) -> Claim {
    temporally_overlapping_claim(id, source, predicate, object, 20240101, 20270101, tx)
}

fn temporally_overlapping_claim(
    id: &str,
    source: &str,
    predicate: &str,
    object: GraphValue,
    valid_from: i64,
    valid_to: i64,
    tx: i64,
) -> Claim {
    Claim {
        id: ClaimId::new(id),
        subject: company_x(),
        predicate: PredicateId::new(predicate),
        object,
        valid_time: TimeInterval::new(ValidTime::new(valid_from), Some(ValidTime::new(valid_to)))
            .expect("valid interval"),
        transaction_time: TxTime::new(tx),
        confidence: Confidence::new(0.8).expect("confidence"),
        source_ids: vec![SourceId::new(source)],
        evidence: format!("evidence from {source} for {id}"),
    }
}

fn company_x() -> EntityId {
    EntityId::new("company-x")
}

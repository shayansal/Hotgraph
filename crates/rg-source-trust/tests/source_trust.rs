use rg_core::{Confidence, SourceId, SourceType, TxTime};
use rg_source_trust::{
    BeliefConfidenceInput, CorroborationGraph, IndependenceScore, SourceAuthority, SourceIdentity,
    SourceReputation, TrustPolicy, TrustUpdateEvent, TrustUpdateKind,
};

#[test]
fn source_identity_scores_type_signature_and_issuer() {
    let signed_regulator = SourceIdentity::new(
        SourceId::new("source-regulator"),
        SourceType::Document,
        "energy-regulator",
    )
    .with_domain("energy")
    .with_signature("sig-key-1", true)
    .with_issuer_verified(true);

    let unsigned_blog = SourceIdentity::new(
        SourceId::new("source-blog"),
        SourceType::WebPage,
        "anonymous-blog",
    )
    .with_domain("energy");

    assert!(signed_regulator.identity_score() > unsigned_blog.identity_score());
    assert!(signed_regulator.identity_score() > 0.8);
    assert!(unsigned_blog.identity_score() < 0.55);
}

#[test]
fn source_authority_combines_domain_authority_and_human_rating() {
    let authority = SourceAuthority::new("energy")
        .with_domain_authority(0.9)
        .with_human_rating(0.8)
        .with_source_type_weight(SourceType::Document, 0.85)
        .with_issuer_authority("energy-regulator", 0.95);

    assert!(authority.authority_score("energy-regulator", &SourceType::Document) > 0.85);
    assert!(authority.authority_score("unknown", &SourceType::WebPage) < 0.75);
}

#[test]
fn reputation_updates_track_accuracy_conflicts_tamper_and_recency() {
    let mut reputation = SourceReputation::new(SourceId::new("source-a"), TxTime::new(100));
    reputation.apply(TrustUpdateEvent::new(
        SourceId::new("source-a"),
        TxTime::new(110),
        TrustUpdateKind::AccurateClaim,
    ));
    reputation.apply(TrustUpdateEvent::new(
        SourceId::new("source-a"),
        TxTime::new(120),
        TrustUpdateKind::ConflictObserved,
    ));
    reputation.apply(TrustUpdateEvent::new(
        SourceId::new("source-a"),
        TxTime::new(130),
        TrustUpdateKind::TamperEvidence,
    ));

    assert_eq!(reputation.historical_accuracy(), 0.5);
    assert_eq!(reputation.conflict_rate(), 0.5);
    assert!(reputation.tamper_penalty() > 0.0);
    assert!(
        reputation.recency_score(TxTime::new(140)) > reputation.recency_score(TxTime::new(10_000))
    );
}

#[test]
fn corroboration_graph_rewards_independent_support_and_penalizes_shared_issuer() {
    let mut graph = CorroborationGraph::new();
    graph.add_support(SourceId::new("source-a"), SourceId::new("claim-1"));
    graph.add_support(SourceId::new("source-b"), SourceId::new("claim-1"));
    graph.add_support(SourceId::new("source-c"), SourceId::new("claim-1"));
    graph.add_shared_issuer(SourceId::new("source-a"), SourceId::new("source-b"));

    let independence = IndependenceScore::from_corroboration(
        &graph,
        &[
            SourceId::new("source-a"),
            SourceId::new("source-b"),
            SourceId::new("source-c"),
        ],
    );

    assert_eq!(
        graph.corroborating_sources(&SourceId::new("claim-1")).len(),
        3
    );
    assert!(independence.score > 0.6);
    assert!(independence.score < 1.0);
    assert!(independence.explanation.contains("shared issuer"));
}

#[test]
fn trust_policy_scores_sources_from_epistemic_signals() {
    let policy = TrustPolicy::default();
    let identity = SourceIdentity::new(
        SourceId::new("source-regulator"),
        SourceType::Document,
        "energy-regulator",
    )
    .with_domain("energy")
    .with_signature("sig-key-1", true)
    .with_issuer_verified(true);
    let authority = SourceAuthority::new("energy")
        .with_domain_authority(0.9)
        .with_human_rating(0.85)
        .with_source_type_weight(SourceType::Document, 0.85)
        .with_issuer_authority("energy-regulator", 0.95);
    let reputation = SourceReputation::new(SourceId::new("source-regulator"), TxTime::new(1))
        .with_observations(18, 2, 1);
    let independence = IndependenceScore {
        score: 0.8,
        explanation: "independent corroboration".to_owned(),
    };

    let score = policy.score_source(
        &identity,
        &authority,
        &reputation,
        independence,
        TxTime::new(5),
    );

    assert!(score.score > 0.75);
    assert!(score
        .factors
        .iter()
        .any(|factor| factor.name == "cryptographic_signature"));
    assert!(score
        .factors
        .iter()
        .any(|factor| factor.name == "historical_accuracy"));
}

#[test]
fn belief_confidence_uses_source_extraction_corroboration_contradiction_and_freshness() {
    let policy = TrustPolicy::default();

    let strong = policy.belief_confidence(BeliefConfidenceInput {
        source_confidence: 0.9,
        extraction_confidence: Confidence::new(0.85).expect("confidence"),
        corroboration: 0.8,
        contradiction: 0.05,
        temporal_freshness: 0.9,
    });
    let weak = policy.belief_confidence(BeliefConfidenceInput {
        source_confidence: 0.45,
        extraction_confidence: Confidence::new(0.6).expect("confidence"),
        corroboration: 0.1,
        contradiction: 0.8,
        temporal_freshness: 0.25,
    });

    assert!(strong.as_f32() > weak.as_f32());
    assert!(strong.as_f32() > 0.75);
    assert!(weak.as_f32() < 0.45);
}

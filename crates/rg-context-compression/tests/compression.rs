use std::collections::BTreeSet;

use rg_ai::{EvidencePack, SourceExcerpt};
use rg_context_compression::{
    CompressionLevel, CompressionPlan, CompressionSignal, ContextBudget, ContextCompressor,
    EvidencePreservationPolicy, GoldAnswerSet, QualityEvaluator,
};
use rg_core::{
    Assertion, AssertionId, AssertionStatus, Confidence, ContentHash, ContextScope, EntityId,
    GraphValue, PredicateId, SourceId, SourceType, TimeInterval, TxTime, ValidTime,
};
use rg_index::{Contradiction, ContradictionType, Severity};

#[test]
fn context_packs_fit_target_token_budget() {
    let pack = fixture_pack();
    let plan = CompressionPlan::new(
        ContextBudget::new(86, 12),
        EvidencePreservationPolicy::strict(),
    )
    .with_max_level(CompressionLevel::TaskSpecificDistilledContext)
    .rank_by(vec![
        CompressionSignal::TaskRelevance,
        CompressionSignal::ContradictionImportance,
        CompressionSignal::Confidence,
        CompressionSignal::SourceTrust,
        CompressionSignal::Recency,
    ]);

    let compressed =
        ContextCompressor::new(plan).compress(&pack, "2024 Oracle employment conflict");

    assert!(compressed.estimated_tokens <= 86);
    assert!(!compressed.units.is_empty());
    assert!(compressed.text.contains("why_this_matters="));
}

#[test]
fn citations_survive_compression() {
    let pack = fixture_pack();
    let compressed = ContextCompressor::new(CompressionPlan::default_for_tokens(120))
        .compress(&pack, "Oracle employment evidence");

    let source_ids = compressed.citation_map.source_ids();
    assert!(source_ids.contains(&SourceId::new("source-employment")));
    assert!(source_ids.contains(&SourceId::new("source-conflict")));
    assert!(compressed.text.contains("source-employment"));
    assert!(compressed
        .citation_map
        .assertion_ids()
        .contains(&AssertionId::new("assertion-worked-at")));
}

#[test]
fn contradictions_are_not_silently_removed() {
    let pack = fixture_pack();
    let compressed = ContextCompressor::new(CompressionPlan::default_for_tokens(74))
        .compress(&pack, "show both sides of the employment contradiction");

    assert!(compressed
        .citation_map
        .contradiction_ids()
        .contains("contradiction-employment"));
    assert!(compressed.text.contains("contradiction"));
    assert!(compressed
        .warnings
        .iter()
        .all(|warning| !warning.contains("contradiction removed")));
}

#[test]
fn compression_supports_all_ranking_signals() {
    let pack = fixture_pack();
    let compressed = ContextCompressor::new(
        CompressionPlan::default_for_tokens(120).rank_by(CompressionSignal::all()),
    )
    .compress(&pack, "recent trusted central employment contradiction");

    let scores = compressed
        .units
        .iter()
        .map(|unit| (unit.id.as_str(), unit.score))
        .collect::<Vec<_>>();
    assert!(scores.windows(2).all(|pair| pair[0].1 >= pair[1].1));
    assert!(compressed.units.iter().any(|unit| {
        unit.applied_signals
            .contains(&CompressionSignal::GraphCentrality)
    }));
    assert!(compressed.units.iter().any(|unit| {
        unit.applied_signals
            .contains(&CompressionSignal::SourceTrust)
    }));
}

#[test]
fn compression_quality_is_evaluated_against_answer_accuracy() {
    let pack = fixture_pack();
    let compressed = ContextCompressor::new(CompressionPlan::default_for_tokens(120))
        .compress(&pack, "Where did Alice work in 2024?");
    let gold = GoldAnswerSet {
        required_source_ids: BTreeSet::from([
            SourceId::new("source-employment"),
            SourceId::new("source-conflict"),
        ]),
        required_assertion_ids: BTreeSet::from([
            AssertionId::new("assertion-worked-at"),
            AssertionId::new("assertion-worked-at-conflict"),
        ]),
        required_contradiction_ids: BTreeSet::from(["contradiction-employment".to_owned()]),
        baseline_answer_accuracy: 0.92,
    };

    let report = QualityEvaluator::evaluate(&compressed, &gold);

    assert_eq!(report.citation_recall, 1.0);
    assert_eq!(report.contradiction_recall, 1.0);
    assert!(report.estimated_answer_accuracy >= 0.82);
    assert!(report.passes_accuracy_floor(0.8));
}

fn fixture_pack() -> EvidencePack {
    let worked_at = assertion(AssertionFixture {
        id: "assertion-worked-at",
        subject: "person-alice",
        predicate: "WORKED_AT",
        object: GraphValue::Entity(EntityId::new("company-oracle")),
        valid_from: 20210101,
        valid_to: Some(20250101),
        tx_from: 20260501,
        confidence: 0.94,
        source: "source-employment",
    });
    let conflict = assertion(AssertionFixture {
        id: "assertion-worked-at-conflict",
        subject: "person-alice",
        predicate: "WORKED_AT",
        object: GraphValue::Entity(EntityId::new("company-sun")),
        valid_from: 20230101,
        valid_to: Some(20250101),
        tx_from: 20260510,
        confidence: 0.82,
        source: "source-conflict",
    });
    let source_employment = SourceExcerpt {
        source_id: SourceId::new("source-employment"),
        source_type: SourceType::Document,
        uri: Some("file://employment.md".to_owned()),
        content_hash: ContentHash::new("sha256:employment"),
        snippet: "Oracle HR filing says Alice worked at Oracle from 2021 through 2024 with payroll evidence and manager attestation.".to_owned(),
        trust_score: Some(0.96),
    };
    let source_conflict = SourceExcerpt {
        source_id: SourceId::new("source-conflict"),
        source_type: SourceType::HumanReport,
        uri: Some("file://conflict.md".to_owned()),
        content_hash: ContentHash::new("sha256:conflict"),
        snippet: "A later interview claims Alice worked at Sun during 2023 and 2024, conflicting with Oracle payroll records.".to_owned(),
        trust_score: Some(0.62),
    };

    EvidencePack {
        query: "Where did Alice work in 2024?".to_owned(),
        entities: Vec::new(),
        assertions: vec![worked_at, conflict],
        sources: vec![source_employment, source_conflict],
        paths: Vec::new(),
        contradictions: vec![Contradiction {
            id: rg_core::ContradictionId::new("contradiction-employment"),
            assertion_a: AssertionId::new("assertion-worked-at"),
            assertion_b: AssertionId::new("assertion-worked-at-conflict"),
            contradiction_type: ContradictionType::ExactPredicateConflict,
            severity: Severity::High,
            explanation:
                "Overlapping WORKED_AT assertions disagree about Alice's employer in 2023-2024."
                    .to_owned(),
        }],
        generated_at: TxTime::new(20260512),
    }
}

struct AssertionFixture<'a> {
    id: &'a str,
    subject: &'a str,
    predicate: &'a str,
    object: GraphValue,
    valid_from: i64,
    valid_to: Option<i64>,
    tx_from: i64,
    confidence: f32,
    source: &'a str,
}

fn assertion(fixture: AssertionFixture<'_>) -> Assertion {
    Assertion {
        id: AssertionId::new(fixture.id),
        subject: EntityId::new(fixture.subject),
        predicate: PredicateId::new(fixture.predicate),
        object: fixture.object,
        valid_time: TimeInterval::new(
            ValidTime::new(fixture.valid_from),
            fixture.valid_to.map(ValidTime::new),
        )
        .expect("valid interval"),
        transaction_time: TimeInterval::new(TxTime::new(fixture.tx_from), None).expect("valid tx"),
        confidence: Confidence::new(fixture.confidence).expect("valid confidence"),
        source_ids: vec![SourceId::new(fixture.source)],
        context: ContextScope::Global,
        status: AssertionStatus::Active,
    }
}

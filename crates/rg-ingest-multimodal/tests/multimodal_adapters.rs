use rg_core::{Confidence, EntityType, SourceId, TxTime, ValidTime};
use rg_ingest_multimodal::{
    all_default_adapters, content_hash_for_bytes, AdapterRegistry, CandidateStatus,
    EvidenceLocator, ReviewTaskStatus, ReviewTaskTarget, SourceContent, SourceInput,
    SourceModality,
};

#[test]
fn every_default_adapter_creates_source_episode_evidence_embedding_and_review_task() {
    let adapters = all_default_adapters();
    assert_eq!(adapters.len(), 9);

    for adapter in adapters {
        let modality = adapter.modality();
        let input = SourceInput {
            id: SourceId::new(format!("source-{modality:?}")),
            modality,
            uri: Some(format!("fixture://{modality:?}")),
            observed_at: TxTime::new(42),
            trust_score: Some(0.8),
            content: SourceContent::Text(format!("fixture content for {modality:?}")),
        };

        let batch = adapter.ingest(input.clone()).expect("adapter is testable");

        assert_eq!(batch.source.source.id, input.id);
        assert_eq!(batch.source.modality, modality);
        assert_eq!(
            batch.source.source.content_hash,
            content_hash_for_bytes(input.content.as_bytes())
        );
        assert_eq!(batch.episode.source_id, batch.source.source.id);
        assert_eq!(batch.evidence_snippets.len(), 1);
        assert_eq!(
            batch.evidence_snippets[0].content_hash,
            batch.source.source.content_hash
        );
        assert!(matches!(
            batch.evidence_snippets[0].locator,
            EvidenceLocator::ByteRange { .. }
        ));
        assert!(!batch.embeddings.is_empty());
        assert!(batch.review_tasks.iter().any(|task| {
            task.status == ReviewTaskStatus::Pending
                && matches!(task.target, ReviewTaskTarget::Source(_))
        }));
    }
}

#[test]
fn text_adapter_extracts_uncertain_candidates_with_exact_evidence_links() {
    let registry = AdapterRegistry::with_default_adapters();
    let content = concat!(
        "entity:Oracle|type=Organization|confidence=0.88|evidence=Oracle\n",
        "assertion:Alice|WORKED_AT|Oracle|valid=2021..2024|confidence=0.91|evidence=Alice worked at Oracle\n",
        "event:Supplier outage|time=2026|confidence=0.72|evidence=Supplier outage\n",
        "causal:Supplier outage|Factory shutdown|mechanism=supply interruption|confidence=0.66|evidence=supply interruption\n"
    );

    let batch = registry
        .ingest(SourceInput {
            id: SourceId::new("source-text-1"),
            modality: SourceModality::Text,
            uri: Some("fixture://text".to_owned()),
            observed_at: TxTime::new(100),
            trust_score: None,
            content: SourceContent::Text(content.to_owned()),
        })
        .expect("text ingest succeeds");

    assert_eq!(batch.candidate_entities.len(), 1);
    assert_eq!(batch.candidate_entities[0].name, "Oracle");
    assert_eq!(
        batch.candidate_entities[0].entity_type,
        Some(EntityType::Organization)
    );
    assert_eq!(
        batch.candidate_entities[0].status,
        CandidateStatus::PendingReview
    );

    let assertion = &batch.candidate_assertions[0];
    assert_eq!(assertion.subject_text, "Alice");
    assert_eq!(assertion.predicate_text, "WORKED_AT");
    assert_eq!(assertion.object_text, "Oracle");
    assert_eq!(
        assertion.valid_time.as_ref().map(|interval| interval.start),
        Some(ValidTime::new(2021))
    );
    assert_eq!(
        assertion.confidence,
        Confidence::new(0.91).expect("confidence")
    );
    assert_eq!(assertion.status, CandidateStatus::PendingReview);

    let evidence = batch
        .evidence_snippets
        .iter()
        .find(|snippet| snippet.id == assertion.evidence_id)
        .expect("assertion links to exact evidence");
    assert_eq!(evidence.text, "Alice worked at Oracle");
    assert_eq!(
        &content.as_bytes()[evidence.byte_start..evidence.byte_end],
        b"Alice worked at Oracle"
    );
    assert_eq!(assertion.source_id, evidence.source_id);

    assert_eq!(batch.candidate_events[0].event_text, "Supplier outage");
    assert_eq!(
        batch.candidate_events[0].valid_time,
        Some(ValidTime::new(2026))
    );
    assert_eq!(
        batch.candidate_causal_links[0].mechanism.as_deref(),
        Some("supply interruption")
    );
    assert!(batch.review_tasks.iter().any(|task| {
        task.status == ReviewTaskStatus::Pending
            && matches!(task.target, ReviewTaskTarget::CandidateAssertion(_))
    }));
}

#[test]
fn structured_adapters_are_deterministic_and_keep_candidates_review_gated() {
    let registry = AdapterRegistry::with_default_adapters();
    let csv_like =
        "subject,predicate,object,evidence\nAlice,WORKED_AT,Oracle,Alice worked at Oracle\n";
    let json_like = r#"{"entity":"Oracle","type":"Organization","evidence":"Oracle"}"#;

    let first = registry
        .ingest(SourceInput {
            id: SourceId::new("source-csv"),
            modality: SourceModality::Csv,
            uri: Some("fixture://sheet.csv".to_owned()),
            observed_at: TxTime::new(11),
            trust_score: Some(0.7),
            content: SourceContent::Text(csv_like.to_owned()),
        })
        .expect("csv ingest succeeds");
    let second = registry
        .ingest(SourceInput {
            id: SourceId::new("source-csv"),
            modality: SourceModality::Csv,
            uri: Some("fixture://sheet.csv".to_owned()),
            observed_at: TxTime::new(11),
            trust_score: Some(0.7),
            content: SourceContent::Text(csv_like.to_owned()),
        })
        .expect("csv ingest is deterministic");
    assert_eq!(first, second);
    assert_eq!(first.candidate_assertions.len(), 1);
    assert!(first
        .candidate_assertions
        .iter()
        .all(|candidate| candidate.status == CandidateStatus::PendingReview));
    assert!(first
        .review_tasks
        .iter()
        .any(|task| matches!(task.target, ReviewTaskTarget::CandidateAssertion(_))));

    let json_batch = registry
        .ingest(SourceInput {
            id: SourceId::new("source-json"),
            modality: SourceModality::Json,
            uri: Some("fixture://entity.json".to_owned()),
            observed_at: TxTime::new(12),
            trust_score: Some(0.6),
            content: SourceContent::Text(json_like.to_owned()),
        })
        .expect("json ingest succeeds");
    assert_eq!(json_batch.candidate_entities[0].name, "Oracle");
    assert_eq!(
        json_batch.candidate_entities[0].entity_type,
        Some(EntityType::Organization)
    );
}

#[test]
fn adapter_registry_rejects_missing_or_mismatched_adapters() {
    let registry = AdapterRegistry::new(vec![Box::new(rg_ingest_multimodal::TextSourceAdapter)]);

    let error = registry
        .ingest(SourceInput {
            id: SourceId::new("source-pdf"),
            modality: SourceModality::Pdf,
            uri: None,
            observed_at: TxTime::new(1),
            trust_score: None,
            content: SourceContent::Bytes(b"%PDF fake".to_vec()),
        })
        .expect_err("pdf adapter is not registered");

    assert!(error.to_string().contains("no adapter registered"));
}

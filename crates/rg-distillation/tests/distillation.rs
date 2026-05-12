use rg_core::{AssertionId, SourceId};
use rg_distillation::{
    DistillationBenchmark, DistillationCorpus, DistillationTask, GraphTruthLabel, OnnxExporter,
    SmallBaselineTrainer, TrainingDataGenerator,
};

#[test]
fn generator_creates_training_data_for_all_distillation_tasks() {
    let dataset = TrainingDataGenerator::default().generate(&fixture_corpus());

    assert_eq!(dataset.tasks(), DistillationTask::all());
    for task in DistillationTask::all() {
        let examples = dataset.examples_for(task);
        assert!(!examples.is_empty(), "{task:?} examples should exist");
        assert!(examples.iter().all(|example| example.task == task));
        assert!(examples
            .iter()
            .all(|example| !example.graph_truth.evidence_assertion_ids.is_empty()));
        assert!(examples
            .iter()
            .all(|example| !example.graph_truth.source_ids.is_empty()));
    }

    let router = dataset.examples_for(DistillationTask::RetrievalRouter);
    assert!(router
        .iter()
        .any(|example| example.label.as_str() == "temporal_graph"));
    assert!(router
        .iter()
        .any(|example| example.label.as_str() == "vector"));
}

#[test]
fn small_baseline_models_train_and_predict_with_graph_truth_labels() {
    let dataset = TrainingDataGenerator::default().generate(&fixture_corpus());
    let trainer = SmallBaselineTrainer::default();

    let router = trainer
        .train(&dataset, DistillationTask::RetrievalRouter)
        .unwrap();
    let temporal = trainer
        .train(&dataset, DistillationTask::TemporalClassifier)
        .unwrap();
    let contradiction = trainer
        .train(&dataset, DistillationTask::ContradictionClassifier)
        .unwrap();

    assert_eq!(router.task, DistillationTask::RetrievalRouter);
    assert!(router.validation_accuracy >= 0.8);
    assert!(router.graph_truth_required);
    assert_eq!(
        router.predict(
            dataset.examples_for(DistillationTask::RetrievalRouter)[0]
                .features
                .clone()
        ),
        dataset.examples_for(DistillationTask::RetrievalRouter)[0].label
    );
    assert_eq!(
        temporal.predict(
            dataset.examples_for(DistillationTask::TemporalClassifier)[0]
                .features
                .clone()
        ),
        dataset.examples_for(DistillationTask::TemporalClassifier)[0].label
    );
    assert_eq!(
        contradiction.predict(
            dataset.examples_for(DistillationTask::ContradictionClassifier)[0]
                .features
                .clone()
        ),
        dataset.examples_for(DistillationTask::ContradictionClassifier)[0].label
    );
}

#[test]
fn onnx_export_is_available_for_portable_baseline_models() {
    let dataset = TrainingDataGenerator::default().generate(&fixture_corpus());
    let model = SmallBaselineTrainer::default()
        .train(&dataset, DistillationTask::SourceTrustEstimator)
        .unwrap();

    let artifact = OnnxExporter::default().export(&model).unwrap();

    assert_eq!(artifact.file_name, "source_trust_estimator.onnx");
    assert!(artifact.portable);
    assert_eq!(artifact.task, DistillationTask::SourceTrustEstimator);
    assert!(artifact.bytes.starts_with(b"ONNX"));
    assert!(artifact
        .metadata
        .iter()
        .any(|entry| entry.contains("graph_truth_required=true")));
    assert!(artifact
        .metadata
        .iter()
        .any(|entry| entry.contains("opset=")));
}

#[test]
fn benchmark_compares_small_models_against_llm_only_decisions() {
    let dataset = TrainingDataGenerator::default().generate(&fixture_corpus());
    let trainer = SmallBaselineTrainer::default();
    let models = DistillationTask::all()
        .into_iter()
        .map(|task| trainer.train(&dataset, task).unwrap())
        .collect::<Vec<_>>();

    let report = DistillationBenchmark::default().compare(&dataset, &models);

    assert!(report.graph_truth_enforced);
    assert!(report.small_model_accuracy >= report.llm_only_accuracy);
    assert!(report.small_model_p95_latency_micros < report.llm_only_p95_latency_micros);
    assert!(report.cost_reduction_ratio > 0.8);
    assert_eq!(report.per_task.len(), DistillationTask::all().len());
    assert!(report
        .summary
        .contains("Use LLMs where needed; use small models where possible"));
}

#[test]
fn path_ranker_and_memory_promotion_are_first_class_tasks() {
    let dataset = TrainingDataGenerator::default().generate(&fixture_corpus());

    let path_examples = dataset.examples_for(DistillationTask::PathRanker);
    let memory_examples = dataset.examples_for(DistillationTask::MemoryPromotionClassifier);

    assert!(path_examples
        .iter()
        .any(|example| example.label.as_str() == "rank_high"));
    assert!(path_examples
        .iter()
        .any(|example| example.label.as_str() == "rank_low"));
    assert!(memory_examples
        .iter()
        .any(|example| example.label.as_str() == "promote"));
    assert!(memory_examples
        .iter()
        .any(|example| example.label.as_str() == "hold"));
}

fn fixture_corpus() -> DistillationCorpus {
    DistillationCorpus::new(vec![
        GraphTruthLabel {
            id: "temporal-contract-question".to_owned(),
            query: "Was the contract active when the lawsuit started?".to_owned(),
            answer_label: "temporal_graph".to_owned(),
            temporal_label: "overlaps".to_owned(),
            contradiction_label: "none".to_owned(),
            source_trust_label: "trusted".to_owned(),
            memory_promotion_label: "promote".to_owned(),
            path_rank_label: "rank_high".to_owned(),
            evidence_assertion_ids: vec![AssertionId::new("assertion-contract-active")],
            source_ids: vec![SourceId::new("source-contract-system")],
            feature_values: vec![0.95, 0.9, 0.1, 0.8, 0.9, 0.7],
        },
        GraphTruthLabel {
            id: "simple-profile-question".to_owned(),
            query: "What is the company headquarters?".to_owned(),
            answer_label: "vector".to_owned(),
            temporal_label: "not_temporal".to_owned(),
            contradiction_label: "none".to_owned(),
            source_trust_label: "trusted".to_owned(),
            memory_promotion_label: "hold".to_owned(),
            path_rank_label: "rank_low".to_owned(),
            evidence_assertion_ids: vec![AssertionId::new("assertion-hq")],
            source_ids: vec![SourceId::new("source-company-profile")],
            feature_values: vec![0.1, 0.2, 0.0, 0.9, 0.2, 0.1],
        },
        GraphTruthLabel {
            id: "conflicting-acquisition-question".to_owned(),
            query: "Did the acquisition close or get blocked?".to_owned(),
            answer_label: "hybrid_contradiction".to_owned(),
            temporal_label: "after".to_owned(),
            contradiction_label: "conflict".to_owned(),
            source_trust_label: "untrusted".to_owned(),
            memory_promotion_label: "hold".to_owned(),
            path_rank_label: "rank_high".to_owned(),
            evidence_assertion_ids: vec![
                AssertionId::new("assertion-acquisition-closed"),
                AssertionId::new("assertion-acquisition-blocked"),
            ],
            source_ids: vec![
                SourceId::new("source-regulator"),
                SourceId::new("source-rumor"),
            ],
            feature_values: vec![0.8, 0.7, 1.0, 0.35, 0.4, 0.8],
        },
    ])
}

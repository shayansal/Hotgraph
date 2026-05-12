use rg_agent_judge::JudgeScores;
use rg_core::{AgentId, AssertionId, MemoryId, SourceId, TxTime};
use rg_feedback_loop::{
    AgentSuccessSignal, EvidenceUsefulnessSignal, FeedbackLoop, FeedbackLoopError,
    MemoryWriteQualitySignal, OutcomeObservation, RetrievalPolicyUpdateCandidate, SignalPolarity,
    TrainingDataExportJob,
};
use rg_learning::RetrievalPolicyId;
use rg_training_data::{ExportFormat, TrainingTaskKind};

#[test]
fn outcome_observation_records_model_graph_outcome_and_oracle_score() {
    let observation = OutcomeObservation::new(
        "outcome-1",
        AgentId::new("agent-research"),
        "answer-with-evidence",
        "Who owned HelioFab on 2024-01-01?",
        TxTime::new(1_778_454_000),
        JudgeScores {
            correctness: 0.92,
            evidence_faithfulness: 0.88,
            temporal_correctness: 0.95,
            hallucination_score: 0.9,
            missing_context_score: 0.8,
            unsafe_memory_use: 1.0,
            contradiction_handling: 0.85,
        },
    )
    .with_answer("HelioFab was owned by Northstar, citing source-a.")
    .with_sources(vec![SourceId::new("source-a")])
    .with_assertions(vec![AssertionId::new("assertion-owner")])
    .with_latency_and_cost(184, 0.014);

    assert_eq!(observation.id, "outcome-1");
    assert_eq!(observation.agent_id.as_str(), "agent-research");
    assert!(observation.eval_passed());
    assert!(observation.reward() > 0.8);
    assert_eq!(observation.used_sources[0].as_str(), "source-a");
}

#[test]
fn feedback_loop_rolls_outcome_into_success_evidence_and_memory_signals() {
    let observation = sample_observation(0.91);
    let mut loop_state = FeedbackLoop::new();

    let event = loop_state.record_observation(observation.clone());
    loop_state.record_agent_success(AgentSuccessSignal::from_observation(&observation));
    loop_state.record_evidence_usefulness(EvidenceUsefulnessSignal::source_clicked(
        observation.id.clone(),
        SourceId::new("source-a"),
    ));
    loop_state.record_memory_quality(MemoryWriteQualitySignal::accepted(
        observation.id.clone(),
        MemoryId::new("memory-a"),
        0.93,
    ));

    let summary = loop_state.summary();
    assert_eq!(event.id, "feedback-loop-outcome-1");
    assert_eq!(summary.observations, 1);
    assert_eq!(summary.success_signals, 1);
    assert_eq!(summary.evidence_signals, 1);
    assert_eq!(summary.memory_quality_signals, 1);
    assert!(summary.mean_reward > 0.8);
}

#[test]
fn retrieval_policy_candidate_is_blocked_without_eval_gate_improvement() {
    let candidate = RetrievalPolicyUpdateCandidate::new(
        RetrievalPolicyId::new("policy-current"),
        RetrievalPolicyId::new("policy-candidate"),
        0.812,
        0.811,
        120,
    )
    .with_rationale("candidate regressed on temporal QA");

    assert!(!candidate.eval_gate_passed());
    assert_eq!(
        candidate.approve_for_deployment(),
        Err(FeedbackLoopError::EvalGateRejected)
    );
}

#[test]
fn retrieval_policy_candidate_requires_positive_eval_delta_before_update() {
    let candidate = RetrievalPolicyUpdateCandidate::new(
        RetrievalPolicyId::new("policy-current"),
        RetrievalPolicyId::new("policy-candidate"),
        0.812,
        0.829,
        120,
    )
    .with_latency_delta_ms(-14)
    .with_rationale("candidate improves evidence recall without latency regression");

    let approval = candidate.approve_for_deployment().expect("gate passes");

    assert_eq!(approval.candidate_policy_id.as_str(), "policy-candidate");
    assert_eq!(approval.example_count, 120);
    assert!(approval.eval_delta > 0.0);
}

#[test]
fn training_export_job_collects_model_improvement_examples_without_auto_policy_update() {
    let mut loop_state = FeedbackLoop::new();
    loop_state.record_observation(sample_observation(0.74));
    loop_state.record_agent_success(AgentSuccessSignal::failed(
        "outcome-1",
        "tool outcome contradicted answer",
    ));
    loop_state.record_evidence_usefulness(EvidenceUsefulnessSignal::missing_evidence(
        "outcome-1",
        "expected SEC filing was absent from context pack",
    ));
    loop_state.record_memory_quality(MemoryWriteQualitySignal::rejected(
        "outcome-1",
        MemoryId::new("memory-a"),
        "memory collapsed disputed ownership into current truth",
    ));

    let job = TrainingDataExportJob::from_feedback_loop(
        "job-1",
        &loop_state,
        TrainingTaskKind::EvidencePackSft,
        ExportFormat::Jsonl,
    )
    .expect("training job");

    assert_eq!(job.id, "job-1");
    assert_eq!(job.task_kind, TrainingTaskKind::EvidencePackSft);
    assert_eq!(job.format, ExportFormat::Jsonl);
    assert_eq!(job.examples.len(), 1);
    assert!(job.examples[0].input_task.contains("Who owned"));
    assert!(job.requires_eval_gate);
    assert!(!job.applies_model_policy_update);
}

#[test]
fn signal_polarity_drives_compounding_loop_recommendations() {
    let success = AgentSuccessSignal::succeeded("outcome-1", "tool outcome matched evidence");
    let missing =
        EvidenceUsefulnessSignal::missing_evidence("outcome-1", "source was not retrieved");
    let memory_rejected =
        MemoryWriteQualitySignal::rejected("outcome-1", MemoryId::new("memory-b"), "unsupported");

    assert_eq!(success.polarity(), SignalPolarity::Positive);
    assert_eq!(missing.polarity(), SignalPolarity::Negative);
    assert_eq!(memory_rejected.polarity(), SignalPolarity::Negative);
}

fn sample_observation(score: f32) -> OutcomeObservation {
    OutcomeObservation::new(
        "outcome-1",
        AgentId::new("agent-research"),
        "verify",
        "Who owned HelioFab on 2024-01-01?",
        TxTime::new(1_778_454_000),
        JudgeScores {
            correctness: score,
            evidence_faithfulness: score,
            temporal_correctness: score,
            hallucination_score: score,
            missing_context_score: score,
            unsafe_memory_use: 1.0,
            contradiction_handling: score,
        },
    )
    .with_answer("HelioFab ownership was disputed; cite source-a.")
    .with_sources(vec![SourceId::new("source-a")])
    .with_assertions(vec![AssertionId::new("assertion-owner")])
    .with_latency_and_cost(184, 0.014)
}

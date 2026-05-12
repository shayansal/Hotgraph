use rg_ai::EvidencePack;
use rg_core::{AssertionId, ContradictionId, SourceId, TxTime};
use rg_learning::{
    BanditRouter, DecisionLog, FeedbackActor, FeedbackEvent, FeedbackSignal, FeedbackStore,
    LearningError, LinearRankingPolicy, OfflineEvaluator, PolicyUpdateGate, RankingFeature,
    RankingTarget, RetrievalDecision, RetrievalOutcome, RetrievalPolicyId, TrainingExample,
};
use rg_retrieval_compiler::{
    QueryIntent, RetrievalBudget, RetrievalOperator, RetrievalPlan, RetrievalTrace,
    RetrievalTraceStep,
};

#[test]
fn every_evidence_pack_can_receive_feedback() {
    let pack = evidence_pack("Where did Alice work?");
    let mut store = FeedbackStore::new();

    let clicked = store.record_for_pack(
        &pack,
        FeedbackSignal::UserClickedSource {
            source_id: SourceId::new("source-employment"),
        },
        FeedbackActor::User("user-1".to_owned()),
        TxTime::new(10),
    );
    let missing = store.record_for_pack(
        &pack,
        FeedbackSignal::HumanMarkedEvidenceMissing {
            description: "missing 2024 employment filing".to_owned(),
        },
        FeedbackActor::HumanReviewer("reviewer-1".to_owned()),
        TxTime::new(11),
    );

    assert_eq!(clicked.evidence_pack_id, missing.evidence_pack_id);
    assert_eq!(store.feedback_for_pack(&pack), &[clicked, missing]);
}

#[test]
fn retrieval_decisions_are_logged_with_ranker_features_and_become_training_examples() {
    let pack = evidence_pack("Find contradictions about Alice");
    let decision = retrieval_decision(&pack, "policy-baseline");
    let mut log = DecisionLog::new();
    log.record(decision.clone()).expect("decision logged");
    let feedback = vec![
        FeedbackEvent::new(
            &pack,
            FeedbackSignal::HumanMarkedEvidenceIrrelevant {
                assertion_id: Some(AssertionId::new("assertion-stale")),
                source_id: None,
            },
            FeedbackActor::HumanReviewer("reviewer-1".to_owned()),
            TxTime::new(20),
        ),
        FeedbackEvent::new(
            &pack,
            FeedbackSignal::ContradictionLaterDiscovered {
                contradiction_id: ContradictionId::new("contradiction-1"),
            },
            FeedbackActor::System("maintenance".to_owned()),
            TxTime::new(21),
        ),
    ];

    let example = TrainingExample::from_decision_feedback_and_outcome(
        &decision,
        &feedback,
        Some(RetrievalOutcome {
            id: "outcome-1".to_owned(),
            evidence_pack_id: decision.evidence_pack_id.clone(),
            agent_succeeded: false,
            answer_accepted: false,
            latency_micros: 900,
            cost_units: 2.5,
            notes: Some("agent failed after stale evidence".to_owned()),
        }),
    );

    assert_eq!(log.decisions_for_pack(&pack), &[decision]);
    assert!(example
        .features
        .iter()
        .any(|feature| feature.target == RankingTarget::RetrievalRouter));
    assert!(example
        .features
        .iter()
        .any(|feature| feature.target == RankingTarget::SourceRanker));
    assert!(example.label < 0.5);
    assert_eq!(example.feedback_events.len(), 2);
}

#[test]
fn offline_replay_compares_old_and_new_retrieval_policies() {
    let pack = evidence_pack("Find source backed employment");
    let mut decision = retrieval_decision(&pack, "policy-baseline");
    decision.features = vec![
        RankingFeature::new(RankingTarget::RetrievalRouter, "temporal_query", 1.0),
        RankingFeature::new(RankingTarget::SourceRanker, "trusted_source_clicked", 1.0),
        RankingFeature::new(RankingTarget::CompressionPolicy, "compact_pack", 1.0),
    ];
    let feedback = vec![FeedbackEvent::new(
        &pack,
        FeedbackSignal::AgentSucceededAfterUsingContext {
            outcome_id: "outcome-success".to_owned(),
        },
        FeedbackActor::Agent("agent-1".to_owned()),
        TxTime::new(30),
    )];
    let example = TrainingExample::from_decision_feedback_and_outcome(&decision, &feedback, None);
    let evaluator = OfflineEvaluator::new(vec![example]);

    let baseline = LinearRankingPolicy::new("baseline").with_weight(
        RankingTarget::RetrievalRouter,
        "temporal_query",
        0.1,
    );
    let candidate = LinearRankingPolicy::new("candidate")
        .with_weight(RankingTarget::RetrievalRouter, "temporal_query", 0.5)
        .with_weight(RankingTarget::SourceRanker, "trusted_source_clicked", 0.4)
        .with_weight(RankingTarget::CompressionPolicy, "compact_pack", 0.1);
    let report = evaluator.compare(&baseline, &candidate);

    assert_eq!(report.baseline_policy_id.as_str(), "baseline");
    assert_eq!(report.candidate_policy_id.as_str(), "candidate");
    assert!(report.candidate.mean_reward > report.baseline.mean_reward);
    assert!(report.reward_delta > 0.0);
}

#[test]
fn no_model_update_is_deployed_without_eval_improvement() {
    let pack = evidence_pack("Find source backed employment");
    let decision = retrieval_decision(&pack, "policy-baseline");
    let feedback = vec![FeedbackEvent::new(
        &pack,
        FeedbackSignal::UserClickedSource {
            source_id: SourceId::new("source-employment"),
        },
        FeedbackActor::User("user-1".to_owned()),
        TxTime::new(40),
    )];
    let evaluator =
        OfflineEvaluator::new(vec![TrainingExample::from_decision_feedback_and_outcome(
            &decision, &feedback, None,
        )]);
    let baseline = LinearRankingPolicy::new("baseline").with_weight(
        RankingTarget::SourceRanker,
        "source_count",
        0.5,
    );
    let no_better = LinearRankingPolicy::new("candidate-no-better").with_weight(
        RankingTarget::SourceRanker,
        "source_count",
        0.5,
    );
    let improved = LinearRankingPolicy::new("candidate-improved").with_weight(
        RankingTarget::SourceRanker,
        "source_count",
        1.0,
    );
    let gate = PolicyUpdateGate::default();
    let mut router = BanditRouter::new(RetrievalPolicyId::new("baseline"));

    let rejected = router.deploy_if_improved(evaluator.compare(&baseline, &no_better), &gate);
    assert_eq!(rejected, Err(LearningError::NoEvalImprovement));
    assert_eq!(router.active_policy_id().as_str(), "baseline");

    let deployed = router
        .deploy_if_improved(evaluator.compare(&baseline, &improved), &gate)
        .expect("improved policy deploys");
    assert_eq!(deployed.active_policy_id.as_str(), "candidate-improved");
    assert_eq!(router.active_policy_id().as_str(), "candidate-improved");
}

#[test]
fn bandit_router_placeholder_tracks_outcomes_without_training_a_model() {
    let mut router = BanditRouter::new(RetrievalPolicyId::new("baseline"));
    router.register_policy(RetrievalPolicyId::new("candidate"));

    router.record_outcome(&RetrievalPolicyId::new("candidate"), 1.0);
    router.record_outcome(&RetrievalPolicyId::new("baseline"), 0.2);

    let selected = router.select_policy();
    assert_eq!(selected.as_str(), "candidate");
    assert_eq!(
        router
            .arm(&RetrievalPolicyId::new("candidate"))
            .unwrap()
            .pulls,
        1
    );
    assert_eq!(
        router
            .arm(&RetrievalPolicyId::new("candidate"))
            .unwrap()
            .mean_reward(),
        1.0
    );
}

fn evidence_pack(query: &str) -> EvidencePack {
    EvidencePack {
        query: query.to_owned(),
        entities: Vec::new(),
        assertions: Vec::new(),
        sources: Vec::new(),
        paths: Vec::new(),
        contradictions: Vec::new(),
        generated_at: TxTime::new(1),
    }
}

fn retrieval_decision(pack: &EvidencePack, policy_id: &str) -> RetrievalDecision {
    RetrievalDecision {
        id: "decision-1".to_owned(),
        evidence_pack_id: rg_learning::EvidencePackId::from_pack(pack),
        policy_id: RetrievalPolicyId::new(policy_id),
        query: pack.query.clone(),
        plan: RetrievalPlan {
            intent: QueryIntent::Historical,
            operators: vec![
                RetrievalOperator::TemporalFilter,
                RetrievalOperator::GraphExpansion,
                RetrievalOperator::Cite,
            ],
            budget: RetrievalBudget::default(),
        },
        trace: RetrievalTrace {
            steps: vec![RetrievalTraceStep {
                operator: RetrievalOperator::TemporalFilter,
                reason: "valid and known time present".to_owned(),
            }],
        },
        features: vec![
            RankingFeature::new(RankingTarget::RetrievalRouter, "temporal_query", 1.0),
            RankingFeature::new(RankingTarget::PathRanker, "path_count", 0.0),
            RankingFeature::new(RankingTarget::SourceRanker, "source_count", 1.0),
            RankingFeature::new(RankingTarget::MemoryRanker, "memory_count", 0.0),
            RankingFeature::new(RankingTarget::SummaryRanker, "summary_available", 0.0),
            RankingFeature::new(RankingTarget::CompressionPolicy, "compact_pack", 1.0),
        ],
    }
}

//! Learning interfaces for retrieval feedback and offline policy evaluation.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use rg_ai::EvidencePack;
use rg_core::{AssertionId, ContradictionId, SourceId, TxTime};
use rg_retrieval_compiler::{RetrievalPlan, RetrievalTrace};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EvidencePackId(String);

impl EvidencePackId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn from_pack(pack: &EvidencePack) -> Self {
        let mut input = format!(
            "query={}|generated_at={}",
            pack.query,
            pack.generated_at.as_i64()
        );
        for assertion in &pack.assertions {
            input.push_str("|assertion=");
            input.push_str(assertion.id.as_str());
        }
        for source in &pack.sources {
            input.push_str("|source=");
            input.push_str(source.source_id.as_str());
        }
        for path in &pack.paths {
            input.push_str("|path=");
            input.push_str(path.start.as_str());
            input.push_str("->");
            input.push_str(path.end.as_str());
        }
        for contradiction in &pack.contradictions {
            input.push_str("|contradiction=");
            input.push_str(contradiction.id.as_str());
        }
        Self(format!("epack-{:016x}", stable_hash(input.as_bytes())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EvidencePackId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RetrievalPolicyId(String);

impl RetrievalPolicyId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RetrievalPolicyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeedbackActor {
    User(String),
    Agent(String),
    HumanReviewer(String),
    System(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeedbackSignal {
    UserClickedSource {
        source_id: SourceId,
    },
    UserIgnoredAnswer,
    UserCorrectedAnswer {
        correction: String,
    },
    AgentSucceededAfterUsingContext {
        outcome_id: String,
    },
    AgentFailedAfterUsingContext {
        outcome_id: String,
    },
    HumanMarkedEvidenceIrrelevant {
        assertion_id: Option<AssertionId>,
        source_id: Option<SourceId>,
    },
    HumanMarkedEvidenceMissing {
        description: String,
    },
    ContradictionLaterDiscovered {
        contradiction_id: ContradictionId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedbackEvent {
    pub id: String,
    pub evidence_pack_id: EvidencePackId,
    pub signal: FeedbackSignal,
    pub actor: FeedbackActor,
    pub observed_at: TxTime,
}

impl FeedbackEvent {
    pub fn new(
        pack: &EvidencePack,
        signal: FeedbackSignal,
        actor: FeedbackActor,
        observed_at: TxTime,
    ) -> Self {
        let evidence_pack_id = EvidencePackId::from_pack(pack);
        let id = feedback_id(&evidence_pack_id, &signal, &actor, observed_at);
        Self {
            id,
            evidence_pack_id,
            signal,
            actor,
            observed_at,
        }
    }

    fn reward_delta(&self) -> f64 {
        match &self.signal {
            FeedbackSignal::UserClickedSource { .. }
            | FeedbackSignal::AgentSucceededAfterUsingContext { .. } => 0.35,
            FeedbackSignal::UserIgnoredAnswer
            | FeedbackSignal::AgentFailedAfterUsingContext { .. }
            | FeedbackSignal::HumanMarkedEvidenceIrrelevant { .. } => -0.35,
            FeedbackSignal::UserCorrectedAnswer { .. }
            | FeedbackSignal::HumanMarkedEvidenceMissing { .. }
            | FeedbackSignal::ContradictionLaterDiscovered { .. } => -0.25,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FeedbackStore {
    events_by_pack: BTreeMap<EvidencePackId, Vec<FeedbackEvent>>,
}

impl FeedbackStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_for_pack(
        &mut self,
        pack: &EvidencePack,
        signal: FeedbackSignal,
        actor: FeedbackActor,
        observed_at: TxTime,
    ) -> FeedbackEvent {
        let event = FeedbackEvent::new(pack, signal, actor, observed_at);
        self.record_event(event.clone());
        event
    }

    pub fn record_event(&mut self, event: FeedbackEvent) {
        let events = self
            .events_by_pack
            .entry(event.evidence_pack_id.clone())
            .or_default();
        events.push(event);
        events.sort_by(|left, right| {
            left.observed_at
                .cmp(&right.observed_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        events.dedup_by(|left, right| left.id == right.id);
    }

    pub fn feedback_for_pack(&self, pack: &EvidencePack) -> &[FeedbackEvent] {
        self.feedback_for_pack_id(&EvidencePackId::from_pack(pack))
    }

    pub fn feedback_for_pack_id(&self, pack_id: &EvidencePackId) -> &[FeedbackEvent] {
        self.events_by_pack.get(pack_id).map_or(&[], Vec::as_slice)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RankingTarget {
    RetrievalRouter,
    PathRanker,
    SourceRanker,
    MemoryRanker,
    SummaryRanker,
    CompressionPolicy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RankingFeature {
    pub target: RankingTarget,
    pub name: String,
    pub value: f64,
}

impl RankingFeature {
    pub fn new(target: RankingTarget, name: impl Into<String>, value: f64) -> Self {
        Self {
            target,
            name: name.into(),
            value,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetrievalDecision {
    pub id: String,
    pub evidence_pack_id: EvidencePackId,
    pub policy_id: RetrievalPolicyId,
    pub query: String,
    pub plan: RetrievalPlan,
    pub trace: RetrievalTrace,
    pub features: Vec<RankingFeature>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DecisionLog {
    decisions_by_pack: BTreeMap<EvidencePackId, Vec<RetrievalDecision>>,
}

impl DecisionLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, decision: RetrievalDecision) -> Result<(), LearningError> {
        if decision.features.is_empty() {
            return Err(LearningError::MissingFeatures);
        }
        let decisions = self
            .decisions_by_pack
            .entry(decision.evidence_pack_id.clone())
            .or_default();
        decisions.push(decision);
        decisions.sort_by(|left, right| left.id.cmp(&right.id));
        decisions.dedup_by(|left, right| left.id == right.id);
        Ok(())
    }

    pub fn decisions_for_pack(&self, pack: &EvidencePack) -> &[RetrievalDecision] {
        self.decisions_for_pack_id(&EvidencePackId::from_pack(pack))
    }

    pub fn decisions_for_pack_id(&self, pack_id: &EvidencePackId) -> &[RetrievalDecision] {
        self.decisions_by_pack
            .get(pack_id)
            .map_or(&[], Vec::as_slice)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetrievalOutcome {
    pub id: String,
    pub evidence_pack_id: EvidencePackId,
    pub agent_succeeded: bool,
    pub answer_accepted: bool,
    pub latency_micros: u64,
    pub cost_units: f64,
    pub notes: Option<String>,
}

impl RetrievalOutcome {
    fn reward_delta(&self) -> f64 {
        let mut reward = 0.0;
        if self.agent_succeeded {
            reward += 0.35;
        } else {
            reward -= 0.25;
        }
        if self.answer_accepted {
            reward += 0.25;
        }
        reward
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrainingExample {
    pub id: String,
    pub decision_id: String,
    pub evidence_pack_id: EvidencePackId,
    pub policy_id: RetrievalPolicyId,
    pub query: String,
    pub features: Vec<RankingFeature>,
    pub label: f64,
    pub feedback_events: Vec<FeedbackEvent>,
    pub outcome: Option<RetrievalOutcome>,
}

impl TrainingExample {
    pub fn from_decision_feedback_and_outcome(
        decision: &RetrievalDecision,
        feedback_events: &[FeedbackEvent],
        outcome: Option<RetrievalOutcome>,
    ) -> Self {
        let mut label = 0.5;
        for event in feedback_events {
            label += event.reward_delta();
        }
        if let Some(outcome) = &outcome {
            label += outcome.reward_delta();
        }
        let label = label.clamp(0.0, 1.0);
        Self {
            id: format!("training-{}", decision.id),
            decision_id: decision.id.clone(),
            evidence_pack_id: decision.evidence_pack_id.clone(),
            policy_id: decision.policy_id.clone(),
            query: decision.query.clone(),
            features: decision.features.clone(),
            label,
            feedback_events: feedback_events.to_vec(),
            outcome,
        }
    }
}

pub trait LearningPolicy {
    fn policy_id(&self) -> &RetrievalPolicyId;
    fn score(&self, example: &TrainingExample) -> f64;
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinearRankingPolicy {
    policy_id: RetrievalPolicyId,
    weights: BTreeMap<FeatureKey, f64>,
    bias: f64,
}

impl LinearRankingPolicy {
    pub fn new(policy_id: impl Into<String>) -> Self {
        Self {
            policy_id: RetrievalPolicyId::new(policy_id),
            weights: BTreeMap::new(),
            bias: 0.0,
        }
    }

    pub fn with_bias(mut self, bias: f64) -> Self {
        self.bias = bias;
        self
    }

    pub fn with_weight(
        mut self,
        target: RankingTarget,
        name: impl Into<String>,
        weight: f64,
    ) -> Self {
        self.weights.insert(FeatureKey::new(target, name), weight);
        self
    }
}

impl LearningPolicy for LinearRankingPolicy {
    fn policy_id(&self) -> &RetrievalPolicyId {
        &self.policy_id
    }

    fn score(&self, example: &TrainingExample) -> f64 {
        let mut score = self.bias;
        for feature in &example.features {
            let key = FeatureKey::new(feature.target, feature.name.clone());
            score += self.weights.get(&key).copied().unwrap_or(0.0) * feature.value;
        }
        score.clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct FeatureKey {
    target: RankingTarget,
    name: String,
}

impl FeatureKey {
    fn new(target: RankingTarget, name: impl Into<String>) -> Self {
        Self {
            target,
            name: name.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OfflineEvaluator {
    examples: Vec<TrainingExample>,
}

impl OfflineEvaluator {
    pub fn new(examples: Vec<TrainingExample>) -> Self {
        Self { examples }
    }

    pub fn compare(
        &self,
        baseline: &dyn LearningPolicy,
        candidate: &dyn LearningPolicy,
    ) -> OfflineComparisonReport {
        let baseline_metrics = self.evaluate(baseline);
        let candidate_metrics = self.evaluate(candidate);
        OfflineComparisonReport {
            baseline_policy_id: baseline.policy_id().clone(),
            candidate_policy_id: candidate.policy_id().clone(),
            reward_delta: candidate_metrics.mean_reward - baseline_metrics.mean_reward,
            baseline: baseline_metrics,
            candidate: candidate_metrics,
            examples: self.examples.clone(),
        }
    }

    fn evaluate(&self, policy: &dyn LearningPolicy) -> OfflineMetrics {
        let case_count = self.examples.len();
        if self.examples.is_empty() {
            return OfflineMetrics::default();
        }
        let mut reward_sum = 0.0;
        let mut absolute_error_sum = 0.0;
        let mut clicked_source_count = 0;
        let mut correction_count = 0;
        for example in &self.examples {
            let score = policy.score(example);
            reward_sum += 1.0 - (example.label - score).abs();
            absolute_error_sum += (example.label - score).abs();
            clicked_source_count += example
                .feedback_events
                .iter()
                .filter(|event| matches!(event.signal, FeedbackSignal::UserClickedSource { .. }))
                .count();
            correction_count += example
                .feedback_events
                .iter()
                .filter(|event| {
                    matches!(
                        event.signal,
                        FeedbackSignal::UserCorrectedAnswer { .. }
                            | FeedbackSignal::HumanMarkedEvidenceMissing { .. }
                            | FeedbackSignal::HumanMarkedEvidenceIrrelevant { .. }
                            | FeedbackSignal::ContradictionLaterDiscovered { .. }
                    )
                })
                .count();
        }
        OfflineMetrics {
            case_count,
            mean_reward: reward_sum / case_count as f64,
            mean_absolute_error: absolute_error_sum / case_count as f64,
            clicked_source_count,
            correction_count,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OfflineMetrics {
    pub case_count: usize,
    pub mean_reward: f64,
    pub mean_absolute_error: f64,
    pub clicked_source_count: usize,
    pub correction_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OfflineComparisonReport {
    pub baseline_policy_id: RetrievalPolicyId,
    pub candidate_policy_id: RetrievalPolicyId,
    pub baseline: OfflineMetrics,
    pub candidate: OfflineMetrics,
    pub reward_delta: f64,
    pub examples: Vec<TrainingExample>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolicyUpdateGate {
    min_reward_delta: f64,
}

impl Default for PolicyUpdateGate {
    fn default() -> Self {
        Self {
            min_reward_delta: 0.000_001,
        }
    }
}

impl PolicyUpdateGate {
    pub fn new(min_reward_delta: f64) -> Self {
        Self { min_reward_delta }
    }

    pub fn allows(&self, report: &OfflineComparisonReport) -> bool {
        report.reward_delta > self.min_reward_delta
            && report.candidate.mean_reward > report.baseline.mean_reward
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BanditRouter {
    active_policy_id: RetrievalPolicyId,
    arms: BTreeMap<RetrievalPolicyId, BanditArm>,
}

impl BanditRouter {
    pub fn new(active_policy_id: RetrievalPolicyId) -> Self {
        let mut arms = BTreeMap::new();
        arms.insert(active_policy_id.clone(), BanditArm::default());
        Self {
            active_policy_id,
            arms,
        }
    }

    pub fn active_policy_id(&self) -> &RetrievalPolicyId {
        &self.active_policy_id
    }

    pub fn register_policy(&mut self, policy_id: RetrievalPolicyId) {
        self.arms.entry(policy_id).or_default();
    }

    pub fn select_policy(&self) -> &RetrievalPolicyId {
        self.arms
            .iter()
            .max_by(|left, right| {
                left.1
                    .mean_reward()
                    .total_cmp(&right.1.mean_reward())
                    .then_with(|| right.0.cmp(left.0))
            })
            .map(|(policy_id, _)| policy_id)
            .unwrap_or(&self.active_policy_id)
    }

    pub fn record_outcome(&mut self, policy_id: &RetrievalPolicyId, reward: f64) {
        let arm = self.arms.entry(policy_id.clone()).or_default();
        arm.pulls += 1;
        arm.reward_sum += reward.clamp(0.0, 1.0);
    }

    pub fn arm(&self, policy_id: &RetrievalPolicyId) -> Option<&BanditArm> {
        self.arms.get(policy_id)
    }

    pub fn deploy_if_improved(
        &mut self,
        report: OfflineComparisonReport,
        gate: &PolicyUpdateGate,
    ) -> Result<PolicyDeployment, LearningError> {
        if !gate.allows(&report) {
            return Err(LearningError::NoEvalImprovement);
        }
        self.active_policy_id = report.candidate_policy_id.clone();
        self.register_policy(report.candidate_policy_id.clone());
        Ok(PolicyDeployment {
            active_policy_id: report.candidate_policy_id,
            reward_delta: report.reward_delta,
            deployed_example_count: report.examples.len(),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BanditArm {
    pub pulls: usize,
    pub reward_sum: f64,
}

impl BanditArm {
    pub fn mean_reward(&self) -> f64 {
        if self.pulls == 0 {
            0.0
        } else {
            self.reward_sum / self.pulls as f64
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolicyDeployment {
    pub active_policy_id: RetrievalPolicyId,
    pub reward_delta: f64,
    pub deployed_example_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LearningError {
    MissingFeatures,
    NoEvalImprovement,
}

impl fmt::Display for LearningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFeatures => {
                formatter.write_str("retrieval decision must include ranking features")
            }
            Self::NoEvalImprovement => {
                formatter.write_str("candidate policy did not improve offline evaluation")
            }
        }
    }
}

impl Error for LearningError {}

fn feedback_id(
    evidence_pack_id: &EvidencePackId,
    signal: &FeedbackSignal,
    actor: &FeedbackActor,
    observed_at: TxTime,
) -> String {
    let raw = format!(
        "{}|{:?}|{:?}|{}",
        evidence_pack_id,
        signal,
        actor,
        observed_at.as_i64()
    );
    format!("feedback-{:016x}", stable_hash(raw.as_bytes()))
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

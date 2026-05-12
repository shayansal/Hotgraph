//! Foundation-model feedback loop orchestration for Reality Graph.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use rg_agent_judge::JudgeScores;
use rg_core::{AgentId, AssertionId, MemoryId, SourceId, TxTime};
use rg_learning::RetrievalPolicyId;
use rg_training_data::{
    Citation, ExportFormat, GraphAttentionExample, GraphAttentionExampleDraft, GraphStateSnapshot,
    RetrievedEvidence, TemporalMetadata, TrainingExampleId, TrainingTaskKind,
};

#[derive(Clone, Debug, PartialEq)]
pub struct OutcomeObservation {
    pub id: String,
    pub agent_id: AgentId,
    pub workflow: String,
    pub task: String,
    pub observed_at: TxTime,
    pub judge_scores: JudgeScores,
    pub model_answer: Option<String>,
    pub used_sources: Vec<SourceId>,
    pub used_assertions: Vec<AssertionId>,
    pub latency_ms: Option<u64>,
    pub cost_usd: Option<f64>,
}

impl OutcomeObservation {
    pub fn new(
        id: impl Into<String>,
        agent_id: AgentId,
        workflow: impl Into<String>,
        task: impl Into<String>,
        observed_at: TxTime,
        judge_scores: JudgeScores,
    ) -> Self {
        Self {
            id: id.into(),
            agent_id,
            workflow: workflow.into(),
            task: task.into(),
            observed_at,
            judge_scores,
            model_answer: None,
            used_sources: Vec::new(),
            used_assertions: Vec::new(),
            latency_ms: None,
            cost_usd: None,
        }
    }

    pub fn with_answer(mut self, answer: impl Into<String>) -> Self {
        self.model_answer = Some(answer.into());
        self
    }

    pub fn with_sources(mut self, source_ids: Vec<SourceId>) -> Self {
        self.used_sources = source_ids;
        self
    }

    pub fn with_assertions(mut self, assertion_ids: Vec<AssertionId>) -> Self {
        self.used_assertions = assertion_ids;
        self
    }

    pub fn with_latency_and_cost(mut self, latency_ms: u64, cost_usd: f64) -> Self {
        self.latency_ms = Some(latency_ms);
        self.cost_usd = Some(cost_usd);
        self
    }

    pub fn eval_passed(&self) -> bool {
        self.judge_scores.mean() >= 0.8
            && self.judge_scores.evidence_faithfulness >= 0.75
            && self.judge_scores.temporal_correctness >= 0.75
            && self.judge_scores.unsafe_memory_use >= 0.75
    }

    pub fn reward(&self) -> f64 {
        let latency_penalty = self
            .latency_ms
            .map(|latency| (latency.saturating_sub(500) as f64 / 2_000.0).min(0.2))
            .unwrap_or(0.0);
        let cost_penalty = self
            .cost_usd
            .map(|cost| ((cost - 0.05).max(0.0) / 0.5).min(0.15))
            .unwrap_or(0.0);
        bounded_f64(self.judge_scores.mean() as f64 - latency_penalty - cost_penalty)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutcomeFeedbackEvent {
    pub id: String,
    pub outcome_id: String,
    pub observed_at: TxTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalPolarity {
    Positive,
    Negative,
    Neutral,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentSuccessSignal {
    pub outcome_id: String,
    pub succeeded: bool,
    pub reason: String,
}

impl AgentSuccessSignal {
    pub fn from_observation(observation: &OutcomeObservation) -> Self {
        if observation.eval_passed() {
            Self::succeeded(
                observation.id.clone(),
                "eval oracle accepted the answer and evidence",
            )
        } else {
            Self::failed(
                observation.id.clone(),
                "eval oracle rejected at least one answer dimension",
            )
        }
    }

    pub fn succeeded(outcome_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            outcome_id: outcome_id.into(),
            succeeded: true,
            reason: reason.into(),
        }
    }

    pub fn failed(outcome_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            outcome_id: outcome_id.into(),
            succeeded: false,
            reason: reason.into(),
        }
    }

    pub fn polarity(&self) -> SignalPolarity {
        if self.succeeded {
            SignalPolarity::Positive
        } else {
            SignalPolarity::Negative
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvidenceUsefulnessKind {
    SourceClicked(SourceId),
    SourceIgnored(SourceId),
    MissingEvidence(String),
    IrrelevantEvidence(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceUsefulnessSignal {
    pub outcome_id: String,
    pub kind: EvidenceUsefulnessKind,
}

impl EvidenceUsefulnessSignal {
    pub fn source_clicked(outcome_id: impl Into<String>, source_id: SourceId) -> Self {
        Self {
            outcome_id: outcome_id.into(),
            kind: EvidenceUsefulnessKind::SourceClicked(source_id),
        }
    }

    pub fn source_ignored(outcome_id: impl Into<String>, source_id: SourceId) -> Self {
        Self {
            outcome_id: outcome_id.into(),
            kind: EvidenceUsefulnessKind::SourceIgnored(source_id),
        }
    }

    pub fn missing_evidence(outcome_id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            outcome_id: outcome_id.into(),
            kind: EvidenceUsefulnessKind::MissingEvidence(description.into()),
        }
    }

    pub fn irrelevant_evidence(
        outcome_id: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            outcome_id: outcome_id.into(),
            kind: EvidenceUsefulnessKind::IrrelevantEvidence(description.into()),
        }
    }

    pub fn polarity(&self) -> SignalPolarity {
        match self.kind {
            EvidenceUsefulnessKind::SourceClicked(_) => SignalPolarity::Positive,
            EvidenceUsefulnessKind::SourceIgnored(_)
            | EvidenceUsefulnessKind::MissingEvidence(_)
            | EvidenceUsefulnessKind::IrrelevantEvidence(_) => SignalPolarity::Negative,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryWriteQualitySignal {
    pub outcome_id: String,
    pub memory_id: MemoryId,
    pub accepted: bool,
    pub quality_score: Option<f32>,
    pub reason: String,
}

impl MemoryWriteQualitySignal {
    pub fn accepted(
        outcome_id: impl Into<String>,
        memory_id: MemoryId,
        quality_score: f32,
    ) -> Self {
        Self {
            outcome_id: outcome_id.into(),
            memory_id,
            accepted: true,
            quality_score: Some(quality_score.clamp(0.0, 1.0)),
            reason: "memory write was accepted for promotion".to_owned(),
        }
    }

    pub fn rejected(
        outcome_id: impl Into<String>,
        memory_id: MemoryId,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            outcome_id: outcome_id.into(),
            memory_id,
            accepted: false,
            quality_score: None,
            reason: reason.into(),
        }
    }

    pub fn polarity(&self) -> SignalPolarity {
        if self.accepted {
            SignalPolarity::Positive
        } else {
            SignalPolarity::Negative
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeedbackLoop {
    observations: BTreeMap<String, OutcomeObservation>,
    agent_success_signals: Vec<AgentSuccessSignal>,
    evidence_usefulness_signals: Vec<EvidenceUsefulnessSignal>,
    memory_write_quality_signals: Vec<MemoryWriteQualitySignal>,
}

impl FeedbackLoop {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_observation(&mut self, observation: OutcomeObservation) -> OutcomeFeedbackEvent {
        let event = OutcomeFeedbackEvent {
            id: format!("feedback-loop-{}", observation.id),
            outcome_id: observation.id.clone(),
            observed_at: observation.observed_at,
        };
        self.observations
            .insert(observation.id.clone(), observation);
        event
    }

    pub fn record_agent_success(&mut self, signal: AgentSuccessSignal) {
        self.agent_success_signals.push(signal);
    }

    pub fn record_evidence_usefulness(&mut self, signal: EvidenceUsefulnessSignal) {
        self.evidence_usefulness_signals.push(signal);
    }

    pub fn record_memory_quality(&mut self, signal: MemoryWriteQualitySignal) {
        self.memory_write_quality_signals.push(signal);
    }

    pub fn observations(&self) -> impl Iterator<Item = &OutcomeObservation> {
        self.observations.values()
    }

    pub fn summary(&self) -> FeedbackLoopSummary {
        let observations = self.observations.len();
        let reward_sum = self
            .observations
            .values()
            .map(OutcomeObservation::reward)
            .sum::<f64>();
        FeedbackLoopSummary {
            observations,
            success_signals: self.agent_success_signals.len(),
            evidence_signals: self.evidence_usefulness_signals.len(),
            memory_quality_signals: self.memory_write_quality_signals.len(),
            mean_reward: if observations == 0 {
                0.0
            } else {
                reward_sum / observations as f64
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FeedbackLoopSummary {
    pub observations: usize,
    pub success_signals: usize,
    pub evidence_signals: usize,
    pub memory_quality_signals: usize,
    pub mean_reward: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetrievalPolicyUpdateCandidate {
    pub baseline_policy_id: RetrievalPolicyId,
    pub candidate_policy_id: RetrievalPolicyId,
    pub baseline_eval_score: f64,
    pub candidate_eval_score: f64,
    pub example_count: usize,
    pub latency_delta_ms: Option<i64>,
    pub rationale: Option<String>,
}

impl RetrievalPolicyUpdateCandidate {
    pub fn new(
        baseline_policy_id: RetrievalPolicyId,
        candidate_policy_id: RetrievalPolicyId,
        baseline_eval_score: f64,
        candidate_eval_score: f64,
        example_count: usize,
    ) -> Self {
        Self {
            baseline_policy_id,
            candidate_policy_id,
            baseline_eval_score,
            candidate_eval_score,
            example_count,
            latency_delta_ms: None,
            rationale: None,
        }
    }

    pub fn with_latency_delta_ms(mut self, latency_delta_ms: i64) -> Self {
        self.latency_delta_ms = Some(latency_delta_ms);
        self
    }

    pub fn with_rationale(mut self, rationale: impl Into<String>) -> Self {
        self.rationale = Some(rationale.into());
        self
    }

    pub fn eval_delta(&self) -> f64 {
        self.candidate_eval_score - self.baseline_eval_score
    }

    pub fn eval_gate_passed(&self) -> bool {
        self.example_count > 0 && self.eval_delta() > 0.0
    }

    pub fn approve_for_deployment(
        &self,
    ) -> Result<RetrievalPolicyDeploymentApproval, FeedbackLoopError> {
        if !self.eval_gate_passed() {
            return Err(FeedbackLoopError::EvalGateRejected);
        }
        Ok(RetrievalPolicyDeploymentApproval {
            baseline_policy_id: self.baseline_policy_id.clone(),
            candidate_policy_id: self.candidate_policy_id.clone(),
            eval_delta: self.eval_delta(),
            example_count: self.example_count,
            rationale: self.rationale.clone().unwrap_or_else(|| {
                "candidate improved offline eval gate; deployment still requires explicit approval"
                    .to_owned()
            }),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetrievalPolicyDeploymentApproval {
    pub baseline_policy_id: RetrievalPolicyId,
    pub candidate_policy_id: RetrievalPolicyId,
    pub eval_delta: f64,
    pub example_count: usize,
    pub rationale: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrainingDataExportJob {
    pub id: String,
    pub task_kind: TrainingTaskKind,
    pub format: ExportFormat,
    pub examples: Vec<GraphAttentionExample>,
    pub requires_eval_gate: bool,
    pub applies_model_policy_update: bool,
}

impl TrainingDataExportJob {
    pub fn from_feedback_loop(
        id: impl Into<String>,
        feedback_loop: &FeedbackLoop,
        task_kind: TrainingTaskKind,
        format: ExportFormat,
    ) -> Result<Self, FeedbackLoopError> {
        let examples = feedback_loop
            .observations()
            .map(|observation| training_example_from_observation(observation, task_kind))
            .collect::<Vec<_>>();
        if examples.is_empty() {
            return Err(FeedbackLoopError::NoObservations);
        }
        Ok(Self {
            id: id.into(),
            task_kind,
            format,
            examples,
            requires_eval_gate: true,
            applies_model_policy_update: false,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeedbackLoopError {
    EvalGateRejected,
    NoObservations,
}

impl fmt::Display for FeedbackLoopError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EvalGateRejected => {
                formatter.write_str("candidate update was rejected by the eval gate")
            }
            Self::NoObservations => formatter.write_str("training export requires observations"),
        }
    }
}

impl Error for FeedbackLoopError {}

fn training_example_from_observation(
    observation: &OutcomeObservation,
    task_kind: TrainingTaskKind,
) -> GraphAttentionExample {
    let retrieved_evidence = observation
        .used_sources
        .iter()
        .enumerate()
        .map(|(index, source_id)| RetrievedEvidence {
            evidence_id: format!("evidence-{}-{index}", observation.id),
            text: format!(
                "Observed source {} was used by {} for workflow {}.",
                source_id, observation.agent_id, observation.workflow
            ),
            source_id: source_id.as_str().to_owned(),
            assertion_ids: observation
                .used_assertions
                .iter()
                .map(|assertion_id| assertion_id.as_str().to_owned())
                .collect(),
            score: observation.reward() as f32,
        })
        .collect::<Vec<_>>();
    let retrieved_evidence = if retrieved_evidence.is_empty() {
        vec![RetrievedEvidence {
            evidence_id: format!("evidence-{}-missing", observation.id),
            text: "No source-backed evidence was observed for this model outcome.".to_owned(),
            source_id: "missing-source".to_owned(),
            assertion_ids: Vec::new(),
            score: 0.0,
        }]
    } else {
        retrieved_evidence
    };

    let citations = observation
        .used_sources
        .iter()
        .enumerate()
        .map(|(index, source_id)| Citation {
            source_id: source_id.as_str().to_owned(),
            assertion_id: observation
                .used_assertions
                .get(index)
                .or_else(|| observation.used_assertions.first())
                .map(|assertion_id| assertion_id.as_str().to_owned()),
            uri: None,
            quote: "citation preserved from observed answer trace".to_owned(),
        })
        .collect::<Vec<_>>();
    let citations = if citations.is_empty() {
        vec![Citation {
            source_id: "missing-source".to_owned(),
            assertion_id: None,
            uri: None,
            quote: "no citation was observed".to_owned(),
        }]
    } else {
        citations
    };

    let correct_answer = if observation.eval_passed() {
        observation
            .model_answer
            .clone()
            .unwrap_or_else(|| "Observed answer passed the eval oracle.".to_owned())
    } else {
        format!(
            "Improve this answer using source-backed, temporally correct evidence. Original answer: {}",
            observation
                .model_answer
                .as_deref()
                .unwrap_or("no answer was observed")
        )
    };

    let draft = GraphAttentionExampleDraft {
        id: TrainingExampleId::new(format!("feedback-example-{}", observation.id)),
        task_kind,
        input_task: observation.task.clone(),
        graph_state: GraphStateSnapshot {
            entity_ids: Vec::new(),
            assertion_ids: observation
                .used_assertions
                .iter()
                .map(|assertion_id| assertion_id.as_str().to_owned())
                .collect::<Vec<_>>(),
            summary: format!(
                "Feedback observation {} for agent {} in workflow {}.",
                observation.id, observation.agent_id, observation.workflow
            ),
            valid_at: None,
            known_at: Some(observation.observed_at.as_i64()),
        },
        retrieved_evidence,
        correct_answer,
        citations,
        temporal_metadata: TemporalMetadata {
            valid_at: None,
            known_at: Some(observation.observed_at.as_i64()),
            valid_window: None,
            transaction_time: observation.observed_at.as_i64(),
        },
    };
    let example = GraphAttentionExample::new(draft);
    if observation.eval_passed() {
        example
    } else {
        example.with_rejected_answer(
            observation
                .model_answer
                .clone()
                .unwrap_or_else(|| "observed model answer failed the eval oracle".to_owned()),
        )
    }
}

fn bounded_f64(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

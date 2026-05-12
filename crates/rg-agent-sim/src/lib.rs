//! Agent plan simulation lab for Reality Graph.

use std::collections::BTreeSet;

use rg_agent_memory::{AgentMemoryService, MemoryQuery, MemoryRetrievalMode, RetrievedMemory};
use rg_ai::EvidencePack;
use rg_causal::{
    CausalGraph, CausalPath, CausalPathQuery, CounterfactualEngine, CounterfactualScenario,
    ImpactTrace, Intervention,
};
use rg_core::{AgentId, EntityId, EventId, GraphValue, SourceId, TxTime, ValidTime};
use rg_events::GraphState;
use rg_governance::{AccessDenial, AuditReason, GovernanceEngine, Principal};
use rg_retrieval_compiler::{
    AgentState, CompilationRequest, EvidencePackCompiler, RetrievalBudget, RetrievalPlan,
    RetrievalTool,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ActionSensitivity {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProposedAction {
    pub id: String,
    pub actor_agent_id: AgentId,
    pub description: String,
    pub action_type: String,
    pub target_entities: Vec<EntityId>,
    pub related_event: Option<EventId>,
    pub required_source_ids: Vec<SourceId>,
    pub sensitivity: ActionSensitivity,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimulationContext {
    pub current_state_label: String,
    pub valid_at: ValidTime,
    pub known_at: TxTime,
    pub principal: Principal,
    pub causal_graph: CausalGraph,
    pub graph_state: GraphState,
    pub memory_service: AgentMemoryService,
    pub governance: GovernanceEngine,
    pub evidence_compiler: EvidencePackCompiler,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimulationReport {
    pub current_state: String,
    pub proposed_action: ProposedAction,
    pub affected_entities: Vec<EntityId>,
    pub outcomes: Vec<OutcomeHypothesis>,
    pub risk: RiskAssessment,
    pub missing_information: Vec<MissingInformation>,
    pub recommended_action: RecommendedAction,
    pub policy_checked: bool,
    pub policy_violations: Vec<PolicyViolation>,
    pub evidence_pack: EvidencePack,
    pub evidence_trace: RetrievalPlan,
    pub causal_paths: Vec<CausalPath>,
    pub impact_trace: Option<ImpactTrace>,
    pub prediction_not_fact: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutcomeHypothesis {
    pub description: String,
    pub confidence: f32,
    pub affected_entities: Vec<EntityId>,
    pub evidence_source_ids: Vec<SourceId>,
    pub prediction_not_fact: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RiskAssessment {
    pub score: f32,
    pub level: RiskLevel,
    pub factors: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MissingInformationKind {
    RequiredSource,
    EvidenceGap,
    PolicyContext,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingInformation {
    pub kind: MissingInformationKind,
    pub description: String,
    pub source_id: Option<SourceId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecommendedActionKind {
    ProceedWithCaution,
    GatherMissingInformation,
    ReviseAction,
    EscalateForReview,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecommendedAction {
    pub kind: RecommendedActionKind,
    pub description: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyViolation {
    pub resource_type: String,
    pub resource_id: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentSimulationLab {
    context: SimulationContext,
}

impl AgentSimulationLab {
    pub fn new(context: SimulationContext) -> Self {
        Self { context }
    }

    pub fn simulate(&self, action: ProposedAction) -> SimulationReport {
        let compiled = self.context.evidence_compiler.compile(CompilationRequest {
            question: format!(
                "What happens if agent {} performs action {}: {}",
                action.actor_agent_id, action.action_type, action.description
            ),
            agent_state: Some(AgentState {
                agent_id: Some(action.actor_agent_id.to_string()),
                user_id: None,
                active_entity_ids: action
                    .target_entities
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            }),
            temporal_constraints: Some((
                self.context.valid_at.as_i64(),
                self.context.known_at.as_i64(),
            )),
            budget: RetrievalBudget {
                max_evidence_items: 8,
                max_path_depth: 3,
                ..RetrievalBudget::default()
            },
            available_tools: RetrievalTool::all(),
            ..CompilationRequest::default()
        });

        let memory_hits = self.retrieve_memory(&action);
        let causal_paths = self.causal_paths(&action);
        let impact_trace = self.impact_trace(&action);
        let policy_checked = action.sensitivity == ActionSensitivity::High;
        let (evidence_pack, policy_violations) = if policy_checked {
            let governed = self.context.governance.enforce_evidence_pack(
                self.context.principal.clone(),
                &compiled.evidence_pack,
                AuditReason::AiContextPack,
            );
            let mut denials = governed.denials;
            for source_id in &action.required_source_ids {
                if let Some(denial) = self
                    .context
                    .governance
                    .check_source_access(&self.context.principal, source_id)
                {
                    denials.push(denial);
                }
            }
            (governed.pack, policy_violations(denials))
        } else {
            (compiled.evidence_pack.clone(), Vec::new())
        };

        let missing_information = missing_information(&action, &self.context.graph_state);
        let affected_entities = affected_entities(
            &action,
            &memory_hits,
            &causal_paths,
            impact_trace.as_ref(),
            &evidence_pack,
        );
        let outcomes = outcome_hypotheses(&action, &causal_paths, &memory_hits, &evidence_pack);
        let risk = assess_risk(
            action.sensitivity,
            &memory_hits,
            &causal_paths,
            &missing_information,
            &policy_violations,
        );
        let recommended_action = recommend(&risk, &missing_information, &policy_violations);

        SimulationReport {
            current_state: self.context.current_state_label.clone(),
            proposed_action: action,
            affected_entities,
            outcomes,
            risk,
            missing_information,
            recommended_action,
            policy_checked,
            policy_violations,
            evidence_pack,
            evidence_trace: compiled.plan,
            causal_paths,
            impact_trace,
            prediction_not_fact: true,
        }
    }

    fn retrieve_memory(&self, action: &ProposedAction) -> Vec<RetrievedMemory> {
        self.context
            .memory_service
            .retrieve_memory(MemoryQuery {
                agent_id: action.actor_agent_id.clone(),
                query: action.description.clone(),
                valid_at: Some(self.context.valid_at),
                related_entities: action.target_entities.clone(),
                include_history: false,
                mode: MemoryRetrievalMode::GraphTemporal,
                limit: Some(5),
            })
            .memories
    }

    fn causal_paths(&self, action: &ProposedAction) -> Vec<CausalPath> {
        let Some(event_id) = &action.related_event else {
            return Vec::new();
        };
        self.context.causal_graph.downstream_paths(CausalPathQuery {
            start: event_id.clone(),
            end: None,
            max_depth: 3,
            min_confidence: None,
        })
    }

    fn impact_trace(&self, action: &ProposedAction) -> Option<ImpactTrace> {
        let event_id = action.related_event.as_ref()?;
        Some(
            CounterfactualEngine::new(&self.context.causal_graph, &self.context.graph_state)
                .simulate(CounterfactualScenario {
                    intervention: Intervention::RemoveEvent(event_id.clone()),
                    valid_at: self.context.valid_at,
                    max_depth: 3,
                    assumptions: vec![format!(
                        "Testing action '{}' before execution; no graph event is appended.",
                        action.id
                    )],
                }),
        )
    }
}

fn missing_information(
    action: &ProposedAction,
    graph_state: &GraphState,
) -> Vec<MissingInformation> {
    let available_sources = graph_state.sources.keys().cloned().collect::<BTreeSet<_>>();
    let mut missing = action
        .required_source_ids
        .iter()
        .filter(|source_id| !available_sources.contains(*source_id))
        .map(|source_id| MissingInformation {
            kind: MissingInformationKind::RequiredSource,
            description: format!(
                "Required source {source_id} was not available in the current graph state."
            ),
            source_id: Some(source_id.clone()),
        })
        .collect::<Vec<_>>();
    missing.sort_by(|left, right| left.description.cmp(&right.description));
    missing
}

fn affected_entities(
    action: &ProposedAction,
    memory_hits: &[RetrievedMemory],
    causal_paths: &[CausalPath],
    impact_trace: Option<&ImpactTrace>,
    evidence_pack: &EvidencePack,
) -> Vec<EntityId> {
    let mut entities = action
        .target_entities
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for memory in memory_hits {
        entities.extend(memory.record.related_entities.iter().cloned());
    }
    for path in causal_paths {
        for event_id in path.event_ids() {
            let _ = event_id;
        }
    }
    if let Some(impact_trace) = impact_trace {
        entities.extend(impact_trace.affected_entities.iter().cloned());
    }
    for assertion in &evidence_pack.assertions {
        entities.insert(assertion.subject.clone());
        if let GraphValue::Entity(entity_id) = &assertion.object {
            entities.insert(entity_id.clone());
        }
    }
    entities.into_iter().collect()
}

fn outcome_hypotheses(
    action: &ProposedAction,
    causal_paths: &[CausalPath],
    memory_hits: &[RetrievedMemory],
    evidence_pack: &EvidencePack,
) -> Vec<OutcomeHypothesis> {
    let mut outcomes = causal_paths
        .iter()
        .map(|path| OutcomeHypothesis {
            description: format!(
                "Predicted outcome from causal path ending at {}: {}",
                path.end.to_string().replace('-', " "),
                path.explanation
            ),
            confidence: path.confidence.as_f32(),
            affected_entities: action.target_entities.clone(),
            evidence_source_ids: source_ids(evidence_pack),
            prediction_not_fact: true,
        })
        .collect::<Vec<_>>();

    if !memory_hits.is_empty() {
        outcomes.push(OutcomeHypothesis {
            description:
                "Predicted outcome: prior commitments may make the proposed action confusing or premature."
                    .to_owned(),
            confidence: 0.7,
            affected_entities: action.target_entities.clone(),
            evidence_source_ids: memory_hits
                .iter()
                .flat_map(|memory| memory.record.source_ids.iter().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            prediction_not_fact: true,
        });
    }

    if outcomes.is_empty() {
        outcomes.push(OutcomeHypothesis {
            description:
                "Predicted outcome: no specific causal or memory-backed downstream effect found."
                    .to_owned(),
            confidence: 0.4,
            affected_entities: action.target_entities.clone(),
            evidence_source_ids: source_ids(evidence_pack),
            prediction_not_fact: true,
        });
    }

    outcomes.sort_by(|left, right| {
        right
            .confidence
            .total_cmp(&left.confidence)
            .then_with(|| left.description.cmp(&right.description))
    });
    outcomes
}

fn assess_risk(
    sensitivity: ActionSensitivity,
    memory_hits: &[RetrievedMemory],
    causal_paths: &[CausalPath],
    missing_information: &[MissingInformation],
    policy_violations: &[PolicyViolation],
) -> RiskAssessment {
    let mut score: f32 = match sensitivity {
        ActionSensitivity::Low => 0.2,
        ActionSensitivity::Medium => 0.45,
        ActionSensitivity::High => 0.65,
    };
    let mut factors = vec![format!("action sensitivity is {:?}", sensitivity)];

    if !memory_hits.is_empty() {
        score += 0.15;
        factors.push("unresolved commitments found in agent memory".to_owned());
    }
    if !causal_paths.is_empty() {
        score += 0.1;
        factors.push("causal graph predicts downstream effects".to_owned());
    }
    if !missing_information.is_empty() {
        score += 0.15;
        factors.push("missing required context before acting".to_owned());
    }
    if !policy_violations.is_empty() {
        score += 0.25;
        factors.push("policy checks found violations".to_owned());
    }

    let score = round_two(score.min(1.0));
    RiskAssessment {
        score,
        level: risk_level(score),
        factors,
    }
}

fn recommend(
    risk: &RiskAssessment,
    missing_information: &[MissingInformation],
    policy_violations: &[PolicyViolation],
) -> RecommendedAction {
    if !policy_violations.is_empty() {
        return RecommendedAction {
            kind: RecommendedActionKind::EscalateForReview,
            description:
                "Escalate before acting because the simulation found policy or critical risk."
                    .to_owned(),
        };
    }
    if !missing_information.is_empty() {
        return RecommendedAction {
            kind: RecommendedActionKind::GatherMissingInformation,
            description: "Gather missing sources or context before executing the proposed action."
                .to_owned(),
        };
    }
    if risk.level == RiskLevel::Critical {
        return RecommendedAction {
            kind: RecommendedActionKind::EscalateForReview,
            description: "Escalate before acting because the simulation found critical risk."
                .to_owned(),
        };
    }
    if risk.level == RiskLevel::High {
        return RecommendedAction {
            kind: RecommendedActionKind::ReviseAction,
            description: "Revise the action to reduce predicted downstream risk.".to_owned(),
        };
    }
    RecommendedAction {
        kind: RecommendedActionKind::ProceedWithCaution,
        description: "Proceed only after reviewing the prediction-labeled evidence.".to_owned(),
    }
}

fn policy_violations(denials: Vec<AccessDenial>) -> Vec<PolicyViolation> {
    denials
        .into_iter()
        .map(|denial| PolicyViolation {
            resource_type: denial.resource_type,
            resource_id: denial.resource_id,
            description: denial.reason,
        })
        .collect()
}

fn source_ids(evidence_pack: &EvidencePack) -> Vec<SourceId> {
    evidence_pack
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn risk_level(score: f32) -> RiskLevel {
    if score >= 0.85 {
        RiskLevel::Critical
    } else if score >= 0.65 {
        RiskLevel::High
    } else if score >= 0.35 {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    }
}

fn round_two(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

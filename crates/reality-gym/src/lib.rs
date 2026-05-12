//! Reality Gym training environment for agents.

use std::collections::{BTreeMap, BTreeSet};

use rg_core::{AgentId, AssertionId, MemoryId, SourceId};
use rg_worldgen::{AgentTaskType, BenchmarkTask, SyntheticWorld};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ScenarioKind {
    ManageCompany,
    NegotiateContract,
    InvestigateFraud,
    CoordinateResearchProject,
    DebugLongRunningCodebase,
    RunCustomerSuccessAccount,
    TrackGeopoliticalCrisis,
    MaintainPersonalAssistantMemory,
}

impl ScenarioKind {
    pub fn all() -> Vec<Self> {
        vec![
            Self::ManageCompany,
            Self::NegotiateContract,
            Self::InvestigateFraud,
            Self::CoordinateResearchProject,
            Self::DebugLongRunningCodebase,
            Self::RunCustomerSuccessAccount,
            Self::TrackGeopoliticalCrisis,
            Self::MaintainPersonalAssistantMemory,
        ]
    }

    fn prompt_hint(self) -> &'static str {
        match self {
            Self::ManageCompany => "manage company operations",
            Self::NegotiateContract => "negotiate contract terms",
            Self::InvestigateFraud => "investigate fraud using noisy evidence",
            Self::CoordinateResearchProject => "coordinate research project work",
            Self::DebugLongRunningCodebase => "debug a long-running codebase",
            Self::RunCustomerSuccessAccount => "run a customer-success account",
            Self::TrackGeopoliticalCrisis => "track a geopolitical crisis",
            Self::MaintainPersonalAssistantMemory => "maintain personal assistant memory",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GymTaskKind {
    Observe,
    RetrieveMemory,
    Reason,
    Act,
    WriteMemory,
    WorldUpdates,
    EvaluateOutcome,
    HandleDelayedConsequence,
}

impl GymTaskKind {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Observe,
            Self::RetrieveMemory,
            Self::Reason,
            Self::Act,
            Self::WriteMemory,
            Self::WorldUpdates,
            Self::EvaluateOutcome,
            Self::HandleDelayedConsequence,
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentPolicy {
    pub requires_memory_retrieval: bool,
    pub requires_reasoning_trace: bool,
    pub requires_action: bool,
    pub requires_memory_write: bool,
}

impl AgentPolicy {
    pub fn memory_first() -> Self {
        Self {
            requires_memory_retrieval: true,
            requires_reasoning_trace: true,
            requires_action: true,
            requires_memory_write: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentConfig {
    pub world: SyntheticWorld,
    pub scenario: ScenarioKind,
    pub agents: Vec<AgentId>,
    pub max_steps: usize,
    pub adversarial_source_injection: bool,
    pub noisy_evidence: bool,
    pub delayed_consequences: bool,
}

impl EnvironmentConfig {
    pub fn single_agent(world: SyntheticWorld, scenario: ScenarioKind, agent_id: AgentId) -> Self {
        Self {
            world,
            scenario,
            agents: vec![agent_id],
            max_steps: 32,
            adversarial_source_injection: false,
            noisy_evidence: true,
            delayed_consequences: true,
        }
    }

    pub fn multi_agent(
        world: SyntheticWorld,
        scenario: ScenarioKind,
        agents: Vec<AgentId>,
    ) -> Self {
        Self {
            world,
            scenario,
            agents,
            max_steps: 64,
            adversarial_source_injection: false,
            noisy_evidence: true,
            delayed_consequences: true,
        }
    }

    pub fn with_adversarial_source_injection(mut self, enabled: bool) -> Self {
        self.adversarial_source_injection = enabled;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ObservationKind {
    NoisyEvidence,
    MemoryContext,
    WorldUpdate,
    Terminal,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Observation {
    pub step_index: usize,
    pub kind: ObservationKind,
    pub prompt: String,
    pub visible_assertion_ids: Vec<AssertionId>,
    pub visible_source_ids: Vec<SourceId>,
    pub hidden_truth_assertion_ids: Vec<AssertionId>,
    pub available_task_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ActionKind {
    RetrieveMemory,
    AnswerQuestion,
    PlanAction,
    VerifyClaim,
    InvestigateContradiction,
    NegotiateContract,
    WriteMemoryOnly,
    SimulateOutcome,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub agent_id: AgentId,
    pub kind: ActionKind,
    pub description: String,
    pub cited_assertion_ids: Vec<AssertionId>,
    pub cited_source_ids: Vec<SourceId>,
    pub memory_write: Option<MemoryWrite>,
}

impl Action {
    pub fn memory_only(agent_id: AgentId, memory_write: MemoryWrite) -> Self {
        Self {
            agent_id,
            kind: ActionKind::WriteMemoryOnly,
            description: "write memory".to_string(),
            cited_assertion_ids: memory_write.assertion_ids.clone(),
            cited_source_ids: memory_write.source_ids.clone(),
            memory_write: Some(memory_write),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryWrite {
    pub id: MemoryId,
    pub agent_id: AgentId,
    pub content: String,
    pub source_ids: Vec<SourceId>,
    pub assertion_ids: Vec<AssertionId>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WorldUpdateKind {
    MemoryCommitted,
    AgentActionApplied,
    AdversarialSourceInjected,
    NoisyEvidenceSurfaced,
    DelayedConsequenceScheduled,
    DelayedConsequenceApplied,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldUpdate {
    pub kind: WorldUpdateKind,
    pub description: String,
    pub related_assertion_ids: Vec<AssertionId>,
    pub related_source_ids: Vec<SourceId>,
    pub apply_after_steps: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RewardKind {
    CorrectAnswer,
    EvidenceCitation,
    MemoryWrite,
    ContradictionHandled,
    TrustedFalseEvidence,
    HiddenStateLeak,
    DelayedConsequence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RewardComponent {
    pub kind: RewardKind,
    pub value: f32,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RewardSignal {
    pub total: f32,
    pub components: Vec<RewardComponent>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentTransition {
    pub actor: AgentId,
    pub observation: Observation,
    pub action: Action,
    pub world_updates: Vec<WorldUpdate>,
    pub reward: RewardSignal,
    pub done: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentEnvironment {
    config: EnvironmentConfig,
    step_index: usize,
    memory_by_agent: BTreeMap<AgentId, Vec<MemoryWrite>>,
    pending_delayed: Vec<WorldUpdate>,
    history: Vec<EnvironmentTransition>,
    reset_seen: bool,
}

impl AgentEnvironment {
    pub fn new(config: EnvironmentConfig) -> Self {
        Self {
            config,
            step_index: 0,
            memory_by_agent: BTreeMap::new(),
            pending_delayed: Vec::new(),
            history: Vec::new(),
            reset_seen: false,
        }
    }

    pub fn reset(&mut self) -> Result<Observation, String> {
        if self.config.agents.is_empty() {
            return Err("reality gym requires at least one agent".to_string());
        }
        self.step_index = 0;
        self.memory_by_agent.clear();
        self.pending_delayed.clear();
        self.history.clear();
        self.reset_seen = true;
        Ok(self.noisy_observation(ObservationKind::NoisyEvidence))
    }

    pub fn observe(&self, agent_id: &AgentId) -> Result<Observation, String> {
        if !self.config.agents.contains(agent_id) {
            return Err(format!("unknown agent {agent_id}"));
        }
        Ok(self.noisy_observation(ObservationKind::MemoryContext))
    }

    pub fn step(&mut self, action: Action) -> Result<EnvironmentTransition, String> {
        if !self.reset_seen {
            return Err("environment must be reset before stepping".to_string());
        }
        if !self.config.agents.contains(&action.agent_id) {
            return Err(format!("unknown agent {}", action.agent_id));
        }

        self.step_index += 1;
        let mut updates = self.apply_due_delayed();
        if let Some(memory_write) = action.memory_write.clone() {
            self.memory_by_agent
                .entry(action.agent_id.clone())
                .or_default()
                .push(memory_write.clone());
            updates.push(WorldUpdate {
                kind: WorldUpdateKind::MemoryCommitted,
                description: format!("Committed memory {}", memory_write.id),
                related_assertion_ids: memory_write.assertion_ids,
                related_source_ids: memory_write.source_ids,
                apply_after_steps: 0,
            });
        }
        updates.push(WorldUpdate {
            kind: WorldUpdateKind::AgentActionApplied,
            description: action.description.clone(),
            related_assertion_ids: action.cited_assertion_ids.clone(),
            related_source_ids: action.cited_source_ids.clone(),
            apply_after_steps: 0,
        });
        if self.config.noisy_evidence {
            updates.push(WorldUpdate {
                kind: WorldUpdateKind::NoisyEvidenceSurfaced,
                description: "Noisy evidence remains visible to the agent.".to_string(),
                related_assertion_ids: self
                    .config
                    .world
                    .noisy_observed_state
                    .rumors
                    .iter()
                    .map(|assertion| assertion.id.clone())
                    .collect(),
                related_source_ids: Vec::new(),
                apply_after_steps: 0,
            });
        }
        if self.config.adversarial_source_injection {
            updates.push(WorldUpdate {
                kind: WorldUpdateKind::AdversarialSourceInjected,
                description: "Adversarial noisy source injected into observation stream."
                    .to_string(),
                related_assertion_ids: self
                    .config
                    .world
                    .noisy_observed_state
                    .contradictions
                    .iter()
                    .map(|pair| pair.observed_false_assertion.clone())
                    .collect(),
                related_source_ids: action.cited_source_ids.clone(),
                apply_after_steps: 0,
            });
        }
        if self.config.delayed_consequences
            && matches!(
                action.kind,
                ActionKind::PlanAction | ActionKind::AnswerQuestion | ActionKind::SimulateOutcome
            )
        {
            let delayed = WorldUpdate {
                kind: WorldUpdateKind::DelayedConsequenceScheduled,
                description: "Delayed consequence scheduled from world reaction.".to_string(),
                related_assertion_ids: action.cited_assertion_ids.clone(),
                related_source_ids: action.cited_source_ids.clone(),
                apply_after_steps: 2,
            };
            self.pending_delayed.push(delayed.clone());
            updates.push(delayed);
        }

        let reward = self.reward_for(&action, &updates);
        let observation = self.noisy_observation(ObservationKind::WorldUpdate);
        let transition = EnvironmentTransition {
            actor: action.agent_id.clone(),
            observation,
            action,
            world_updates: updates,
            reward,
            done: self.step_index >= self.config.max_steps,
        };
        self.history.push(transition.clone());
        Ok(transition)
    }

    pub fn current_step(&self) -> usize {
        self.step_index
    }

    pub fn agents(&self) -> &[AgentId] {
        &self.config.agents
    }

    pub fn memory_writes_for(&self, agent_id: &AgentId) -> &[MemoryWrite] {
        self.memory_by_agent
            .get(agent_id)
            .map_or(&[], Vec::as_slice)
    }

    pub fn pending_delayed_consequences(&self) -> &[WorldUpdate] {
        &self.pending_delayed
    }

    pub fn world(&self) -> &SyntheticWorld {
        &self.config.world
    }

    fn noisy_observation(&self, kind: ObservationKind) -> Observation {
        Observation {
            step_index: self.step_index,
            kind,
            prompt: format!(
                "Scenario: {}. Agent must observe, retrieve memory, reason, act, write memory, and evaluate outcome.",
                self.config.scenario.prompt_hint()
            ),
            visible_assertion_ids: self
                .config
                .world
                .noisy_observed_state
                .assertions
                .iter()
                .map(|assertion| assertion.id.clone())
                .collect(),
            visible_source_ids: self
                .config
                .world
                .noisy_observed_state
                .source_documents
                .iter()
                .map(|document| document.id.clone())
                .collect(),
            hidden_truth_assertion_ids: Vec::new(),
            available_task_ids: self
                .config
                .world
                .benchmark_tasks
                .iter()
                .map(|task| task.id.clone())
                .collect(),
        }
    }

    fn apply_due_delayed(&mut self) -> Vec<WorldUpdate> {
        let mut due = Vec::new();
        for update in &mut self.pending_delayed {
            if update.apply_after_steps > 0 {
                update.apply_after_steps -= 1;
            }
        }
        let mut remaining = Vec::new();
        for update in self.pending_delayed.drain(..) {
            if update.apply_after_steps == 0 {
                due.push(WorldUpdate {
                    kind: WorldUpdateKind::DelayedConsequenceApplied,
                    description: update.description,
                    related_assertion_ids: update.related_assertion_ids,
                    related_source_ids: update.related_source_ids,
                    apply_after_steps: 0,
                });
            } else {
                remaining.push(update);
            }
        }
        self.pending_delayed = remaining;
        due
    }

    fn reward_for(&self, action: &Action, updates: &[WorldUpdate]) -> RewardSignal {
        let true_assertions = self.true_assertions();
        let false_assertions = self.false_observed_assertions();
        let mut components = Vec::new();

        if action
            .cited_assertion_ids
            .iter()
            .any(|assertion_id| true_assertions.contains(assertion_id))
        {
            components.push(RewardComponent {
                kind: RewardKind::CorrectAnswer,
                value: 1.0,
                explanation: "Action cited hidden-truth-backed evidence.".to_string(),
            });
        }
        if !action.cited_source_ids.is_empty() {
            components.push(RewardComponent {
                kind: RewardKind::EvidenceCitation,
                value: 0.25,
                explanation: "Action cited source evidence.".to_string(),
            });
        }
        if action.memory_write.is_some() {
            components.push(RewardComponent {
                kind: RewardKind::MemoryWrite,
                value: 0.35,
                explanation: "Agent wrote memory after acting.".to_string(),
            });
        }
        if action
            .cited_assertion_ids
            .iter()
            .any(|assertion_id| false_assertions.contains(assertion_id))
        {
            components.push(RewardComponent {
                kind: RewardKind::TrustedFalseEvidence,
                value: -1.2,
                explanation: "Action trusted a noisy contradiction.".to_string(),
            });
        }
        if updates
            .iter()
            .any(|update| update.kind == WorldUpdateKind::DelayedConsequenceScheduled)
        {
            components.push(RewardComponent {
                kind: RewardKind::DelayedConsequence,
                value: 0.05,
                explanation: "Action produced a delayed consequence.".to_string(),
            });
        }
        if matches!(action.kind, ActionKind::InvestigateContradiction)
            && !self
                .config
                .world
                .noisy_observed_state
                .contradictions
                .is_empty()
        {
            components.push(RewardComponent {
                kind: RewardKind::ContradictionHandled,
                value: 0.5,
                explanation: "Agent investigated contradiction instead of collapsing it."
                    .to_string(),
            });
        }

        let total = components.iter().map(|component| component.value).sum();
        RewardSignal { total, components }
    }

    fn true_assertions(&self) -> BTreeSet<AssertionId> {
        self.config
            .world
            .hidden_true_state
            .assertions
            .iter()
            .map(|assertion| assertion.id.clone())
            .collect()
    }

    fn false_observed_assertions(&self) -> BTreeSet<AssertionId> {
        self.config
            .world
            .noisy_observed_state
            .contradictions
            .iter()
            .map(|pair| pair.observed_false_assertion.clone())
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvaluationOracle {
    answers_by_task: BTreeMap<String, String>,
    hidden_truth_assertions: BTreeSet<AssertionId>,
    pending_delayed: Vec<WorldUpdate>,
}

impl EvaluationOracle {
    pub fn from_environment(environment: &AgentEnvironment) -> Self {
        Self {
            answers_by_task: environment
                .config
                .world
                .benchmark_tasks
                .iter()
                .map(|task| (task.id.clone(), task.expected_answer.clone()))
                .collect(),
            hidden_truth_assertions: environment
                .config
                .world
                .hidden_true_state
                .assertions
                .iter()
                .map(|assertion| assertion.id.clone())
                .collect(),
            pending_delayed: environment.pending_delayed.clone(),
        }
    }

    pub fn answer_task(&self, task: &BenchmarkTask) -> Option<&str> {
        self.answers_by_task.get(&task.id).map(String::as_str)
    }

    pub fn hidden_truth_contains(&self, assertion_id: &AssertionId) -> bool {
        self.hidden_truth_assertions.contains(assertion_id)
    }

    pub fn pending_delayed_consequences(&self) -> &[WorldUpdate] {
        &self.pending_delayed
    }
}

impl From<AgentTaskType> for GymTaskKind {
    fn from(task_type: AgentTaskType) -> Self {
        match task_type {
            AgentTaskType::AnswerQuestions => Self::Act,
            AgentTaskType::PlanActions => Self::Act,
            AgentTaskType::DetectContradictions => Self::Reason,
            AgentTaskType::UpdateBeliefs => Self::EvaluateOutcome,
            AgentTaskType::RememberPreferences => Self::WriteMemory,
            AgentTaskType::SimulateOutcomes => Self::HandleDelayedConsequence,
            AgentTaskType::RecoverTimelines => Self::Reason,
            AgentTaskType::VerifyClaims => Self::EvaluateOutcome,
        }
    }
}

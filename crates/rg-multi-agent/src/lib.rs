//! Multi-agent shared reality primitives for Reality Graph.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rg_belief::{Claim, ClaimId};
pub use rg_core::AgentId;
use rg_core::MemoryId;

macro_rules! string_newtype {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

string_newtype!(TeamId);
string_newtype!(EvidenceExchangeId);
string_newtype!(MultiAgentAuditEventId);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BeliefNamespace {
    AgentPrivate(AgentId),
    Team(TeamId),
    Organization(String),
    PublicWorldState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemorySharePolicy {
    Private,
    DirectAgents(BTreeSet<AgentId>),
    Team(TeamId),
    Organization(String),
    Public,
}

impl MemorySharePolicy {
    pub fn private() -> Self {
        Self::Private
    }

    pub fn direct(agent_ids: Vec<AgentId>) -> Self {
        Self::DirectAgents(agent_ids.into_iter().collect())
    }

    pub fn team(team_id: TeamId) -> Self {
        Self::Team(team_id)
    }

    pub fn organization(organization_id: impl Into<String>) -> Self {
        Self::Organization(organization_id.into())
    }

    pub fn public() -> Self {
        Self::Public
    }

    fn allows_exchange(
        &self,
        from_agent_id: &AgentId,
        to_agent_id: &AgentId,
        teams: &BTreeMap<TeamId, BTreeSet<AgentId>>,
    ) -> bool {
        if from_agent_id == to_agent_id {
            return true;
        }

        match self {
            Self::Private => false,
            Self::DirectAgents(agent_ids) => agent_ids.contains(to_agent_id),
            Self::Team(team_id) => teams.get(team_id).is_some_and(|members| {
                members.contains(from_agent_id) && members.contains(to_agent_id)
            }),
            Self::Organization(_) | Self::Public => true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryProvenance {
    pub source_ids: Vec<rg_core::SourceId>,
    pub shared_from: Vec<AgentId>,
    pub exchange_ids: Vec<EvidenceExchangeId>,
}

impl MemoryProvenance {
    fn from_claim(claim: &Claim) -> Self {
        Self {
            source_ids: claim.source_ids.clone(),
            shared_from: Vec::new(),
            exchange_ids: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BeliefRecord {
    pub memory_id: MemoryId,
    pub claim: Claim,
    pub namespace: BeliefNamespace,
    pub created_by: AgentId,
    pub share_policy: MemorySharePolicy,
    pub shared_with: BTreeSet<AgentId>,
    pub provenance: MemoryProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivateMemorySpace {
    pub owner_agent_id: AgentId,
    pub memory_ids: Vec<MemoryId>,
    pub claim_ids: Vec<ClaimId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedMemorySpace {
    pub namespace: BeliefNamespace,
    pub members: Vec<AgentId>,
    pub memory_ids: Vec<MemoryId>,
    pub claim_ids: Vec<ClaimId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentBeliefState {
    pub agent_id: AgentId,
    pub private_memory: PrivateMemorySpace,
    pub shared_spaces: Vec<SharedMemorySpace>,
    pub beliefs: Vec<BeliefRecord>,
    pub conflicts: Vec<MultiAgentConflict>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TeamBeliefState {
    pub team_id: TeamId,
    pub team_memory: SharedMemorySpace,
    pub beliefs: Vec<BeliefRecord>,
    pub conflicts: Vec<MultiAgentConflict>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlobalBeliefState {
    pub beliefs: Vec<BeliefRecord>,
    pub conflicts: Vec<MultiAgentConflict>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KnowledgeDelta {
    pub left_agent_id: AgentId,
    pub right_agent_id: AgentId,
    pub only_known_by_left: Vec<BeliefRecord>,
    pub only_known_by_right: Vec<BeliefRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskContext {
    pub task_id: String,
    pub target_agent_id: AgentId,
    pub objective: String,
    pub candidate_source_agents: Vec<AgentId>,
}

impl TaskContext {
    pub fn new(
        task_id: impl Into<String>,
        target_agent_id: AgentId,
        objective: impl Into<String>,
        candidate_source_agents: Vec<AgentId>,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            target_agent_id,
            objective: objective.into(),
            candidate_source_agents,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShareRecommendation {
    pub task_id: String,
    pub claim_id: ClaimId,
    pub memory_id: MemoryId,
    pub share_from: AgentId,
    pub share_to: AgentId,
    pub reason: String,
    pub score: f32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterAgentEvidenceExchange {
    pub id: EvidenceExchangeId,
    pub from_agent: AgentId,
    pub to_agent: AgentId,
    pub claim_ids: Vec<ClaimId>,
    pub reason: String,
}

impl InterAgentEvidenceExchange {
    pub fn new(
        id: impl Into<String>,
        from_agent: AgentId,
        to_agent: AgentId,
        claim_ids: Vec<ClaimId>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            id: EvidenceExchangeId::new(id),
            from_agent,
            to_agent,
            claim_ids,
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceExchangeReceipt {
    pub exchange_id: EvidenceExchangeId,
    pub permitted: bool,
    pub shared_claim_ids: Vec<ClaimId>,
    pub audit_event_id: MultiAgentAuditEventId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MultiAgentConflict {
    pub id: String,
    pub left: BeliefRecord,
    pub right: BeliefRecord,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedMultiAgentConflict {
    pub conflict_id: String,
    pub preferred_claim_id: Option<ClaimId>,
    pub preserved_claim_ids: Vec<ClaimId>,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MultiAgentConflictResolver {
    pub confidence_weight: f32,
}

impl Default for MultiAgentConflictResolver {
    fn default() -> Self {
        Self {
            confidence_weight: 1.0,
        }
    }
}

impl MultiAgentConflictResolver {
    pub fn resolve(&self, conflict: &MultiAgentConflict) -> ResolvedMultiAgentConflict {
        let preferred = preferred_record(&conflict.left, &conflict.right);
        let preserved_claim_ids = vec![
            conflict.left.claim.id.clone(),
            conflict.right.claim.id.clone(),
        ];
        ResolvedMultiAgentConflict {
            conflict_id: conflict.id.clone(),
            preferred_claim_id: preferred.map(|record| record.claim.id.clone()),
            preserved_claim_ids,
            explanation: format!(
                "{} is preferred by confidence and recency; all claims are preserved as competing beliefs",
                preferred
                    .map(|record| record.claim.id.to_string())
                    .unwrap_or_else(|| "no claim".to_owned())
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultiAgentAuditAction {
    MemoryRecorded,
    EvidenceExchanged,
    QueryRan,
    ShareDenied,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultiAgentAuditEvent {
    pub id: MultiAgentAuditEventId,
    pub action: MultiAgentAuditAction,
    pub actor_agent_id: AgentId,
    pub target_agent_id: Option<AgentId>,
    pub team_id: Option<TeamId>,
    pub claim_ids: Vec<ClaimId>,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultiAgentError {
    UnknownAgent(AgentId),
    UnknownTeam(TeamId),
    UnknownClaim(ClaimId),
    ShareDenied {
        claim_id: ClaimId,
        from_agent: AgentId,
        to_agent: AgentId,
    },
}

impl fmt::Display for MultiAgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAgent(agent_id) => write!(formatter, "unknown agent {agent_id}"),
            Self::UnknownTeam(team_id) => write!(formatter, "unknown team {team_id}"),
            Self::UnknownClaim(claim_id) => write!(formatter, "unknown claim {claim_id}"),
            Self::ShareDenied {
                claim_id,
                from_agent,
                to_agent,
            } => write!(
                formatter,
                "agent {from_agent} cannot share claim {claim_id} with {to_agent}"
            ),
        }
    }
}

impl std::error::Error for MultiAgentError {}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MultiAgentReality {
    agents: BTreeSet<AgentId>,
    teams: BTreeMap<TeamId, BTreeSet<AgentId>>,
    records: BTreeMap<ClaimId, BeliefRecord>,
    audit_log: Vec<MultiAgentAuditEvent>,
    next_audit_index: u64,
}

impl MultiAgentReality {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_agent(&mut self, agent_id: AgentId) {
        self.agents.insert(agent_id);
    }

    pub fn register_team(&mut self, team_id: TeamId, agent_ids: Vec<AgentId>) {
        let members = agent_ids.into_iter().collect::<BTreeSet<_>>();
        for agent_id in &members {
            self.register_agent(agent_id.clone());
        }
        self.teams.insert(team_id, members);
    }

    pub fn record_private_memory(
        &mut self,
        agent_id: AgentId,
        memory_id: MemoryId,
        claim: Claim,
        share_policy: MemorySharePolicy,
    ) -> Result<BeliefRecord, MultiAgentError> {
        self.register_agent(agent_id.clone());
        self.insert_record(
            agent_id.clone(),
            memory_id,
            claim,
            BeliefNamespace::AgentPrivate(agent_id),
            share_policy,
            None,
        )
    }

    pub fn record_team_memory(
        &mut self,
        team_id: TeamId,
        created_by: AgentId,
        memory_id: MemoryId,
        claim: Claim,
        share_policy: MemorySharePolicy,
    ) -> Result<BeliefRecord, MultiAgentError> {
        let members = self
            .teams
            .get(&team_id)
            .ok_or_else(|| MultiAgentError::UnknownTeam(team_id.clone()))?;
        if !members.contains(&created_by) {
            return Err(MultiAgentError::UnknownAgent(created_by));
        }
        self.insert_record(
            created_by,
            memory_id,
            claim,
            BeliefNamespace::Team(team_id.clone()),
            share_policy,
            Some(team_id),
        )
    }

    pub fn record_organization_memory(
        &mut self,
        organization_id: impl Into<String>,
        created_by: AgentId,
        memory_id: MemoryId,
        claim: Claim,
        share_policy: MemorySharePolicy,
    ) -> Result<BeliefRecord, MultiAgentError> {
        self.register_agent(created_by.clone());
        self.insert_record(
            created_by,
            memory_id,
            claim,
            BeliefNamespace::Organization(organization_id.into()),
            share_policy,
            None,
        )
    }

    pub fn record_public_world_state(
        &mut self,
        created_by: AgentId,
        memory_id: MemoryId,
        claim: Claim,
        share_policy: MemorySharePolicy,
    ) -> Result<BeliefRecord, MultiAgentError> {
        self.register_agent(created_by.clone());
        self.insert_record(
            created_by,
            memory_id,
            claim,
            BeliefNamespace::PublicWorldState,
            share_policy,
            None,
        )
    }

    pub fn agent_beliefs(&self, agent_id: &AgentId) -> Result<AgentBeliefState, MultiAgentError> {
        self.ensure_agent(agent_id)?;
        let beliefs = self
            .records
            .values()
            .filter(|record| self.agent_can_read_record(agent_id, record))
            .cloned()
            .collect::<Vec<_>>();
        let private_records = beliefs
            .iter()
            .filter(|record| record.namespace == BeliefNamespace::AgentPrivate(agent_id.clone()))
            .cloned()
            .collect::<Vec<_>>();
        let private_memory = PrivateMemorySpace {
            owner_agent_id: agent_id.clone(),
            memory_ids: memory_ids(&private_records),
            claim_ids: claim_ids(&private_records),
        };
        let shared_spaces = self.shared_spaces_for_agent(agent_id, &beliefs);
        let conflicts = conflicts_for_records(&beliefs);

        Ok(AgentBeliefState {
            agent_id: agent_id.clone(),
            private_memory,
            shared_spaces,
            beliefs,
            conflicts,
        })
    }

    pub fn team_beliefs(&self, team_id: &TeamId) -> Result<TeamBeliefState, MultiAgentError> {
        let members = self
            .teams
            .get(team_id)
            .ok_or_else(|| MultiAgentError::UnknownTeam(team_id.clone()))?;
        let beliefs = self
            .records
            .values()
            .filter(|record| {
                record.namespace == BeliefNamespace::Team(team_id.clone())
                    || record.namespace == BeliefNamespace::PublicWorldState
            })
            .cloned()
            .collect::<Vec<_>>();
        let team_records = beliefs
            .iter()
            .filter(|record| record.namespace == BeliefNamespace::Team(team_id.clone()))
            .cloned()
            .collect::<Vec<_>>();
        let team_memory = SharedMemorySpace {
            namespace: BeliefNamespace::Team(team_id.clone()),
            members: members.iter().cloned().collect(),
            memory_ids: memory_ids(&team_records),
            claim_ids: claim_ids(&team_records),
        };
        let conflicts = conflicts_for_records(&beliefs);

        Ok(TeamBeliefState {
            team_id: team_id.clone(),
            team_memory,
            beliefs,
            conflicts,
        })
    }

    pub fn globally_accepted(&self) -> GlobalBeliefState {
        let beliefs = self
            .records
            .values()
            .filter(|record| record.namespace == BeliefNamespace::PublicWorldState)
            .cloned()
            .collect::<Vec<_>>();
        let conflicts = conflicts_for_records(&beliefs);
        GlobalBeliefState { beliefs, conflicts }
    }

    pub fn knowledge_delta(
        &self,
        left_agent_id: &AgentId,
        right_agent_id: &AgentId,
    ) -> Result<KnowledgeDelta, MultiAgentError> {
        let left = self.agent_beliefs(left_agent_id)?.beliefs;
        let right = self.agent_beliefs(right_agent_id)?.beliefs;
        let left_ids = claim_ids(&left).into_iter().collect::<BTreeSet<_>>();
        let right_ids = claim_ids(&right).into_iter().collect::<BTreeSet<_>>();

        Ok(KnowledgeDelta {
            left_agent_id: left_agent_id.clone(),
            right_agent_id: right_agent_id.clone(),
            only_known_by_left: left
                .into_iter()
                .filter(|record| !right_ids.contains(&record.claim.id))
                .collect(),
            only_known_by_right: right
                .into_iter()
                .filter(|record| !left_ids.contains(&record.claim.id))
                .collect(),
        })
    }

    pub fn recommend_sharing_for_task(
        &self,
        task: &TaskContext,
    ) -> Result<Vec<ShareRecommendation>, MultiAgentError> {
        self.ensure_agent(&task.target_agent_id)?;
        let target_known = self
            .agent_beliefs(&task.target_agent_id)?
            .beliefs
            .into_iter()
            .map(|record| record.claim.id)
            .collect::<BTreeSet<_>>();
        let mut recommendations = Vec::new();

        for source_agent_id in &task.candidate_source_agents {
            self.ensure_agent(source_agent_id)?;
            for record in self.records.values() {
                if record.created_by != *source_agent_id
                    || target_known.contains(&record.claim.id)
                    || !record.share_policy.allows_exchange(
                        source_agent_id,
                        &task.target_agent_id,
                        &self.teams,
                    )
                {
                    continue;
                }

                let score = relevance_score(&task.objective, record);
                if score > 0.0 {
                    recommendations.push(ShareRecommendation {
                        task_id: task.task_id.clone(),
                        claim_id: record.claim.id.clone(),
                        memory_id: record.memory_id.clone(),
                        share_from: source_agent_id.clone(),
                        share_to: task.target_agent_id.clone(),
                        reason: format!(
                            "source-backed memory from {} matches task objective with provenance {}",
                            source_agent_id,
                            source_list(record)
                        ),
                        score,
                    });
                }
            }
        }

        recommendations.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.claim_id.cmp(&right.claim_id))
        });
        Ok(recommendations)
    }

    pub fn exchange_evidence(
        &mut self,
        exchange: InterAgentEvidenceExchange,
    ) -> Result<EvidenceExchangeReceipt, MultiAgentError> {
        self.ensure_agent(&exchange.from_agent)?;
        self.ensure_agent(&exchange.to_agent)?;

        for claim_id in &exchange.claim_ids {
            let record = self
                .records
                .get(claim_id)
                .ok_or_else(|| MultiAgentError::UnknownClaim(claim_id.clone()))?;
            if record.created_by != exchange.from_agent
                || !record.share_policy.allows_exchange(
                    &exchange.from_agent,
                    &exchange.to_agent,
                    &self.teams,
                )
            {
                let audit_event_id = self.append_audit(
                    MultiAgentAuditAction::ShareDenied,
                    exchange.from_agent.clone(),
                    Some(exchange.to_agent.clone()),
                    None,
                    vec![claim_id.clone()],
                    exchange.reason.clone(),
                );
                let _ = audit_event_id;
                return Err(MultiAgentError::ShareDenied {
                    claim_id: claim_id.clone(),
                    from_agent: exchange.from_agent,
                    to_agent: exchange.to_agent,
                });
            }
        }

        for claim_id in &exchange.claim_ids {
            if let Some(record) = self.records.get_mut(claim_id) {
                record.shared_with.insert(exchange.to_agent.clone());
                record
                    .provenance
                    .shared_from
                    .push(exchange.from_agent.clone());
                record.provenance.exchange_ids.push(exchange.id.clone());
            }
        }

        let audit_event_id = self.append_audit(
            MultiAgentAuditAction::EvidenceExchanged,
            exchange.from_agent.clone(),
            Some(exchange.to_agent.clone()),
            None,
            exchange.claim_ids.clone(),
            exchange.reason,
        );

        Ok(EvidenceExchangeReceipt {
            exchange_id: exchange.id,
            permitted: true,
            shared_claim_ids: exchange.claim_ids,
            audit_event_id,
        })
    }

    pub fn conflicts_for_agents(&self, agent_ids: &[AgentId]) -> Vec<MultiAgentConflict> {
        let mut records_by_id = BTreeMap::<ClaimId, BeliefRecord>::new();
        for agent_id in agent_ids {
            if let Ok(state) = self.agent_beliefs(agent_id) {
                for record in state.beliefs {
                    records_by_id.insert(record.claim.id.clone(), record);
                }
            }
        }
        let records = records_by_id.into_values().collect::<Vec<_>>();
        conflicts_for_records(&records)
    }

    pub fn audit_log(&self) -> &[MultiAgentAuditEvent] {
        &self.audit_log
    }

    fn insert_record(
        &mut self,
        created_by: AgentId,
        memory_id: MemoryId,
        claim: Claim,
        namespace: BeliefNamespace,
        share_policy: MemorySharePolicy,
        team_id: Option<TeamId>,
    ) -> Result<BeliefRecord, MultiAgentError> {
        let record = BeliefRecord {
            memory_id,
            provenance: MemoryProvenance::from_claim(&claim),
            claim,
            namespace,
            created_by: created_by.clone(),
            share_policy,
            shared_with: BTreeSet::new(),
        };
        self.records.insert(record.claim.id.clone(), record.clone());
        self.append_audit(
            MultiAgentAuditAction::MemoryRecorded,
            created_by,
            None,
            team_id,
            vec![record.claim.id.clone()],
            "memory recorded with provenance".to_owned(),
        );
        Ok(record)
    }

    fn ensure_agent(&self, agent_id: &AgentId) -> Result<(), MultiAgentError> {
        if self.agents.contains(agent_id) {
            Ok(())
        } else {
            Err(MultiAgentError::UnknownAgent(agent_id.clone()))
        }
    }

    fn agent_can_read_record(&self, agent_id: &AgentId, record: &BeliefRecord) -> bool {
        match &record.namespace {
            BeliefNamespace::AgentPrivate(owner) => {
                owner == agent_id || record.shared_with.contains(agent_id)
            }
            BeliefNamespace::Team(team_id) => self
                .teams
                .get(team_id)
                .is_some_and(|members| members.contains(agent_id)),
            BeliefNamespace::Organization(_) | BeliefNamespace::PublicWorldState => true,
        }
    }

    fn shared_spaces_for_agent(
        &self,
        agent_id: &AgentId,
        beliefs: &[BeliefRecord],
    ) -> Vec<SharedMemorySpace> {
        let mut by_namespace = BTreeMap::<BeliefNamespace, Vec<BeliefRecord>>::new();
        for record in beliefs {
            if !matches!(record.namespace, BeliefNamespace::AgentPrivate(_)) {
                by_namespace
                    .entry(record.namespace.clone())
                    .or_default()
                    .push(record.clone());
            }
        }

        by_namespace
            .into_iter()
            .map(|(namespace, records)| {
                let members = match &namespace {
                    BeliefNamespace::Team(team_id) => self
                        .teams
                        .get(team_id)
                        .map(|members| members.iter().cloned().collect())
                        .unwrap_or_default(),
                    BeliefNamespace::Organization(_) | BeliefNamespace::PublicWorldState => {
                        self.agents.iter().cloned().collect()
                    }
                    BeliefNamespace::AgentPrivate(_) => vec![agent_id.clone()],
                };
                SharedMemorySpace {
                    namespace,
                    members,
                    memory_ids: memory_ids(&records),
                    claim_ids: claim_ids(&records),
                }
            })
            .collect()
    }

    fn append_audit(
        &mut self,
        action: MultiAgentAuditAction,
        actor_agent_id: AgentId,
        target_agent_id: Option<AgentId>,
        team_id: Option<TeamId>,
        claim_ids: Vec<ClaimId>,
        reason: String,
    ) -> MultiAgentAuditEventId {
        self.next_audit_index += 1;
        let id =
            MultiAgentAuditEventId::new(format!("multi-agent-audit-{}", self.next_audit_index));
        self.audit_log.push(MultiAgentAuditEvent {
            id: id.clone(),
            action,
            actor_agent_id,
            target_agent_id,
            team_id,
            claim_ids,
            reason,
        });
        id
    }
}

fn conflicts_for_records(records: &[BeliefRecord]) -> Vec<MultiAgentConflict> {
    let mut conflicts = Vec::new();
    for left_index in 0..records.len() {
        let left = &records[left_index];
        for right in records.iter().skip(left_index + 1) {
            if claims_conflict(left, right) {
                let (left, right) = ordered_pair(left, right);
                conflicts.push(MultiAgentConflict {
                    id: format!("multi-agent-conflict-{}-{}", left.claim.id, right.claim.id),
                    left: left.clone(),
                    right: right.clone(),
                    explanation: format!(
                        "{} and {} disagree about {} for {} across namespaces",
                        left.claim.id, right.claim.id, left.claim.predicate, left.claim.subject
                    ),
                });
            }
        }
    }
    conflicts.sort_by(|left, right| left.id.cmp(&right.id));
    conflicts.dedup_by(|left, right| left.id == right.id);
    conflicts
}

fn claims_conflict(left: &BeliefRecord, right: &BeliefRecord) -> bool {
    left.claim.id != right.claim.id
        && left.claim.subject == right.claim.subject
        && left.claim.predicate == right.claim.predicate
        && left.claim.object != right.claim.object
        && left.claim.valid_time.overlaps(&right.claim.valid_time)
}

fn ordered_pair<'a>(
    left: &'a BeliefRecord,
    right: &'a BeliefRecord,
) -> (&'a BeliefRecord, &'a BeliefRecord) {
    if left.claim.id <= right.claim.id {
        (left, right)
    } else {
        (right, left)
    }
}

fn preferred_record<'a>(
    left: &'a BeliefRecord,
    right: &'a BeliefRecord,
) -> Option<&'a BeliefRecord> {
    [left, right].into_iter().max_by(|left, right| {
        left.claim
            .confidence
            .as_f32()
            .total_cmp(&right.claim.confidence.as_f32())
            .then_with(|| {
                left.claim
                    .transaction_time
                    .cmp(&right.claim.transaction_time)
            })
            .then_with(|| right.claim.id.cmp(&left.claim.id))
    })
}

fn memory_ids(records: &[BeliefRecord]) -> Vec<MemoryId> {
    records
        .iter()
        .map(|record| record.memory_id.clone())
        .collect()
}

fn claim_ids(records: &[BeliefRecord]) -> Vec<ClaimId> {
    records
        .iter()
        .map(|record| record.claim.id.clone())
        .collect()
}

fn relevance_score(objective: &str, record: &BeliefRecord) -> f32 {
    let objective_tokens = tokens(objective);
    if objective_tokens.is_empty() {
        return 0.0;
    }
    let haystack = format!(
        "{} {} {:?} {}",
        record.claim.subject, record.claim.predicate, record.claim.object, record.claim.evidence
    );
    let haystack_tokens = tokens(&haystack);
    let overlap = objective_tokens
        .iter()
        .filter(|token| haystack_tokens.contains(*token))
        .count();
    overlap as f32 / objective_tokens.len() as f32
}

fn tokens(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn source_list(record: &BeliefRecord) -> String {
    if record.provenance.source_ids.is_empty() {
        return "no sources".to_owned();
    }
    record
        .provenance
        .source_ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

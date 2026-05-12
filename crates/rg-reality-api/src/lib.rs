//! AI-native Reality API product layer.

use std::collections::BTreeSet;
use std::fmt;

use rg_agent_memory::{
    AgentMemoryError, AgentMemoryKind, AgentMemoryService, MemoryPermissions, MemoryQuery,
    MemoryRecord, MemoryRetrievalMode, WriteMemory,
};
use rg_agent_sim::{AgentSimulationLab, ProposedAction, SimulationContext, SimulationReport};
use rg_ai::EvidencePack;
use rg_causal::CausalGraph;
use rg_core::{
    AgentId, Assertion, AssertionId, Confidence, EntityId, GraphValue, MemoryId, PredicateId,
    SourceId, TimeInterval, TxTime, ValidTime,
};
use rg_governance::{GovernanceEngine, Principal};
use rg_index::{Contradiction, TemporalIndex};
use rg_query::QueryResult;
use rg_retrieval_compiler::{
    AgentState, CompilationRequest, CompiledEvidencePack, EvidencePackCompiler, RetrievalBudget,
    RetrievalPlan, RetrievalTool, RetrievalTrace,
};
use rg_storage::InMemoryStorage;

#[derive(Clone, Debug, PartialEq)]
pub struct RealityApiContext {
    pub storage: InMemoryStorage,
    pub memory_service: AgentMemoryService,
    pub causal_graph: CausalGraph,
    pub governance: GovernanceEngine,
    pub principal: Principal,
    pub valid_at: ValidTime,
    pub known_at: TxTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RealityApi {
    storage: InMemoryStorage,
    memory_service: AgentMemoryService,
    causal_graph: CausalGraph,
    governance: GovernanceEngine,
    principal: Principal,
    valid_at: ValidTime,
    known_at: TxTime,
}

impl RealityApi {
    pub fn new(context: RealityApiContext) -> Self {
        Self {
            storage: context.storage,
            memory_service: context.memory_service,
            causal_graph: context.causal_graph,
            governance: context.governance,
            principal: context.principal,
            valid_at: context.valid_at,
            known_at: context.known_at,
        }
    }

    pub fn endpoint_paths() -> [&'static str; 9] {
        [
            "/remember",
            "/recall",
            "/verify",
            "/explain",
            "/timeline",
            "/simulate",
            "/context",
            "/contradictions",
            "/state",
        ]
    }

    pub fn recommended_product_api() -> Vec<RecommendedProductApi> {
        vec![
            RecommendedProductApi::Memory,
            RecommendedProductApi::Verification,
            RecommendedProductApi::Explanation,
            RecommendedProductApi::Simulation,
            RecommendedProductApi::Context,
        ]
    }

    pub fn remember(
        &mut self,
        request: RememberRequest,
    ) -> Result<RealityResponse<RememberData>, RealityApiError> {
        if request.source_ids.is_empty() {
            return Err(RealityApiError::MissingSourceEvidence);
        }
        let compiled = self.compile(
            &format!("Remember: {}", request.content),
            Some(request.agent_id.clone()),
            request.related_entities.clone(),
            None,
            Some(6),
        );
        let memory = self.memory_service.write_memory(WriteMemory {
            id: request.memory_id,
            agent_id: request.agent_id,
            memory_type: request.memory_type,
            content: request.content,
            valid_time: request.valid_time,
            confidence: request.confidence,
            source_ids: request.source_ids,
            related_entities: request.related_entities,
            supersedes: Vec::new(),
            contradicts: Vec::new(),
            lifecycle: rg_core::MemoryStatus::Active,
            permissions: MemoryPermissions::private(agent_from_principal(&self.principal)),
        })?;
        let extra_sources = memory.source_ids.clone();
        Ok(response(
            compiled,
            RememberData {
                memory_id: memory.id,
                stored: true,
                source_ids: extra_sources.clone(),
            },
            &extra_sources,
            vec!["Memory is stored as source-backed agent memory.".to_owned()],
        ))
    }

    pub fn recall(&self, request: RecallRequest) -> RealityResponse<RecallData> {
        let compiled = self.compile(
            &format!("Recall: {}", request.task),
            Some(request.agent_id.clone()),
            request.related_entities.clone(),
            None,
            request.limit,
        );
        let retrieval = self.memory_service.retrieve_memory(MemoryQuery {
            agent_id: request.agent_id,
            query: request.task,
            valid_at: Some(self.valid_at),
            related_entities: request.related_entities,
            include_history: false,
            mode: MemoryRetrievalMode::GraphTemporal,
            limit: request.limit,
        });
        let extra_sources = retrieval
            .memories
            .iter()
            .flat_map(|memory| memory.record.source_ids.iter().cloned())
            .collect::<Vec<_>>();
        response(
            compiled,
            RecallData {
                memories: retrieval
                    .memories
                    .into_iter()
                    .map(|memory| memory.record)
                    .collect(),
                quality_score: retrieval.quality_score,
            },
            &extra_sources,
            vec!["Recall returns current source-backed memories, not transcript blobs.".to_owned()],
        )
    }

    pub fn verify(&self, request: VerifyRequest) -> RealityResponse<VerifyData> {
        let compiled = self.compile(
            &format!("Verify claim: {}", request.claim),
            None,
            request.subject.iter().cloned().collect(),
            temporal_pair(request.valid_at, request.known_at),
            Some(8),
        );
        let supporting = self
            .matching_assertions(
                request.subject.as_ref(),
                request.predicate.as_ref(),
                request.valid_at,
                request.known_at,
            )
            .collect::<Vec<_>>();
        let supporting_ids = supporting
            .iter()
            .map(|assertion| assertion.id.clone())
            .collect::<Vec<_>>();
        let relevant_contradictions =
            contradictions_for_assertions(&self.storage, &supporting_ids.iter().cloned().collect());
        let status = if !relevant_contradictions.is_empty() {
            VerificationStatus::Contradicted
        } else if !supporting_ids.is_empty() {
            VerificationStatus::Supported
        } else {
            VerificationStatus::InsufficientEvidence
        };
        let source_ids = supporting
            .iter()
            .flat_map(|assertion| assertion.source_ids.iter().cloned())
            .collect::<Vec<_>>();
        response(
            compiled,
            VerifyData {
                status,
                supporting_assertions: supporting_ids,
                contradictions: relevant_contradictions,
                explanation: "Claim checked against source-backed assertions and conflicts."
                    .to_owned(),
            },
            &source_ids,
            vec![
                "Verification is evidence-backed and may return insufficient evidence.".to_owned(),
            ],
        )
    }

    pub fn explain(&self, request: ExplainRequest) -> RealityResponse<ExplainData> {
        let compiled = self.compile(
            &format!("Explain: {}", request.question),
            None,
            request.entity_id.iter().cloned().collect(),
            None,
            Some(8),
        );
        let mut explanation = format!(
            "Explanation is based on evidence from {} source-backed assertions.",
            compiled.evidence_pack.assertions.len()
        );
        let mut source_ids = source_ids_from_pack(&compiled.evidence_pack);
        if let Some(memory_id) = request.memory_id {
            if let Some(memory) = self.memory_service.explain_memory(&memory_id) {
                explanation.push_str(&format!(
                    " Memory {memory_id} is explained because {}.",
                    memory.reason
                ));
                source_ids.extend(memory.source_ids);
            }
        }
        response(
            compiled,
            ExplainData {
                explanation,
                evidence_source_ids: dedup(source_ids.clone()),
            },
            &source_ids,
            vec![
                "Explanation summarizes evidence; the graph decides what evidence exists."
                    .to_owned(),
            ],
        )
    }

    pub fn timeline(&self, request: TimelineRequest) -> RealityResponse<TimelineData> {
        let compiled = self.compile(
            &format!("Timeline for {}", request.entity_id),
            None,
            vec![request.entity_id.clone()],
            temporal_pair(request.valid_at, request.known_at),
            Some(12),
        );
        let mut assertions = self
            .assertions_for_entity(&request.entity_id, request.valid_at, request.known_at)
            .collect::<Vec<_>>();
        assertions.sort_by(|left, right| {
            left.valid_time
                .start
                .cmp(&right.valid_time.start)
                .then_with(|| left.id.cmp(&right.id))
        });
        let entity_name = self
            .storage
            .entity(&request.entity_id)
            .and_then(|entity| entity.canonical_name.clone())
            .unwrap_or_else(|| request.entity_id.to_string());
        let source_ids = assertions
            .iter()
            .flat_map(|assertion| assertion.source_ids.iter().cloned())
            .collect::<Vec<_>>();
        response(
            compiled,
            TimelineData {
                items: assertions
                    .into_iter()
                    .map(|assertion| TimelineItem {
                        assertion_id: assertion.id.clone(),
                        label: format!(
                            "{entity_name}: {} -> {}",
                            assertion.predicate,
                            graph_value_name(&assertion.object)
                        ),
                        valid_from: assertion.valid_time.start,
                        valid_to: assertion.valid_time.end,
                        source_ids: assertion.source_ids.clone(),
                    })
                    .collect(),
            },
            &source_ids,
            vec!["Timeline preserves valid time and source IDs.".to_owned()],
        )
    }

    pub fn simulate(&self, request: SimulateRequest) -> RealityResponse<SimulationReport> {
        let compiled = self.compile(
            &format!("Simulate: {}", request.action.description),
            Some(request.action.actor_agent_id.clone()),
            request.action.target_entities.clone(),
            Some((self.valid_at.as_i64(), self.known_at.as_i64())),
            Some(8),
        );
        let lab = AgentSimulationLab::new(SimulationContext {
            current_state_label: "Reality API current graph state".to_owned(),
            valid_at: self.valid_at,
            known_at: self.known_at,
            principal: self.principal.clone(),
            causal_graph: self.causal_graph.clone(),
            graph_state: self.storage.graph_state().clone(),
            memory_service: self.memory_service.clone(),
            governance: self.governance.clone(),
            evidence_compiler: self.compiler(),
        });
        let required_source_ids = request.action.required_source_ids.clone();
        let report = lab.simulate(request.action);
        response(
            compiled,
            report,
            &required_source_ids,
            vec![
                "Simulation output is prediction, not fact.".to_owned(),
                "High-risk actions are checked through governance policy.".to_owned(),
            ],
        )
    }

    pub fn context(&self, request: ContextRequest) -> RealityResponse<ContextData> {
        let compiled = self.compile(
            &request.question,
            request.agent_id,
            request.entity_ids,
            None,
            request.max_evidence_items,
        );
        response(
            compiled.clone(),
            ContextData {
                context_pack: compiled.evidence_pack.clone(),
            },
            &[],
            vec!["Context packs are model-ready and source-backed.".to_owned()],
        )
    }

    pub fn contradictions(
        &self,
        request: ContradictionsRequest,
    ) -> RealityResponse<ContradictionsData> {
        let compiled = self.compile(&request.question, None, Vec::new(), None, Some(12));
        let mut index = TemporalIndex::new();
        for assertion in self.storage.graph_state().assertions.values() {
            index.insert_assertion(assertion.clone());
        }
        let unresolved = index.contradictions();
        response(
            compiled,
            ContradictionsData { unresolved },
            &source_ids_from_storage(&self.storage),
            vec!["Contradictions preserve competing claims instead of collapsing them.".to_owned()],
        )
    }

    pub fn state(&self, request: StateRequest) -> RealityResponse<StateData> {
        let compiled = self.compile(
            &request.question,
            None,
            request.entity_id.iter().cloned().collect(),
            temporal_pair(request.valid_at, request.known_at),
            Some(12),
        );
        let assertions = request.entity_id.as_ref().map_or_else(
            || {
                self.storage
                    .graph_state()
                    .assertions
                    .values()
                    .filter(|assertion| visible(assertion, request.valid_at, request.known_at))
                    .cloned()
                    .collect::<Vec<_>>()
            },
            |entity_id| {
                self.assertions_for_entity(entity_id, request.valid_at, request.known_at)
                    .cloned()
                    .collect::<Vec<_>>()
            },
        );
        let source_ids = assertions
            .iter()
            .flat_map(|assertion| assertion.source_ids.iter().cloned())
            .collect::<Vec<_>>();
        response(
            compiled,
            StateData {
                assertions: assertions.iter().map(QueryResult::from_assertion).collect(),
            },
            &source_ids,
            vec![
                "State answers current or historical world state with temporal filters.".to_owned(),
            ],
        )
    }

    fn compile(
        &self,
        question: &str,
        agent_id: Option<AgentId>,
        entity_ids: Vec<EntityId>,
        temporal_constraints: Option<(i64, i64)>,
        max_evidence_items: Option<usize>,
    ) -> CompiledEvidencePack {
        self.compiler().compile(CompilationRequest {
            question: question.to_owned(),
            agent_state: (agent_id.is_some() || !entity_ids.is_empty()).then(|| AgentState {
                agent_id: agent_id.map(|id| id.to_string()),
                user_id: None,
                active_entity_ids: entity_ids.iter().map(ToString::to_string).collect(),
            }),
            temporal_constraints,
            budget: RetrievalBudget {
                max_evidence_items: max_evidence_items.unwrap_or(8),
                max_path_depth: 3,
                ..RetrievalBudget::default()
            },
            available_tools: RetrievalTool::all(),
            ..CompilationRequest::default()
        })
    }

    fn compiler(&self) -> EvidencePackCompiler {
        EvidencePackCompiler::new(self.storage.clone())
    }

    fn matching_assertions<'a>(
        &'a self,
        subject: Option<&'a EntityId>,
        predicate: Option<&'a PredicateId>,
        valid_at: Option<ValidTime>,
        known_at: Option<TxTime>,
    ) -> impl Iterator<Item = &'a Assertion> {
        self.storage
            .graph_state()
            .assertions
            .values()
            .filter(move |assertion| {
                subject.map_or(true, |subject| &assertion.subject == subject)
                    && predicate.map_or(true, |predicate| &assertion.predicate == predicate)
                    && visible(assertion, valid_at, known_at)
            })
    }

    fn assertions_for_entity<'a>(
        &'a self,
        entity_id: &'a EntityId,
        valid_at: Option<ValidTime>,
        known_at: Option<TxTime>,
    ) -> impl Iterator<Item = &'a Assertion> {
        self.storage
            .graph_state()
            .assertions
            .values()
            .filter(move |assertion| {
                (&assertion.subject == entity_id
                    || matches!(&assertion.object, GraphValue::Entity(object) if object == entity_id))
                    && visible(assertion, valid_at, known_at)
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecommendedProductApi {
    Memory,
    Verification,
    Explanation,
    Simulation,
    Context,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RealityResponse<T> {
    pub data: T,
    pub evidence_pack: EvidencePack,
    pub retrieval_plan: RetrievalPlan,
    pub retrieval_trace: RetrievalTrace,
    pub evidence_backed: bool,
    pub safety_notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RememberRequest {
    pub agent_id: AgentId,
    pub memory_id: MemoryId,
    pub memory_type: AgentMemoryKind,
    pub content: String,
    pub source_ids: Vec<SourceId>,
    pub related_entities: Vec<EntityId>,
    pub confidence: Confidence,
    pub valid_time: TimeInterval<ValidTime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RememberData {
    pub memory_id: MemoryId,
    pub stored: bool,
    pub source_ids: Vec<SourceId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallRequest {
    pub agent_id: AgentId,
    pub task: String,
    pub related_entities: Vec<EntityId>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecallData {
    pub memories: Vec<MemoryRecord>,
    pub quality_score: f32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifyRequest {
    pub claim: String,
    pub subject: Option<EntityId>,
    pub predicate: Option<PredicateId>,
    pub valid_at: Option<ValidTime>,
    pub known_at: Option<TxTime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationStatus {
    Supported,
    Contradicted,
    InsufficientEvidence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VerifyData {
    pub status: VerificationStatus,
    pub supporting_assertions: Vec<AssertionId>,
    pub contradictions: Vec<Contradiction>,
    pub explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplainRequest {
    pub question: String,
    pub entity_id: Option<EntityId>,
    pub memory_id: Option<MemoryId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplainData {
    pub explanation: String,
    pub evidence_source_ids: Vec<SourceId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimelineRequest {
    pub entity_id: EntityId,
    pub valid_at: Option<ValidTime>,
    pub known_at: Option<TxTime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimelineData {
    pub items: Vec<TimelineItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimelineItem {
    pub assertion_id: AssertionId,
    pub label: String,
    pub valid_from: ValidTime,
    pub valid_to: Option<ValidTime>,
    pub source_ids: Vec<SourceId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimulateRequest {
    pub action: ProposedAction,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextRequest {
    pub question: String,
    pub agent_id: Option<AgentId>,
    pub entity_ids: Vec<EntityId>,
    pub max_evidence_items: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextData {
    pub context_pack: EvidencePack,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContradictionsRequest {
    pub question: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContradictionsData {
    pub unresolved: Vec<Contradiction>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateRequest {
    pub question: String,
    pub entity_id: Option<EntityId>,
    pub valid_at: Option<ValidTime>,
    pub known_at: Option<TxTime>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StateData {
    pub assertions: Vec<QueryResult>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RealityApiError {
    MissingSourceEvidence,
    Memory(AgentMemoryError),
}

impl fmt::Display for RealityApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSourceEvidence => {
                formatter.write_str("remember requires at least one source ID")
            }
            Self::Memory(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for RealityApiError {}

impl From<AgentMemoryError> for RealityApiError {
    fn from(value: AgentMemoryError) -> Self {
        Self::Memory(value)
    }
}

fn response<T>(
    compiled: CompiledEvidencePack,
    data: T,
    extra_source_ids: &[SourceId],
    mut safety_notes: Vec<String>,
) -> RealityResponse<T> {
    safety_notes.push("Response was produced through the retrieval compiler.".to_owned());
    RealityResponse {
        data,
        evidence_backed: evidence_backed(&compiled.evidence_pack, extra_source_ids),
        evidence_pack: compiled.evidence_pack,
        retrieval_plan: compiled.plan,
        retrieval_trace: compiled.trace,
        safety_notes,
    }
}

fn evidence_backed(pack: &EvidencePack, extra_source_ids: &[SourceId]) -> bool {
    !extra_source_ids.is_empty()
        || !pack.sources.is_empty()
        || pack
            .assertions
            .iter()
            .any(|assertion| !assertion.source_ids.is_empty())
}

fn temporal_pair(valid_at: Option<ValidTime>, known_at: Option<TxTime>) -> Option<(i64, i64)> {
    match (valid_at, known_at) {
        (Some(valid_at), Some(known_at)) => Some((valid_at.as_i64(), known_at.as_i64())),
        _ => None,
    }
}

fn visible(assertion: &Assertion, valid_at: Option<ValidTime>, known_at: Option<TxTime>) -> bool {
    valid_at.map_or(true, |valid_at| assertion.valid_time.contains(valid_at))
        && known_at.map_or(true, |known_at| {
            assertion.transaction_time.contains(known_at)
        })
}

fn contradictions_for_assertions(
    storage: &InMemoryStorage,
    assertion_ids: &BTreeSet<AssertionId>,
) -> Vec<Contradiction> {
    let mut index = TemporalIndex::new();
    for assertion in storage.graph_state().assertions.values() {
        index.insert_assertion(assertion.clone());
    }
    index
        .contradictions()
        .into_iter()
        .filter(|contradiction| {
            assertion_ids.contains(&contradiction.assertion_a)
                || assertion_ids.contains(&contradiction.assertion_b)
        })
        .collect()
}

fn source_ids_from_pack(pack: &EvidencePack) -> Vec<SourceId> {
    pack.sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect()
}

fn source_ids_from_storage(storage: &InMemoryStorage) -> Vec<SourceId> {
    storage.graph_state().sources.keys().cloned().collect()
}

fn dedup(source_ids: Vec<SourceId>) -> Vec<SourceId> {
    source_ids
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn graph_value_name(value: &GraphValue) -> String {
    match value {
        GraphValue::Entity(entity_id) => entity_id.to_string(),
        GraphValue::Text(value) => value.clone(),
        GraphValue::Integer(value) => value.to_string(),
        GraphValue::Decimal(value) => value.to_string(),
        GraphValue::Boolean(value) => value.to_string(),
        GraphValue::Time(value) => value.as_i64().to_string(),
        GraphValue::Null => "null".to_owned(),
    }
}

fn agent_from_principal(principal: &Principal) -> AgentId {
    principal
        .agent_id
        .clone()
        .unwrap_or_else(|| AgentId::new(principal.id.to_string()))
}

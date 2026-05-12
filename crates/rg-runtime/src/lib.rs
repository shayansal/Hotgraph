//! Experimental direct model-runtime integration APIs.

use rg_agent_memory::{
    AgentMemoryError, AgentMemoryKind, AgentMemoryService, MemoryPermissions, MemoryRecord,
    WriteMemory,
};
use rg_ai::EvidencePack;
use rg_belief::{
    BeliefEngine, BeliefQuery, BeliefState, Claim, ResolutionPolicy, SourceTrustModel,
};
use rg_core::{
    AgentId, AssertionId, Confidence, EntityId, MemoryId, MemoryStatus, SourceId, TimeInterval,
    TxTime, ValidTime,
};
use rg_retrieval_compiler::{
    AgentState, CompilationRequest, CompiledEvidencePack, DomainOntology, EvidencePackCompiler,
    RetrievalBudget, RetrievalOperator, RetrievalTool,
};
use rg_storage::InMemoryStorage;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimePhase {
    PreAttentionContextInjection,
    RetrievalDuringDecoding,
    MemoryAwareSpeculativeDecoding,
    GraphConditionedPlanning,
    ExternalBeliefStateCache,
    LongContextRefresh,
    AgentLoopMemoryHook,
    FinalAnswerVerification,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeculativeDecodeHint {
    None,
    PreferEvidenceGathering,
    PreferPlanning,
    PreferMemoryWrite,
    DeferFinalAnswer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeProfile {
    pub runtime_name: String,
    pub pre_attention_context: bool,
    pub retrieval_during_decoding: bool,
    pub speculative_decoding: bool,
    pub graph_conditioned_planning: bool,
    pub long_context_refresh: bool,
    pub agent_loop_hooks: bool,
    pub max_context_tokens: usize,
}

impl RuntimeProfile {
    pub fn prefill(runtime_name: impl Into<String>) -> Self {
        Self {
            runtime_name: runtime_name.into(),
            pre_attention_context: true,
            retrieval_during_decoding: true,
            speculative_decoding: false,
            graph_conditioned_planning: true,
            long_context_refresh: true,
            agent_loop_hooks: true,
            max_context_tokens: 4_096,
        }
    }

    pub fn local_agent(runtime_name: impl Into<String>) -> Self {
        Self {
            runtime_name: runtime_name.into(),
            pre_attention_context: true,
            retrieval_during_decoding: true,
            speculative_decoding: true,
            graph_conditioned_planning: true,
            long_context_refresh: true,
            agent_loop_hooks: true,
            max_context_tokens: 8_192,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentLoopState {
    pub agent_id: AgentId,
    pub task: String,
    pub turn: u64,
    pub active_entities: Vec<EntityId>,
    pub last_refresh_turn: Option<u64>,
}

impl AgentLoopState {
    pub fn new(agent_id: AgentId, task: impl Into<String>) -> Self {
        Self {
            agent_id,
            task: task.into(),
            turn: 0,
            active_entities: Vec::new(),
            last_refresh_turn: None,
        }
    }

    pub fn with_turn(mut self, turn: u64) -> Self {
        self.turn = turn;
        self
    }

    pub fn with_active_entity(mut self, entity_id: EntityId) -> Self {
        self.active_entities.push(entity_id);
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextDelta {
    pub token_estimate: usize,
    pub new_assertion_ids: Vec<AssertionId>,
    pub new_source_ids: Vec<SourceId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrefillContext {
    pub phase: RuntimePhase,
    pub context_pack: EvidencePack,
    pub prompt_prefix: String,
    pub citation_coverage: f32,
    pub recommended_integration: String,
    pub hook_trace: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextRefresh {
    pub phase: RuntimePhase,
    pub compiled_pack: CompiledEvidencePack,
    pub context_delta: ContextDelta,
    pub hook_trace: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolChoiceContext {
    pub phase: RuntimePhase,
    pub selected_tool: Option<String>,
    pub context_pack: EvidencePack,
    pub plan_operators: Vec<RetrievalOperator>,
    pub speculative_decode_hint: SpeculativeDecodeHint,
    pub hook_trace: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalAnswerVerification {
    pub phase: RuntimePhase,
    pub allowed_to_answer: bool,
    pub final_answer_guardrail: String,
    pub supporting_assertion_id: Option<AssertionId>,
    pub hook_trace: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentActionOutcome {
    pub memory_id: MemoryId,
    pub agent_id: AgentId,
    pub content: String,
    pub source_ids: Vec<SourceId>,
    pub related_entities: Vec<EntityId>,
    pub valid_at: ValidTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeMemoryWrite {
    pub phase: RuntimePhase,
    pub memory: MemoryRecord,
    pub hook_trace: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BeliefObservation {
    pub claim: Claim,
    pub valid_at: ValidTime,
    pub known_at: TxTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BeliefRuntimeUpdate {
    pub phase: RuntimePhase,
    pub cache_key: String,
    pub belief_state: BeliefState,
    pub hook_trace: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelRuntimeBridge {
    storage: InMemoryStorage,
    compiler: EvidencePackCompiler,
    memory_service: AgentMemoryService,
    belief_engine: BeliefEngine,
}

impl ModelRuntimeBridge {
    pub fn new(storage: InMemoryStorage, memory_service: AgentMemoryService) -> Self {
        let compiler = EvidencePackCompiler::new(storage.clone());
        Self {
            storage,
            compiler,
            memory_service,
            belief_engine: BeliefEngine::new(
                ResolutionPolicy::trust_weighted(),
                SourceTrustModel::new(0.75),
            ),
        }
    }

    pub fn prefill_context_pack(&self, question: &str, profile: RuntimeProfile) -> PrefillContext {
        let compiled = self.compile_for_question(question, None, Some(6));
        let prompt_prefix = render_prefill_prefix(&compiled.evidence_pack);
        let recommended_integration = integration_hint(&profile);
        PrefillContext {
            phase: RuntimePhase::PreAttentionContextInjection,
            citation_coverage: compiled.citation_coverage(),
            context_pack: compiled.evidence_pack,
            prompt_prefix,
            recommended_integration,
            hook_trace: vec![
                format!("prefill_context_pack requested by {}", profile.runtime_name),
                "compiled evidence before model prefill".to_owned(),
                "context is safe to inject before attention only with citation metadata".to_owned(),
            ],
        }
    }

    pub fn refresh_context_during_agent_loop(&self, state: &mut AgentLoopState) -> ContextRefresh {
        let agent_state = AgentState {
            agent_id: Some(state.agent_id.to_string()),
            user_id: None,
            active_entity_ids: state
                .active_entities
                .iter()
                .map(ToString::to_string)
                .collect(),
        };
        let compiled = self.compile_for_question(&state.task, Some(agent_state), Some(4));
        state.last_refresh_turn = Some(state.turn);
        let context_delta = delta_from_pack(&compiled.evidence_pack);
        ContextRefresh {
            phase: RuntimePhase::LongContextRefresh,
            compiled_pack: compiled,
            context_delta,
            hook_trace: vec![
                format!("agent loop turn {} requested context refresh", state.turn),
                "long-context refresh hook rebuilt compact evidence delta".to_owned(),
            ],
        }
    }

    pub fn retrieve_before_tool_choice(
        &self,
        question: &str,
        candidate_tools: Vec<&str>,
    ) -> ToolChoiceContext {
        let compiled = self.compile_for_question(question, None, Some(5));
        let selected_tool = choose_tool(question, &candidate_tools, &compiled);
        let speculative_decode_hint = match selected_tool.as_deref() {
            Some("verify_claim") => SpeculativeDecodeHint::PreferEvidenceGathering,
            Some("write_memory") => SpeculativeDecodeHint::PreferMemoryWrite,
            _ if compiled
                .plan
                .operators
                .contains(&RetrievalOperator::PathSearch) =>
            {
                SpeculativeDecodeHint::PreferPlanning
            }
            _ => SpeculativeDecodeHint::None,
        };
        ToolChoiceContext {
            phase: RuntimePhase::RetrievalDuringDecoding,
            selected_tool,
            context_pack: compiled.evidence_pack,
            plan_operators: compiled.plan.operators,
            speculative_decode_hint,
            hook_trace: vec![
                "retrieved evidence before tool choice".to_owned(),
                "tool bias is advisory and should remain inspectable".to_owned(),
            ],
        }
    }

    pub fn verify_before_final_answer(
        &self,
        answer: &str,
        supporting_assertion_id: Option<AssertionId>,
    ) -> FinalAnswerVerification {
        let assertion_exists = supporting_assertion_id
            .as_ref()
            .and_then(|id| self.storage.assertion(id))
            .is_some();
        let allowed_to_answer = assertion_exists && !answer.trim().is_empty();
        let final_answer_guardrail = if allowed_to_answer {
            "final answer is grounded by a source-backed assertion".to_owned()
        } else {
            "blocked: insufficient evidence for final answer".to_owned()
        };
        FinalAnswerVerification {
            phase: RuntimePhase::FinalAnswerVerification,
            allowed_to_answer,
            final_answer_guardrail,
            supporting_assertion_id,
            hook_trace: vec![
                "verify_before_final_answer checked support before model response".to_owned(),
            ],
        }
    }

    pub fn write_memory_after_action(
        &mut self,
        outcome: AgentActionOutcome,
    ) -> Result<RuntimeMemoryWrite, AgentMemoryError> {
        let memory = self.memory_service.write_memory(WriteMemory {
            id: outcome.memory_id,
            agent_id: outcome.agent_id.clone(),
            memory_type: AgentMemoryKind::Episodic,
            content: outcome.content,
            valid_time: TimeInterval::new(outcome.valid_at, None).expect("open interval is valid"),
            confidence: Confidence::new(0.8).expect("constant confidence is valid"),
            source_ids: outcome.source_ids,
            related_entities: outcome.related_entities,
            supersedes: Vec::new(),
            contradicts: Vec::new(),
            lifecycle: MemoryStatus::Active,
            permissions: MemoryPermissions::private(outcome.agent_id),
        })?;
        Ok(RuntimeMemoryWrite {
            phase: RuntimePhase::AgentLoopMemoryHook,
            memory,
            hook_trace: vec![
                "write_memory_after_action recorded action outcome as episodic memory".to_owned(),
                "action outcome memory remains source-backed".to_owned(),
            ],
        })
    }

    pub fn update_belief_after_observation(
        &mut self,
        observation: BeliefObservation,
    ) -> BeliefRuntimeUpdate {
        let subject = observation.claim.subject.clone();
        let predicate = observation.claim.predicate.clone();
        self.belief_engine.ingest_claim(observation.claim);
        let belief_state = self.belief_engine.belief_state(BeliefQuery {
            subject: subject.clone(),
            predicate: predicate.clone(),
            valid_at: observation.valid_at,
            known_at: observation.known_at,
        });
        let cache_key = format!(
            "{}|{}|{}|{}",
            subject,
            predicate,
            observation.valid_at.as_i64(),
            observation.known_at.as_i64()
        );
        BeliefRuntimeUpdate {
            phase: RuntimePhase::ExternalBeliefStateCache,
            cache_key,
            belief_state,
            hook_trace: vec![
                "update_belief_after_observation ingested observed claim".to_owned(),
                "external belief-state cache key updated".to_owned(),
            ],
        }
    }

    fn compile_for_question(
        &self,
        question: &str,
        agent_state: Option<AgentState>,
        max_evidence_items: Option<usize>,
    ) -> CompiledEvidencePack {
        let mut budget = RetrievalBudget::default();
        if let Some(max_evidence_items) = max_evidence_items {
            budget.max_evidence_items = max_evidence_items;
        }
        self.compiler.compile(CompilationRequest {
            question: question.to_owned(),
            agent_state,
            temporal_constraints: Some((2024, 8)),
            domain_ontology: domain_ontology_for_runtime(question, &self.storage),
            budget,
            available_tools: RetrievalTool::all(),
            ..CompilationRequest::default()
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IntegrationKind {
    OpenSourceInferenceServer,
    LocalAgentRuntime,
    ResearchNotebook,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeIntegration {
    pub kind: IntegrationKind,
    pub name: String,
    pub hooks: Vec<RuntimePhase>,
    pub description: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeIntegrationCatalog {
    integrations: Vec<RuntimeIntegration>,
}

impl RuntimeIntegrationCatalog {
    pub fn default_catalog() -> Self {
        Self {
            integrations: vec![
                RuntimeIntegration {
                    kind: IntegrationKind::OpenSourceInferenceServer,
                    name: "vLLM/TGI prefill adapter".to_owned(),
                    hooks: vec![
                        RuntimePhase::PreAttentionContextInjection,
                        RuntimePhase::RetrievalDuringDecoding,
                        RuntimePhase::FinalAnswerVerification,
                    ],
                    description:
                        "Inject compact evidence packs before prefill and refresh them between decoding phases."
                            .to_owned(),
                },
                RuntimeIntegration {
                    kind: IntegrationKind::LocalAgentRuntime,
                    name: "LangGraph-style loop hooks".to_owned(),
                    hooks: vec![
                        RuntimePhase::LongContextRefresh,
                        RuntimePhase::AgentLoopMemoryHook,
                        RuntimePhase::ExternalBeliefStateCache,
                    ],
                    description:
                        "Call memory refresh, memory write, and belief update hooks around agent-loop nodes."
                            .to_owned(),
                },
                RuntimeIntegration {
                    kind: IntegrationKind::ResearchNotebook,
                    name: "Notebook replay harness".to_owned(),
                    hooks: vec![
                        RuntimePhase::GraphConditionedPlanning,
                        RuntimePhase::MemoryAwareSpeculativeDecoding,
                        RuntimePhase::FinalAnswerVerification,
                    ],
                    description:
                        "Run deterministic model-loop experiments with saved context packs and traces."
                            .to_owned(),
                },
            ],
        }
    }

    pub fn integrations_for(&self, kind: IntegrationKind) -> Vec<&RuntimeIntegration> {
        self.integrations
            .iter()
            .filter(|integration| integration.kind == kind)
            .collect()
    }

    pub fn integrations(&self) -> &[RuntimeIntegration] {
        &self.integrations
    }
}

fn render_prefill_prefix(pack: &EvidencePack) -> String {
    let assertion_lines = pack
        .assertions
        .iter()
        .map(|assertion| {
            format!(
                "- {} {} {:?} sources={}",
                assertion.subject,
                assertion.predicate,
                assertion.object,
                assertion
                    .source_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Reality Graph context pack\nquery: {}\nassertions:\n{}\n",
        pack.query, assertion_lines
    )
}

fn integration_hint(profile: &RuntimeProfile) -> String {
    if profile.runtime_name.contains("inference") || profile.runtime_name.contains("vllm") {
        "open-source inference server prefill integration".to_owned()
    } else if profile.agent_loop_hooks {
        "local agent runtime hook integration".to_owned()
    } else {
        "research notebook replay integration".to_owned()
    }
}

fn delta_from_pack(pack: &EvidencePack) -> ContextDelta {
    let new_assertion_ids = pack
        .assertions
        .iter()
        .map(|assertion| assertion.id.clone())
        .collect::<Vec<_>>();
    let new_source_ids = pack
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<Vec<_>>();
    ContextDelta {
        token_estimate: pack
            .assertions
            .iter()
            .map(|assertion| assertion.source_ids.len().max(1) * 24)
            .sum::<usize>()
            .max(1),
        new_assertion_ids,
        new_source_ids,
    }
}

fn choose_tool(
    question: &str,
    candidate_tools: &[&str],
    compiled: &CompiledEvidencePack,
) -> Option<String> {
    let lowered = question.to_ascii_lowercase();
    let preferred = if lowered.contains("verify") || !compiled.evidence_pack.assertions.is_empty() {
        "verify_claim"
    } else if lowered.contains("memory") {
        "write_memory"
    } else {
        "send_email"
    };
    candidate_tools
        .iter()
        .copied()
        .find(|tool| *tool == preferred)
        .or_else(|| candidate_tools.first().copied())
        .map(str::to_owned)
}

fn domain_ontology_for_runtime(question: &str, storage: &InMemoryStorage) -> DomainOntology {
    DomainOntology {
        preferred_predicates: predicates_for_question(question, storage),
        ..DomainOntology::default()
    }
}

fn predicates_for_question(question: &str, storage: &InMemoryStorage) -> Vec<String> {
    let question = normalize(question);
    let mut predicates = BTreeSet::new();
    for assertion in storage.graph_state().assertions.values() {
        let predicate = assertion.predicate.as_str();
        if predicate_matches_question(&question, &normalize(predicate)) {
            predicates.insert(predicate.to_owned());
        }
    }
    predicates.into_iter().collect()
}

fn predicate_matches_question(question: &str, predicate: &str) -> bool {
    (contains_any(question, &["work", "worked", "employ", "job"])
        && contains_any(predicate, &["worked at", "works at", "employ"]))
        || (contains_any(question, &["own", "owns", "ownership", "control"])
            && contains_any(predicate, &["owns", "ownership", "controls"]))
        || (contains_any(question, &["ceo", "chief executive"])
            && contains_any(predicate, &["ceo of", "chief executive"]))
        || (contains_any(question, &["supply", "supplier"])
            && contains_any(predicate, &["supplies", "supplier"]))
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['_', '-'], " ")
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

//! Adaptive context compilation for Reality Graph.

use std::collections::BTreeSet;

use rg_ai::{EvidencePack, EvidencePackGenerator, EvidencePackRequest};
use rg_core::{Assertion, ContextScope, Entity, EntityId, GraphValue, PredicateId, TxTime};
use rg_query::{EntityPattern, GraphQuery, PathQuery, PredicatePattern, QueryEngine};
use rg_storage::InMemoryStorage;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryIntent {
    SimpleFact,
    Relationship,
    Historical,
    MultiHop,
    Ambiguous,
    BroadGlobal,
    ContradictoryEvidence,
    Causal,
    AgentMemory,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetrievalPlan {
    pub intent: QueryIntent,
    pub operators: Vec<RetrievalOperator>,
    pub budget: RetrievalBudget,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RetrievalOperator {
    VectorSearch,
    KeywordSearch,
    TemporalFilter,
    GraphExpansion,
    PathSearch,
    CommunitySearch,
    CausalExpansion,
    ContradictionCheck,
    Rerank,
    Compress,
    Cite,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetrievalBudget {
    pub max_latency_micros: u64,
    pub max_cost_units: f32,
    pub max_evidence_items: usize,
    pub max_path_depth: usize,
}

impl Default for RetrievalBudget {
    fn default() -> Self {
        Self {
            max_latency_micros: 1_000,
            max_cost_units: 3.0,
            max_evidence_items: 6,
            max_path_depth: 2,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetrievalTrace {
    pub steps: Vec<RetrievalTraceStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetrievalTraceStep {
    pub operator: RetrievalOperator,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetrievalTool {
    VectorIndex,
    KeywordIndex,
    GraphIndex,
    TemporalIndex,
    CommunityIndex,
    CausalGraph,
    ContradictionIndex,
    Compressor,
    CitationBuilder,
}

impl RetrievalTool {
    pub fn all() -> Vec<Self> {
        vec![
            Self::VectorIndex,
            Self::KeywordIndex,
            Self::GraphIndex,
            Self::TemporalIndex,
            Self::CommunityIndex,
            Self::CausalGraph,
            Self::ContradictionIndex,
            Self::Compressor,
            Self::CitationBuilder,
        ]
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentState {
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
    pub active_entity_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DomainOntology {
    pub preferred_predicates: Vec<String>,
    pub mutually_exclusive_predicates: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrustPolicy {
    pub min_confidence: Option<f32>,
    pub required_source_ids: Vec<String>,
}

impl Default for TrustPolicy {
    fn default() -> Self {
        Self {
            min_confidence: Some(0.0),
            required_source_ids: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompilationRequest {
    pub question: String,
    pub agent_state: Option<AgentState>,
    pub temporal_constraints: Option<(i64, i64)>,
    pub domain_ontology: DomainOntology,
    pub trust_policy: TrustPolicy,
    pub budget: RetrievalBudget,
    pub available_tools: Vec<RetrievalTool>,
}

impl Default for CompilationRequest {
    fn default() -> Self {
        Self {
            question: String::new(),
            agent_state: None,
            temporal_constraints: None,
            domain_ontology: DomainOntology::default(),
            trust_policy: TrustPolicy::default(),
            budget: RetrievalBudget::default(),
            available_tools: RetrievalTool::all(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledEvidencePack {
    pub evidence_pack: EvidencePack,
    pub plan: RetrievalPlan,
    pub trace: RetrievalTrace,
}

impl CompiledEvidencePack {
    pub fn citation_coverage(&self) -> f32 {
        if self.evidence_pack.assertions.is_empty() {
            return 1.0;
        }
        let cited = self
            .evidence_pack
            .assertions
            .iter()
            .filter(|assertion| !assertion.source_ids.is_empty())
            .count();
        cited as f32 / self.evidence_pack.assertions.len() as f32
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvidencePackCompiler {
    storage: InMemoryStorage,
}

impl EvidencePackCompiler {
    pub fn new(storage: InMemoryStorage) -> Self {
        Self { storage }
    }

    pub fn compile(&self, request: CompilationRequest) -> CompiledEvidencePack {
        let intent = infer_intent(&request, &self.storage);
        let plan = self.plan(&request, intent);
        let trace = trace_for_plan(&plan, &request);
        let evidence_pack = self.execute_plan(&request, &plan);
        CompiledEvidencePack {
            evidence_pack,
            plan,
            trace,
        }
    }

    pub fn vector_only_baseline(&self, request: &CompilationRequest) -> EvidencePack {
        let best = self
            .storage
            .graph_state()
            .assertions
            .values()
            .filter(|assertion| {
                request
                    .temporal_constraints
                    .map_or(true, |(valid_at, known_at)| {
                        assertion
                            .valid_time
                            .contains(rg_core::ValidTime::new(valid_at))
                            && assertion
                                .transaction_time
                                .contains(rg_core::TxTime::new(known_at))
                    })
            })
            .max_by(|left, right| {
                assertion_vector_score(&request.question, left, &self.storage)
                    .total_cmp(&assertion_vector_score(
                        &request.question,
                        right,
                        &self.storage,
                    ))
                    .then_with(|| right.id.cmp(&left.id))
            });

        let graph_query = best.map_or_else(
            || broad_query(request, None, None),
            |assertion| GraphQuery {
                subject: Some(EntityPattern::Id(assertion.subject.clone())),
                predicate: Some(PredicatePattern::Id(assertion.predicate.clone())),
                object: None,
                valid_at: request.temporal_constraints.map(|(valid_at, _)| valid_at),
                known_at: request.temporal_constraints.map(|(_, known_at)| known_at),
                context: None,
                min_confidence: request.trust_policy.min_confidence,
                limit: Some(1),
            },
        );
        EvidencePackGenerator::new(&self.storage).generate(EvidencePackRequest {
            query: request.question.clone(),
            graph_query,
            path_query: None,
            generated_at: generated_at(&self.storage),
        })
    }

    fn plan(&self, request: &CompilationRequest, intent: QueryIntent) -> RetrievalPlan {
        let mut operators = match intent {
            QueryIntent::SimpleFact => vec![
                RetrievalOperator::KeywordSearch,
                RetrievalOperator::VectorSearch,
                RetrievalOperator::Rerank,
                RetrievalOperator::Compress,
                RetrievalOperator::Cite,
            ],
            QueryIntent::Relationship => vec![
                RetrievalOperator::GraphExpansion,
                RetrievalOperator::VectorSearch,
                RetrievalOperator::Rerank,
                RetrievalOperator::Compress,
                RetrievalOperator::Cite,
            ],
            QueryIntent::Historical => vec![
                RetrievalOperator::TemporalFilter,
                RetrievalOperator::GraphExpansion,
                RetrievalOperator::Rerank,
                RetrievalOperator::Compress,
                RetrievalOperator::Cite,
            ],
            QueryIntent::MultiHop => vec![
                RetrievalOperator::PathSearch,
                RetrievalOperator::GraphExpansion,
                RetrievalOperator::VectorSearch,
                RetrievalOperator::Rerank,
                RetrievalOperator::Compress,
                RetrievalOperator::Cite,
            ],
            QueryIntent::Ambiguous => vec![
                RetrievalOperator::KeywordSearch,
                RetrievalOperator::VectorSearch,
                RetrievalOperator::CommunitySearch,
                RetrievalOperator::Rerank,
                RetrievalOperator::Compress,
                RetrievalOperator::Cite,
            ],
            QueryIntent::BroadGlobal => vec![
                RetrievalOperator::CommunitySearch,
                RetrievalOperator::TemporalFilter,
                RetrievalOperator::GraphExpansion,
                RetrievalOperator::Rerank,
                RetrievalOperator::Compress,
                RetrievalOperator::Cite,
            ],
            QueryIntent::ContradictoryEvidence => vec![
                RetrievalOperator::KeywordSearch,
                RetrievalOperator::TemporalFilter,
                RetrievalOperator::GraphExpansion,
                RetrievalOperator::ContradictionCheck,
                RetrievalOperator::Rerank,
                RetrievalOperator::Compress,
                RetrievalOperator::Cite,
            ],
            QueryIntent::Causal => vec![
                RetrievalOperator::CausalExpansion,
                RetrievalOperator::TemporalFilter,
                RetrievalOperator::Rerank,
                RetrievalOperator::Compress,
                RetrievalOperator::Cite,
            ],
            QueryIntent::AgentMemory => vec![
                RetrievalOperator::KeywordSearch,
                RetrievalOperator::VectorSearch,
                RetrievalOperator::TemporalFilter,
                RetrievalOperator::Rerank,
                RetrievalOperator::Compress,
                RetrievalOperator::Cite,
            ],
        };

        operators.retain(|operator| operator_available(*operator, &request.available_tools));
        RetrievalPlan {
            intent,
            operators,
            budget: request.budget.clone(),
        }
    }

    fn execute_plan(&self, request: &CompilationRequest, plan: &RetrievalPlan) -> EvidencePack {
        let predicate = infer_predicate(request, &plan.intent);
        let entities = infer_entities(&request.question, &self.storage);
        let subject =
            subject_for_request(request, plan, &entities, predicate.as_ref(), &self.storage);
        let path_query = if plan.operators.contains(&RetrievalOperator::PathSearch) {
            path_query_for_request(request, plan, &entities, predicate.clone(), &self.storage)
        } else {
            None
        };
        let graph_query = broad_query(request, subject, predicate);
        let mut pack = EvidencePackGenerator::new(&self.storage).generate(EvidencePackRequest {
            query: request.question.clone(),
            graph_query,
            path_query,
            generated_at: generated_at(&self.storage),
        });
        compact_pack(&mut pack, request.budget.max_evidence_items);
        pack
    }
}

fn infer_intent(request: &CompilationRequest, storage: &InMemoryStorage) -> QueryIntent {
    let question = normalize(&request.question);
    if contains_any(
        &question,
        &["contradict", "conflict", "disagree", "both sides"],
    ) {
        return QueryIntent::ContradictoryEvidence;
    }
    if contains_any(&question, &["cause", "caused", "why", "led to", "impact"]) {
        return QueryIntent::Causal;
    }
    if request.agent_state.is_some()
        || contains_any(&question, &["remember", "memory", "preference", "agent"])
    {
        return QueryIntent::AgentMemory;
    }
    if contains_any(
        &question,
        &[
            "broad",
            "global",
            "overview",
            "summarize",
            "communities",
            "community",
        ],
    ) {
        return QueryIntent::BroadGlobal;
    }
    if contains_any(
        &question,
        &[
            "multi hop",
            "through",
            "chain",
            "ultimately",
            "connected",
            "path",
        ],
    ) {
        return QueryIntent::MultiHop;
    }
    if request.temporal_constraints.is_some()
        || contains_any(
            &question,
            &["historical", "in 2024", "in 2023", "what was true"],
        )
    {
        return QueryIntent::Historical;
    }
    if contains_any(
        &question,
        &[
            "relationship",
            "related",
            "owns",
            "works",
            "supplies",
            "ceo",
        ],
    ) {
        return QueryIntent::Relationship;
    }
    if infer_entities(&request.question, storage).is_empty() {
        QueryIntent::Ambiguous
    } else {
        QueryIntent::SimpleFact
    }
}

fn subject_for_request(
    request: &CompilationRequest,
    plan: &RetrievalPlan,
    entities: &[EntityId],
    predicate: Option<&PredicateId>,
    storage: &InMemoryStorage,
) -> Option<EntityId> {
    request
        .agent_state
        .as_ref()
        .and_then(|state| state.active_entity_ids.first())
        .map(EntityId::new)
        .or_else(|| {
            if plan.intent == QueryIntent::MultiHop {
                let end = entities.last()?;
                candidate_start_for_end(request, end, predicate, storage)
                    .or_else(|| entities.first().cloned())
            } else {
                entities.first().cloned()
            }
        })
}

fn path_query_for_request(
    request: &CompilationRequest,
    plan: &RetrievalPlan,
    entities: &[EntityId],
    predicate: Option<PredicateId>,
    storage: &InMemoryStorage,
) -> Option<PathQuery> {
    let end = entities.last().cloned();
    let start = if entities.len() >= 2 {
        entities.first().cloned()
    } else {
        end.as_ref()
            .and_then(|end| candidate_start_for_end(request, end, predicate.as_ref(), storage))
    }?;
    let predicates = predicate
        .map(|predicate| vec![predicate; plan.budget.max_path_depth])
        .unwrap_or_default();
    Some(PathQuery {
        start,
        end,
        predicates,
        valid_at: request.temporal_constraints.map(|(valid_at, _)| valid_at),
        max_depth: plan.budget.max_path_depth.max(1),
        min_confidence: request.trust_policy.min_confidence,
    })
}

fn candidate_start_for_end(
    request: &CompilationRequest,
    end: &EntityId,
    predicate: Option<&PredicateId>,
    storage: &InMemoryStorage,
) -> Option<EntityId> {
    let engine = QueryEngine::from_storage(storage.clone());
    let predicates = predicate
        .cloned()
        .map(|predicate| vec![predicate; request.budget.max_path_depth])
        .unwrap_or_default();
    storage
        .graph_state()
        .entities
        .keys()
        .filter(|entity| *entity != end)
        .find(|entity| {
            !engine
                .execute_path(PathQuery {
                    start: (*entity).clone(),
                    end: Some(end.clone()),
                    predicates: predicates.clone(),
                    valid_at: request.temporal_constraints.map(|(valid_at, _)| valid_at),
                    max_depth: request.budget.max_path_depth.max(1),
                    min_confidence: request.trust_policy.min_confidence,
                })
                .is_empty()
        })
        .cloned()
}

fn broad_query(
    request: &CompilationRequest,
    subject: Option<EntityId>,
    predicate: Option<PredicateId>,
) -> GraphQuery {
    GraphQuery {
        subject: subject.map(EntityPattern::Id),
        predicate: predicate.map(PredicatePattern::Id),
        object: None,
        valid_at: request.temporal_constraints.map(|(valid_at, _)| valid_at),
        known_at: request.temporal_constraints.map(|(_, known_at)| known_at),
        context: Some(ContextScope::Global),
        min_confidence: request.trust_policy.min_confidence,
        limit: Some(request.budget.max_evidence_items.max(1)),
    }
}

fn infer_predicate(request: &CompilationRequest, intent: &QueryIntent) -> Option<PredicateId> {
    if let Some(predicate) = request.domain_ontology.preferred_predicates.first() {
        return Some(PredicateId::new(predicate.clone()));
    }
    let question = normalize(&request.question);
    if contains_any(&question, &["work", "worked", "employ", "job"]) {
        Some(PredicateId::new("WORKED_AT"))
    } else if contains_any(&question, &["control", "own", "owns", "ownership"]) {
        Some(PredicateId::new("OWNS"))
    } else if contains_any(&question, &["ceo", "chief executive"]) {
        Some(PredicateId::new("CEO_OF"))
    } else if contains_any(&question, &["supply", "supplier"]) {
        Some(PredicateId::new("SUPPLIES"))
    } else if matches!(intent, QueryIntent::ContradictoryEvidence) {
        Some(PredicateId::new("CEO_OF"))
    } else {
        None
    }
}

fn infer_entities(question: &str, storage: &InMemoryStorage) -> Vec<EntityId> {
    let normalized_question = normalize(question);
    storage
        .graph_state()
        .entities
        .values()
        .filter(|entity| entity_matches_question(entity, &normalized_question))
        .map(|entity| entity.id.clone())
        .collect()
}

fn entity_matches_question(entity: &Entity, normalized_question: &str) -> bool {
    normalized_question.contains(&normalize(entity.id.as_str()))
        || entity
            .canonical_name
            .as_ref()
            .is_some_and(|name| normalized_question.contains(&normalize(name)))
}

fn trace_for_plan(plan: &RetrievalPlan, request: &CompilationRequest) -> RetrievalTrace {
    RetrievalTrace {
        steps: plan
            .operators
            .iter()
            .map(|operator| RetrievalTraceStep {
                operator: *operator,
                reason: operator_reason(*operator, &plan.intent, request),
            })
            .collect(),
    }
}

fn operator_reason(
    operator: RetrievalOperator,
    intent: &QueryIntent,
    request: &CompilationRequest,
) -> String {
    match operator {
        RetrievalOperator::VectorSearch => {
            if request.agent_state.is_some() {
                "selected for semantic recall using current agent state".to_owned()
            } else {
                "selected for semantic candidate retrieval".to_owned()
            }
        }
        RetrievalOperator::KeywordSearch => {
            "selected for exact lexical anchors in the question".to_owned()
        }
        RetrievalOperator::TemporalFilter => {
            "selected to enforce valid-time and transaction-time constraints".to_owned()
        }
        RetrievalOperator::GraphExpansion => {
            "selected because the query needs structured relationships".to_owned()
        }
        RetrievalOperator::PathSearch => {
            "selected because the query has multi-hop relationship language".to_owned()
        }
        RetrievalOperator::CommunitySearch => {
            if matches!(intent, QueryIntent::BroadGlobal) {
                "selected for temporal community retrieval over broad/global graph context"
                    .to_owned()
            } else {
                "selected to explore ambiguous entity neighborhoods".to_owned()
            }
        }
        RetrievalOperator::CausalExpansion => {
            "selected because the query asks for causal or impact reasoning".to_owned()
        }
        RetrievalOperator::ContradictionCheck => {
            "selected to retrieve both sides of conflicting evidence".to_owned()
        }
        RetrievalOperator::Rerank => {
            format!("selected to order evidence for {intent:?} intent")
        }
        RetrievalOperator::Compress => {
            "selected to fit the compact evidence-pack budget".to_owned()
        }
        RetrievalOperator::Cite => {
            "selected so every non-trivial answer carries source-backed citations".to_owned()
        }
    }
}

fn operator_available(operator: RetrievalOperator, available_tools: &[RetrievalTool]) -> bool {
    if available_tools.is_empty() {
        return true;
    }
    let required = match operator {
        RetrievalOperator::VectorSearch => RetrievalTool::VectorIndex,
        RetrievalOperator::KeywordSearch => RetrievalTool::KeywordIndex,
        RetrievalOperator::TemporalFilter => RetrievalTool::TemporalIndex,
        RetrievalOperator::GraphExpansion | RetrievalOperator::PathSearch => {
            RetrievalTool::GraphIndex
        }
        RetrievalOperator::CommunitySearch => RetrievalTool::CommunityIndex,
        RetrievalOperator::CausalExpansion => RetrievalTool::CausalGraph,
        RetrievalOperator::ContradictionCheck => RetrievalTool::ContradictionIndex,
        RetrievalOperator::Rerank => return true,
        RetrievalOperator::Compress => RetrievalTool::Compressor,
        RetrievalOperator::Cite => RetrievalTool::CitationBuilder,
    };
    available_tools.contains(&required)
}

fn compact_pack(pack: &mut EvidencePack, max_evidence_items: usize) {
    if max_evidence_items == 0 {
        return;
    }
    pack.assertions.truncate(max_evidence_items);
    let assertion_source_ids = pack
        .assertions
        .iter()
        .flat_map(|assertion| assertion.source_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    pack.sources
        .retain(|source| assertion_source_ids.contains(&source.source_id));
}

fn assertion_vector_score(question: &str, assertion: &Assertion, storage: &InMemoryStorage) -> f32 {
    let document = assertion_document(assertion, storage);
    let left = embedding(question);
    let right = embedding(&document);
    cosine_similarity(&left, &right)
}

fn assertion_document(assertion: &Assertion, storage: &InMemoryStorage) -> String {
    let subject = storage
        .entity(&assertion.subject)
        .and_then(|entity| entity.canonical_name.clone())
        .unwrap_or_else(|| assertion.subject.as_str().to_owned());
    let object = match &assertion.object {
        GraphValue::Entity(id) => storage
            .entity(id)
            .and_then(|entity| entity.canonical_name.clone())
            .unwrap_or_else(|| id.as_str().to_owned()),
        GraphValue::Text(value) => value.clone(),
        GraphValue::Integer(value) => value.to_string(),
        GraphValue::Decimal(value) => value.to_string(),
        GraphValue::Boolean(value) => value.to_string(),
        GraphValue::Time(value) => value.as_i64().to_string(),
        GraphValue::Null => "null".to_owned(),
    };
    format!(
        "{subject} {} {} {}",
        assertion.predicate.as_str(),
        assertion.predicate.as_str().replace('_', " "),
        object
    )
}

fn embedding(value: &str) -> [f32; 8] {
    let value = normalize(value);
    [
        contains_any(&value, &["work", "employ", "job"]) as u8 as f32,
        contains_any(&value, &["own", "control", "ownership"]) as u8 as f32,
        contains_any(&value, &["ceo", "chief executive"]) as u8 as f32,
        contains_any(&value, &["supply", "supplier", "contract"]) as u8 as f32,
        contains_any(&value, &["cause", "impact", "event"]) as u8 as f32,
        contains_any(&value, &["memory", "remember", "preference", "agent"]) as u8 as f32,
        contains_any(&value, &["2024", "2023", "historical"]) as u8 as f32,
        1.0,
    ]
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    let dot_product = left
        .iter()
        .zip(right.iter())
        .map(|(left, right)| left * right)
        .sum::<f32>();
    let magnitude = squared_magnitude(left).sqrt() * squared_magnitude(right).sqrt();
    if magnitude == 0.0 {
        0.0
    } else {
        dot_product / magnitude
    }
}

fn squared_magnitude(values: &[f32]) -> f32 {
    values.iter().map(|value| value * value).sum()
}

fn generated_at(storage: &InMemoryStorage) -> TxTime {
    TxTime::new(storage.events().len() as i64)
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['_', '-'], " ")
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

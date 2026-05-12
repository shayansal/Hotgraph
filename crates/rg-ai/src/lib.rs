//! AI-facing integration points for graph enrichment.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};

use rg_core::{
    AgentId, AgentMemory, Assertion, AssertionId, ContentHash, Entity, EntityId, EntityType,
    EventId, GraphValue, MemoryId, MemoryStatus, MemoryType, Source, SourceId, SourceType, TxTime,
    ValidTime,
};
use rg_index::{Contradiction, TemporalIndex};
use rg_query::{GraphQuery, PathQuery, QueryEngine, QueryResult};
use rg_storage::InMemoryStorage;

pub trait EmbeddingProvider {
    fn embed_assertion(&self, assertion: &Assertion) -> Vec<f32>;
}

#[derive(Default)]
pub struct NullEmbeddingProvider;

impl EmbeddingProvider for NullEmbeddingProvider {
    fn embed_assertion(&self, _assertion: &Assertion) -> Vec<f32> {
        Vec::new()
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VectorId(String);

impl VectorId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for VectorId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for VectorId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmbeddingKind {
    SourceDocument,
    SourceChunk,
    EntityDescription,
    AssertionExplanation,
    EventDescription,
    AgentMemory,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VectorRecord {
    pub id: VectorId,
    pub kind: EmbeddingKind,
    pub embedding: Vec<f32>,
    pub source_id: Option<SourceId>,
    pub entity_id: Option<EntityId>,
    pub assertion_id: Option<AssertionId>,
    pub event_id: Option<EventId>,
    pub memory_id: Option<MemoryId>,
    pub text: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VectorSearchResult {
    pub id: VectorId,
    pub kind: EmbeddingKind,
    pub score: f32,
    pub source_id: Option<SourceId>,
    pub entity_id: Option<EntityId>,
    pub assertion_id: Option<AssertionId>,
    pub event_id: Option<EventId>,
    pub memory_id: Option<MemoryId>,
    pub text: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VectorIndexHealth {
    pub is_healthy: bool,
    pub stored_vectors: usize,
    pub dimension: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VectorIndexError {
    EmptyEmbedding,
    ZeroMagnitudeEmbedding,
    NonFiniteEmbedding { index: usize },
    DimensionMismatch { expected: usize, actual: usize },
    LimitMustBePositive,
}

impl fmt::Display for VectorIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEmbedding => formatter.write_str("embedding cannot be empty"),
            Self::ZeroMagnitudeEmbedding => {
                formatter.write_str("embedding must have non-zero magnitude")
            }
            Self::NonFiniteEmbedding { index } => {
                write!(formatter, "embedding value at index {index} is not finite")
            }
            Self::DimensionMismatch { expected, actual } => write!(
                formatter,
                "embedding dimension mismatch: expected {expected}, got {actual}"
            ),
            Self::LimitMustBePositive => formatter.write_str("search limit must be positive"),
        }
    }
}

impl std::error::Error for VectorIndexError {}

pub trait VectorIndex {
    fn upsert_embedding(&mut self, record: VectorRecord) -> Result<(), VectorIndexError>;

    fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<VectorSearchResult>, VectorIndexError>;

    fn delete(&mut self, id: &VectorId) -> Result<bool, VectorIndexError>;

    fn health_check(&self) -> Result<VectorIndexHealth, VectorIndexError>;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InMemoryVectorIndex {
    records: BTreeMap<VectorId, VectorRecord>,
    dimension: Option<usize>,
}

impl InMemoryVectorIndex {
    pub fn new() -> Self {
        Self::default()
    }

    fn validate_dimension(&self, actual: usize) -> Result<(), VectorIndexError> {
        if let Some(expected) = self.dimension {
            if expected != actual {
                return Err(VectorIndexError::DimensionMismatch { expected, actual });
            }
        }
        Ok(())
    }
}

impl VectorIndex for InMemoryVectorIndex {
    fn upsert_embedding(&mut self, record: VectorRecord) -> Result<(), VectorIndexError> {
        validate_embedding(&record.embedding)?;
        self.validate_dimension(record.embedding.len())?;
        self.dimension.get_or_insert(record.embedding.len());
        self.records.insert(record.id.clone(), record);
        Ok(())
    }

    fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<VectorSearchResult>, VectorIndexError> {
        if limit == 0 {
            return Err(VectorIndexError::LimitMustBePositive);
        }
        validate_embedding(query_embedding)?;
        self.validate_dimension(query_embedding.len())?;

        let mut hits = self
            .records
            .values()
            .map(|record| VectorSearchResult {
                id: record.id.clone(),
                kind: record.kind.clone(),
                score: cosine_similarity(query_embedding, &record.embedding),
                source_id: record.source_id.clone(),
                entity_id: record.entity_id.clone(),
                assertion_id: record.assertion_id.clone(),
                event_id: record.event_id.clone(),
                memory_id: record.memory_id.clone(),
                text: record.text.clone(),
            })
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.id.cmp(&right.id))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    fn delete(&mut self, id: &VectorId) -> Result<bool, VectorIndexError> {
        Ok(self.records.remove(id).is_some())
    }

    fn health_check(&self) -> Result<VectorIndexHealth, VectorIndexError> {
        Ok(VectorIndexHealth {
            is_healthy: true,
            stored_vectors: self.records.len(),
            dimension: self.dimension,
        })
    }
}

fn validate_embedding(embedding: &[f32]) -> Result<(), VectorIndexError> {
    if embedding.is_empty() {
        return Err(VectorIndexError::EmptyEmbedding);
    }
    for (index, value) in embedding.iter().enumerate() {
        if !value.is_finite() {
            return Err(VectorIndexError::NonFiniteEmbedding { index });
        }
    }
    if squared_magnitude(embedding) == 0.0 {
        return Err(VectorIndexError::ZeroMagnitudeEmbedding);
    }
    Ok(())
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    let dot_product = left
        .iter()
        .zip(right.iter())
        .map(|(left, right)| left * right)
        .sum::<f32>();
    dot_product / (squared_magnitude(left).sqrt() * squared_magnitude(right).sqrt())
}

fn squared_magnitude(values: &[f32]) -> f32 {
    values.iter().map(|value| value * value).sum()
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentMemoryQuery {
    pub agent_id: Option<AgentId>,
    pub memory_type: Option<MemoryType>,
    pub valid_at: Option<ValidTime>,
    pub related_entity: Option<EntityId>,
    pub semantic_query: Option<Vec<f32>>,
    pub include_superseded: bool,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentMemorySearchResult {
    pub memory: AgentMemory,
    pub semantic_score: Option<f32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentMemoryRetrievalError {
    SemanticIndexUnavailable,
    Vector(VectorIndexError),
}

impl fmt::Display for AgentMemoryRetrievalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SemanticIndexUnavailable => formatter.write_str("semantic index unavailable"),
            Self::Vector(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for AgentMemoryRetrievalError {}

impl From<VectorIndexError> for AgentMemoryRetrievalError {
    fn from(error: VectorIndexError) -> Self {
        Self::Vector(error)
    }
}

pub struct AgentMemoryRetriever<'a> {
    storage: &'a InMemoryStorage,
    vector_index: Option<&'a dyn VectorIndex>,
}

impl<'a> AgentMemoryRetriever<'a> {
    pub fn new(storage: &'a InMemoryStorage, vector_index: Option<&'a dyn VectorIndex>) -> Self {
        Self {
            storage,
            vector_index,
        }
    }

    pub fn search(
        &self,
        query: AgentMemoryQuery,
    ) -> Result<Vec<AgentMemorySearchResult>, AgentMemoryRetrievalError> {
        let candidates = self.filtered_memories(&query);
        let mut results = if let Some(semantic_query) = &query.semantic_query {
            let vector_index = self
                .vector_index
                .ok_or(AgentMemoryRetrievalError::SemanticIndexUnavailable)?;
            vector_index
                .search(semantic_query, usize::MAX)?
                .into_iter()
                .filter(|hit| hit.kind == EmbeddingKind::AgentMemory)
                .filter_map(|hit| {
                    let memory_id = hit.memory_id?;
                    let memory = candidates.get(&memory_id)?;
                    Some(AgentMemorySearchResult {
                        memory: memory.clone(),
                        semantic_score: Some(hit.score),
                    })
                })
                .collect::<Vec<_>>()
        } else {
            candidates
                .values()
                .cloned()
                .map(|memory| AgentMemorySearchResult {
                    memory,
                    semantic_score: None,
                })
                .collect::<Vec<_>>()
        };

        if query.semantic_query.is_none() {
            results.sort_by(|left, right| left.memory.id.cmp(&right.memory.id));
        }
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }
        Ok(results)
    }

    fn filtered_memories(&self, query: &AgentMemoryQuery) -> BTreeMap<MemoryId, AgentMemory> {
        self.storage
            .graph_state()
            .agent_memories
            .values()
            .filter(|memory| {
                query
                    .agent_id
                    .as_ref()
                    .map_or(true, |agent_id| &memory.agent_id == agent_id)
            })
            .filter(|memory| {
                query
                    .memory_type
                    .as_ref()
                    .map_or(true, |memory_type| &memory.memory_type == memory_type)
            })
            .filter(|memory| {
                query
                    .valid_at
                    .map_or(true, |valid_at| memory.valid_time.contains(valid_at))
            })
            .filter(|memory| {
                query.include_superseded
                    || matches!(
                        memory.status,
                        MemoryStatus::Active | MemoryStatus::Reinforced
                    )
            })
            .filter(|memory| {
                query.related_entity.as_ref().map_or(true, |entity_id| {
                    memory.related_entities.contains(entity_id)
                })
            })
            .map(|memory| (memory.id.clone(), memory.clone()))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvidencePackRequest {
    pub query: String,
    pub graph_query: GraphQuery,
    pub path_query: Option<PathQuery>,
    pub generated_at: TxTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvidencePack {
    pub query: String,
    pub entities: Vec<Entity>,
    pub assertions: Vec<Assertion>,
    pub sources: Vec<SourceExcerpt>,
    pub paths: Vec<GraphPath>,
    pub contradictions: Vec<Contradiction>,
    pub generated_at: TxTime,
}

impl EvidencePack {
    pub fn to_golden_string(&self) -> String {
        let mut output = String::new();
        writeln!(&mut output, "query: {}", self.query).expect("write to string");
        writeln!(&mut output, "generated_at: {}", self.generated_at.as_i64())
            .expect("write to string");
        writeln!(&mut output).expect("write to string");

        writeln!(&mut output, "entities:").expect("write to string");
        for entity in &self.entities {
            writeln!(
                &mut output,
                "- {} | {} | {}",
                entity.id,
                entity_type_name(&entity.entity_type),
                entity.canonical_name.as_deref().unwrap_or("")
            )
            .expect("write to string");
        }
        writeln!(&mut output).expect("write to string");

        writeln!(&mut output, "assertions:").expect("write to string");
        for assertion in &self.assertions {
            writeln!(
                &mut output,
                "- {} | {} | {} | {} | valid={} | tx={} | confidence={:.2} | sources={}",
                assertion.id,
                assertion.subject,
                assertion.predicate,
                graph_value_name(&assertion.object),
                valid_interval_name(assertion),
                transaction_interval_name(assertion),
                assertion.confidence.as_f32(),
                source_ids_name(&assertion.source_ids)
            )
            .expect("write to string");
        }
        writeln!(&mut output).expect("write to string");

        writeln!(&mut output, "sources:").expect("write to string");
        for source in &self.sources {
            writeln!(
                &mut output,
                "- {} | {} | uri={} | hash={} | trust={} | snippet={}",
                source.source_id,
                source_type_name(&source.source_type),
                source.uri.as_deref().unwrap_or("none"),
                source.content_hash,
                trust_score_name(source.trust_score),
                source.snippet
            )
            .expect("write to string");
        }
        writeln!(&mut output).expect("write to string");

        writeln!(&mut output, "paths:").expect("write to string");
        for path in &self.paths {
            writeln!(
                &mut output,
                "- {} -> {} | {}",
                path.start,
                path.end,
                path.hops
                    .iter()
                    .map(|hop| hop.assertion_id.as_str())
                    .collect::<Vec<_>>()
                    .join(" > ")
            )
            .expect("write to string");
        }
        writeln!(&mut output).expect("write to string");

        writeln!(&mut output, "contradictions:").expect("write to string");
        for contradiction in &self.contradictions {
            writeln!(
                &mut output,
                "- {} <-> {}",
                contradiction.assertion_a, contradiction.assertion_b
            )
            .expect("write to string");
        }
        output
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceExcerpt {
    pub source_id: SourceId,
    pub source_type: SourceType,
    pub uri: Option<String>,
    pub content_hash: ContentHash,
    pub snippet: String,
    pub trust_score: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphPath {
    pub start: EntityId,
    pub end: EntityId,
    pub hops: Vec<QueryResult>,
}

pub struct EvidencePackGenerator<'a> {
    storage: &'a InMemoryStorage,
}

impl<'a> EvidencePackGenerator<'a> {
    pub fn new(storage: &'a InMemoryStorage) -> Self {
        Self { storage }
    }

    pub fn generate(&self, request: EvidencePackRequest) -> EvidencePack {
        let engine = QueryEngine::from_storage(self.storage.clone());
        let graph_results = engine.execute_graph(request.graph_query);
        let path_results = request
            .path_query
            .map_or_else(Vec::new, |query| engine.execute_path(query));
        let paths = graph_paths(path_results);

        let mut assertion_ids = BTreeSet::new();
        for result in &graph_results {
            assertion_ids.insert(result.assertion_id.clone());
        }
        for path in &paths {
            for hop in &path.hops {
                assertion_ids.insert(hop.assertion_id.clone());
            }
        }

        let contradictions = relevant_contradictions(self.storage, &mut assertion_ids);
        let assertions = assertions_for_ids(self.storage, &assertion_ids);
        let entities = entities_for_assertions(self.storage, &assertions);
        let sources = sources_for_assertions(self.storage, &assertions);

        EvidencePack {
            query: request.query,
            entities,
            assertions,
            sources,
            paths,
            contradictions,
            generated_at: request.generated_at,
        }
    }

    pub fn generate_for_graph_query(
        &self,
        query: impl Into<String>,
        graph_query: GraphQuery,
        generated_at: TxTime,
    ) -> EvidencePack {
        self.generate(EvidencePackRequest {
            query: query.into(),
            graph_query,
            path_query: None,
            generated_at,
        })
    }
}

fn graph_paths(paths: Vec<rg_query::PathResult>) -> Vec<GraphPath> {
    paths
        .into_iter()
        .map(|path| GraphPath {
            start: path.start,
            end: path.end,
            hops: path.hops,
        })
        .collect()
}

fn relevant_contradictions(
    storage: &InMemoryStorage,
    assertion_ids: &mut BTreeSet<AssertionId>,
) -> Vec<Contradiction> {
    let mut index = TemporalIndex::new();
    for assertion in storage.graph_state().assertions.values() {
        index.insert_assertion(assertion.clone());
    }

    let mut contradictions = index.contradictions();
    contradictions.retain(|contradiction| {
        assertion_ids.contains(&contradiction.assertion_a)
            || assertion_ids.contains(&contradiction.assertion_b)
    });
    contradictions.sort_by(|left, right| {
        left.assertion_a
            .cmp(&right.assertion_a)
            .then_with(|| left.assertion_b.cmp(&right.assertion_b))
            .then_with(|| left.contradiction_type.cmp(&right.contradiction_type))
    });
    contradictions.dedup();
    for contradiction in &contradictions {
        assertion_ids.insert(contradiction.assertion_a.clone());
        assertion_ids.insert(contradiction.assertion_b.clone());
    }
    contradictions
}

fn assertions_for_ids(
    storage: &InMemoryStorage,
    assertion_ids: &BTreeSet<AssertionId>,
) -> Vec<Assertion> {
    assertion_ids
        .iter()
        .filter_map(|id| storage.assertion(id).cloned())
        .collect()
}

fn entities_for_assertions(storage: &InMemoryStorage, assertions: &[Assertion]) -> Vec<Entity> {
    let mut entity_ids = BTreeSet::new();
    for assertion in assertions {
        entity_ids.insert(assertion.subject.clone());
        if let GraphValue::Entity(entity_id) = &assertion.object {
            entity_ids.insert(entity_id.clone());
        }
    }
    entity_ids
        .iter()
        .filter_map(|id| storage.entity(id).cloned())
        .collect()
}

fn sources_for_assertions(
    storage: &InMemoryStorage,
    assertions: &[Assertion],
) -> Vec<SourceExcerpt> {
    let mut source_ids = BTreeSet::new();
    for assertion in assertions {
        for source_id in &assertion.source_ids {
            source_ids.insert(source_id.clone());
        }
    }
    source_ids
        .iter()
        .filter_map(|id| storage.source(id))
        .map(source_excerpt)
        .collect()
}

fn source_excerpt(source: &Source) -> SourceExcerpt {
    let snippet = source.uri.as_ref().map_or_else(
        || {
            format!(
                "Source {} with no URI, content {}",
                source.id, source.content_hash
            )
        },
        |uri| {
            format!(
                "Source {} from {}, content {}",
                source.id, uri, source.content_hash
            )
        },
    );

    SourceExcerpt {
        source_id: source.id.clone(),
        source_type: source.source_type.clone(),
        uri: source.uri.clone(),
        content_hash: source.content_hash.clone(),
        snippet,
        trust_score: source.trust_score,
    }
}

fn entity_type_name(entity_type: &EntityType) -> String {
    match entity_type {
        EntityType::Person => "Person".to_owned(),
        EntityType::Organization => "Organization".to_owned(),
        EntityType::Place => "Place".to_owned(),
        EntityType::Event => "Event".to_owned(),
        EntityType::Document => "Document".to_owned(),
        EntityType::Concept => "Concept".to_owned(),
        EntityType::Custom(value) => format!("Custom({value})"),
    }
}

fn source_type_name(source_type: &SourceType) -> String {
    match source_type {
        SourceType::Document => "Document".to_owned(),
        SourceType::WebPage => "WebPage".to_owned(),
        SourceType::DatabaseRecord => "DatabaseRecord".to_owned(),
        SourceType::ApiResponse => "ApiResponse".to_owned(),
        SourceType::HumanReport => "HumanReport".to_owned(),
        SourceType::SensorReading => "SensorReading".to_owned(),
        SourceType::Custom(value) => format!("Custom({value})"),
    }
}

fn graph_value_name(value: &GraphValue) -> String {
    match value {
        GraphValue::Entity(id) => format!("Entity({id})"),
        GraphValue::Text(value) => format!("Text({value})"),
        GraphValue::Integer(value) => format!("Integer({value})"),
        GraphValue::Decimal(value) => format!("Decimal({value})"),
        GraphValue::Boolean(value) => format!("Boolean({value})"),
        GraphValue::Time(value) => format!("Time({})", value.as_i64()),
        GraphValue::Null => "Null".to_owned(),
    }
}

fn valid_interval_name(assertion: &Assertion) -> String {
    interval_name(
        assertion.valid_time.start.as_i64(),
        assertion.valid_time.end.map(|end| end.as_i64()),
    )
}

fn transaction_interval_name(assertion: &Assertion) -> String {
    interval_name(
        assertion.transaction_time.start.as_i64(),
        assertion.transaction_time.end.map(|end| end.as_i64()),
    )
}

fn interval_name(start: i64, end: Option<i64>) -> String {
    match end {
        Some(end) => format!("{start}..{end}"),
        None => format!("{start}.."),
    }
}

fn source_ids_name(source_ids: &[SourceId]) -> String {
    source_ids
        .iter()
        .map(SourceId::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

fn trust_score_name(score: Option<f32>) -> String {
    score.map_or_else(|| "none".to_owned(), |value| format!("{value:.2}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_core::{
        AssertionId, AssertionStatus, Confidence, ContextScope, EntityId, EventId, GraphValue,
        PredicateId, SourceId, TimeInterval, TxTime, ValidTime,
    };
    use rg_events::{
        AddAssertion, AddSource, ContentHash, CreateEntity, EventLog, GraphCommand, SourceType,
    };
    use rg_query::{EntityPattern, GraphQuery, PathQuery, PredicatePattern};
    use rg_storage::InMemoryStorage;

    #[test]
    fn null_embedding_provider_returns_no_embedding() {
        let assertion = Assertion {
            id: AssertionId::new("assertion-1"),
            subject: EntityId::new("person-a"),
            predicate: PredicateId::new("works_at"),
            object: GraphValue::Entity(EntityId::new("company-b")),
            valid_time: TimeInterval::new(ValidTime::new(10), None).expect("valid interval"),
            transaction_time: TimeInterval::new(TxTime::new(20), None).expect("valid interval"),
            confidence: Confidence::new(0.9).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-1")],
            context: ContextScope::Global,
            status: AssertionStatus::Active,
        };

        assert!(NullEmbeddingProvider.embed_assertion(&assertion).is_empty());
    }

    #[test]
    fn evidence_pack_contains_provenance_paths_and_contradictions() {
        let storage = evidence_fixture();
        let generator = EvidencePackGenerator::new(&storage);

        let pack = generator.generate(EvidencePackRequest {
            query: "Where did Person A work in 2024?".to_owned(),
            graph_query: GraphQuery {
                subject: Some(EntityPattern::Id(EntityId::new("person-a"))),
                predicate: Some(PredicatePattern::Id(PredicateId::new("worked_at"))),
                object: None,
                valid_at: Some(2024),
                known_at: Some(9),
                context: Some(ContextScope::Named("world".to_owned())),
                min_confidence: Some(0.8),
                limit: None,
            },
            path_query: Some(PathQuery {
                start: EntityId::new("person-a"),
                end: Some(EntityId::new("city-c")),
                predicates: vec![
                    PredicateId::new("worked_at"),
                    PredicateId::new("located_in"),
                ],
                valid_at: Some(2024),
                max_depth: 2,
                min_confidence: Some(0.8),
            }),
            generated_at: TxTime::new(99),
        });

        assert_eq!(pack.entities.len(), 4);
        assert_eq!(pack.assertions.len(), 3);
        assert_eq!(pack.sources.len(), 2);
        assert_eq!(pack.paths.len(), 1);
        assert_eq!(pack.contradictions.len(), 1);
        assert_eq!(
            pack.to_golden_string(),
            include_str!("../../../tests/golden/evidence_pack_basic.txt")
        );
    }

    #[test]
    fn graph_query_pack_generation_is_supported_without_path_search() {
        let storage = evidence_fixture();
        let generator = EvidencePackGenerator::new(&storage);

        let pack = generator.generate_for_graph_query(
            "current employment",
            GraphQuery {
                subject: Some(EntityPattern::Id(EntityId::new("person-a"))),
                predicate: Some(PredicatePattern::Id(PredicateId::new("worked_at"))),
                object: None,
                valid_at: Some(2024),
                known_at: Some(9),
                context: Some(ContextScope::Named("world".to_owned())),
                min_confidence: Some(0.8),
                limit: Some(1),
            },
            TxTime::new(100),
        );

        assert_eq!(pack.query, "current employment");
        assert_eq!(pack.paths, Vec::<GraphPath>::new());
        assert_eq!(pack.assertions.len(), 2);
        assert_eq!(pack.contradictions.len(), 1);
    }

    #[test]
    fn in_memory_vector_index_returns_ranked_candidate_hits_with_graph_links() {
        let mut index = InMemoryVectorIndex::new();
        index
            .upsert_embedding(VectorRecord {
                id: VectorId::new("vector-source-a"),
                kind: EmbeddingKind::SourceChunk,
                embedding: vec![1.0, 0.0],
                source_id: Some(SourceId::new("source-employment")),
                entity_id: Some(EntityId::new("person-a")),
                assertion_id: Some(AssertionId::new("assertion-worked-at")),
                event_id: None,
                memory_id: None,
                text: Some("Person A worked at Company B.".to_owned()),
            })
            .expect("embedding upsert");
        index
            .upsert_embedding(VectorRecord {
                id: VectorId::new("vector-entity-b"),
                kind: EmbeddingKind::EntityDescription,
                embedding: vec![0.8, 0.2],
                source_id: None,
                entity_id: Some(EntityId::new("company-b")),
                assertion_id: None,
                event_id: None,
                memory_id: None,
                text: Some("Company B is an organization.".to_owned()),
            })
            .expect("embedding upsert");
        index
            .upsert_embedding(VectorRecord {
                id: VectorId::new("vector-event-c"),
                kind: EmbeddingKind::EventDescription,
                embedding: vec![0.0, 1.0],
                source_id: Some(SourceId::new("source-conflict")),
                entity_id: None,
                assertion_id: None,
                event_id: Some(EventId::new("event-conflict")),
                memory_id: None,
                text: Some("A conflicting report was observed.".to_owned()),
            })
            .expect("embedding upsert");

        let hits = index.search(&[1.0, 0.0], 2).expect("search succeeds");

        assert_eq!(
            hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(),
            vec!["vector-source-a", "vector-entity-b"]
        );
        assert_eq!(hits[0].kind, EmbeddingKind::SourceChunk);
        assert_eq!(hits[0].source_id, Some(SourceId::new("source-employment")));
        assert_eq!(
            hits[0].assertion_id,
            Some(AssertionId::new("assertion-worked-at"))
        );
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn vector_index_rejects_invalid_embeddings_and_dimension_mismatches() {
        let mut index = InMemoryVectorIndex::new();

        let empty = index.upsert_embedding(vector_record("empty", Vec::new()));
        assert_eq!(empty, Err(VectorIndexError::EmptyEmbedding));

        let non_finite = index.upsert_embedding(vector_record("nan", vec![1.0, f32::NAN]));
        assert_eq!(
            non_finite,
            Err(VectorIndexError::NonFiniteEmbedding { index: 1 })
        );

        let zero = index.upsert_embedding(vector_record("zero", vec![0.0, 0.0]));
        assert_eq!(zero, Err(VectorIndexError::ZeroMagnitudeEmbedding));

        index
            .upsert_embedding(vector_record("valid", vec![1.0, 0.0]))
            .expect("valid embedding");

        let wrong_dimension = index.upsert_embedding(vector_record("wrong", vec![1.0, 0.0, 0.0]));
        assert_eq!(
            wrong_dimension,
            Err(VectorIndexError::DimensionMismatch {
                expected: 2,
                actual: 3
            })
        );

        let wrong_query = index.search(&[1.0, 0.0, 0.0], 1);
        assert_eq!(
            wrong_query,
            Err(VectorIndexError::DimensionMismatch {
                expected: 2,
                actual: 3
            })
        );

        let zero_limit = index.search(&[1.0, 0.0], 0);
        assert_eq!(zero_limit, Err(VectorIndexError::LimitMustBePositive));
    }

    #[test]
    fn vector_index_delete_and_health_check_report_index_state() {
        let mut index = InMemoryVectorIndex::new();

        assert_eq!(
            index.health_check().expect("health check"),
            VectorIndexHealth {
                is_healthy: true,
                stored_vectors: 0,
                dimension: None
            }
        );

        index
            .upsert_embedding(vector_record("memory", vec![0.2, 0.8]))
            .expect("embedding upsert");
        assert_eq!(
            index.health_check().expect("health check"),
            VectorIndexHealth {
                is_healthy: true,
                stored_vectors: 1,
                dimension: Some(2)
            }
        );

        assert!(index
            .delete(&VectorId::new("memory"))
            .expect("delete succeeds"));
        assert!(!index
            .delete(&VectorId::new("memory"))
            .expect("delete succeeds"));
        assert!(index.search(&[0.2, 0.8], 10).expect("search").is_empty());
        assert_eq!(
            index.health_check().expect("health check"),
            VectorIndexHealth {
                is_healthy: true,
                stored_vectors: 0,
                dimension: Some(2)
            }
        );
    }

    fn memory(
        id: &str,
        agent: &str,
        memory_type: rg_core::MemoryType,
        entity: &str,
        start: i64,
        status: rg_core::MemoryStatus,
    ) -> rg_core::AgentMemory {
        rg_core::AgentMemory {
            id: rg_core::MemoryId::new(id),
            agent_id: rg_core::AgentId::new(agent),
            memory_type,
            content: format!("{id} content"),
            valid_time: rg_core::TimeInterval::new(ValidTime::new(start), None)
                .expect("valid interval"),
            confidence: Confidence::new(0.8).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-memory")],
            related_entities: vec![EntityId::new(entity)],
            supersedes: Vec::new(),
            status,
        }
    }

    fn memory_fixture() -> InMemoryStorage {
        let mut log = EventLog::new(TxTime::new(0));
        log.execute(GraphCommand::AddSource(AddSource {
            id: SourceId::new("source-memory"),
            source_type: SourceType::Document,
            uri: Some("file://memory.md".to_owned()),
            content_hash: ContentHash::new("sha256:memory"),
            trust_score: Some(0.9),
        }))
        .expect("source added");
        for entity_id in ["person-a", "company-b"] {
            log.execute(GraphCommand::CreateEntity(CreateEntity {
                id: EntityId::new(entity_id),
                entity_type: EntityType::Person,
                canonical_name: Some(entity_id.to_owned()),
                properties: rg_core::PropertyMap::default(),
            }))
            .expect("entity added");
        }
        for memory in [
            memory(
                "memory-observation",
                "agent-1",
                rg_core::MemoryType::Observation,
                "person-a",
                10,
                rg_core::MemoryStatus::Active,
            ),
            memory(
                "memory-plan",
                "agent-1",
                rg_core::MemoryType::Plan,
                "company-b",
                20,
                rg_core::MemoryStatus::Active,
            ),
            memory(
                "memory-other-agent",
                "agent-2",
                rg_core::MemoryType::Preference,
                "person-a",
                10,
                rg_core::MemoryStatus::Active,
            ),
            memory(
                "memory-superseded",
                "agent-1",
                rg_core::MemoryType::Reflection,
                "person-a",
                10,
                rg_core::MemoryStatus::Active,
            ),
        ] {
            log.execute(GraphCommand::RecordAgentMemory(
                rg_events::RecordAgentMemory { memory },
            ))
            .expect("memory recorded");
        }
        let mut correction = memory(
            "memory-correction",
            "agent-1",
            rg_core::MemoryType::Correction,
            "company-b",
            10,
            rg_core::MemoryStatus::Active,
        );
        correction.supersedes = vec![rg_core::MemoryId::new("memory-superseded")];
        log.execute(GraphCommand::RecordAgentMemory(
            rg_events::RecordAgentMemory { memory: correction },
        ))
        .expect("correction recorded");
        InMemoryStorage::replay(log.events()).expect("storage replay")
    }

    #[test]
    fn agent_memory_retrieval_filters_by_agent_time_and_entity() {
        let storage = memory_fixture();
        let retriever = AgentMemoryRetriever::new(&storage, None);

        let memories = retriever
            .search(AgentMemoryQuery {
                agent_id: Some(rg_core::AgentId::new("agent-1")),
                memory_type: None,
                valid_at: Some(ValidTime::new(15)),
                related_entity: Some(EntityId::new("person-a")),
                semantic_query: None,
                include_superseded: false,
                limit: None,
            })
            .expect("memory search succeeds");

        assert_eq!(
            memories
                .iter()
                .map(|result| result.memory.id.as_str())
                .collect::<Vec<_>>(),
            vec!["memory-observation"]
        );
    }

    #[test]
    fn agent_memory_retrieval_can_include_superseded_revisions() {
        let storage = memory_fixture();
        let retriever = AgentMemoryRetriever::new(&storage, None);

        let memories = retriever
            .search(AgentMemoryQuery {
                agent_id: Some(rg_core::AgentId::new("agent-1")),
                memory_type: None,
                valid_at: Some(ValidTime::new(15)),
                related_entity: Some(EntityId::new("person-a")),
                semantic_query: None,
                include_superseded: true,
                limit: None,
            })
            .expect("memory search succeeds");

        assert_eq!(
            memories
                .iter()
                .map(|result| result.memory.id.as_str())
                .collect::<Vec<_>>(),
            vec!["memory-observation", "memory-superseded"]
        );
    }

    #[test]
    fn agent_memory_retrieval_uses_semantic_similarity_candidates() {
        let storage = memory_fixture();
        let mut vectors = InMemoryVectorIndex::new();
        vectors
            .upsert_embedding(VectorRecord {
                id: VectorId::new("vector-observation"),
                kind: EmbeddingKind::AgentMemory,
                embedding: vec![1.0, 0.0],
                source_id: None,
                entity_id: Some(EntityId::new("person-a")),
                assertion_id: None,
                event_id: None,
                memory_id: Some(rg_core::MemoryId::new("memory-observation")),
                text: Some("observation memory".to_owned()),
            })
            .expect("embedding upsert");
        vectors
            .upsert_embedding(VectorRecord {
                id: VectorId::new("vector-plan"),
                kind: EmbeddingKind::AgentMemory,
                embedding: vec![0.0, 1.0],
                source_id: None,
                entity_id: Some(EntityId::new("company-b")),
                assertion_id: None,
                event_id: None,
                memory_id: Some(rg_core::MemoryId::new("memory-plan")),
                text: Some("plan memory".to_owned()),
            })
            .expect("embedding upsert");
        let retriever = AgentMemoryRetriever::new(&storage, Some(&vectors));

        let memories = retriever
            .search(AgentMemoryQuery {
                agent_id: Some(rg_core::AgentId::new("agent-1")),
                memory_type: None,
                valid_at: None,
                related_entity: None,
                semantic_query: Some(vec![0.0, 1.0]),
                include_superseded: false,
                limit: Some(1),
            })
            .expect("semantic memory search succeeds");

        assert_eq!(memories[0].memory.id.as_str(), "memory-plan");
        assert_eq!(memories[0].semantic_score, Some(1.0));
    }

    fn vector_record(id: &str, embedding: Vec<f32>) -> VectorRecord {
        VectorRecord {
            id: VectorId::new(id),
            kind: EmbeddingKind::AgentMemory,
            embedding,
            source_id: None,
            entity_id: None,
            assertion_id: None,
            event_id: None,
            memory_id: None,
            text: Some("candidate memory".to_owned()),
        }
    }

    fn evidence_fixture() -> InMemoryStorage {
        let mut log = EventLog::new(TxTime::new(0));
        log.execute(GraphCommand::AddSource(AddSource {
            id: SourceId::new("source-employment"),
            source_type: SourceType::Document,
            uri: Some("file://employment.md".to_owned()),
            content_hash: ContentHash::new("sha256:employment"),
            trust_score: Some(0.95),
        }))
        .expect("source added");
        log.execute(GraphCommand::AddSource(AddSource {
            id: SourceId::new("source-conflict"),
            source_type: SourceType::HumanReport,
            uri: Some("file://conflict.md".to_owned()),
            content_hash: ContentHash::new("sha256:conflict"),
            trust_score: Some(0.7),
        }))
        .expect("source added");
        for (id, entity_type, name) in [
            ("person-a", rg_core::EntityType::Person, "Person A"),
            ("company-b", rg_core::EntityType::Organization, "Company B"),
            ("company-x", rg_core::EntityType::Organization, "Company X"),
            ("city-c", rg_core::EntityType::Place, "City C"),
        ] {
            log.execute(GraphCommand::CreateEntity(CreateEntity {
                id: EntityId::new(id),
                entity_type,
                canonical_name: Some(name.to_owned()),
                properties: rg_core::PropertyMap::default(),
            }))
            .expect("entity created");
        }
        log.execute(GraphCommand::AddAssertion(AddAssertion {
            id: AssertionId::new("assertion-worked-at"),
            subject: EntityId::new("person-a"),
            predicate: PredicateId::new("worked_at"),
            object: GraphValue::Entity(EntityId::new("company-b")),
            valid_time: TimeInterval::new(ValidTime::new(2021), Some(ValidTime::new(2025)))
                .expect("valid interval"),
            confidence: Confidence::new(0.92).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-employment")],
            context: ContextScope::Named("world".to_owned()),
        }))
        .expect("assertion added");
        log.execute(GraphCommand::AddAssertion(AddAssertion {
            id: AssertionId::new("assertion-worked-at-conflict"),
            subject: EntityId::new("person-a"),
            predicate: PredicateId::new("worked_at"),
            object: GraphValue::Entity(EntityId::new("company-x")),
            valid_time: TimeInterval::new(ValidTime::new(2023), Some(ValidTime::new(2025)))
                .expect("valid interval"),
            confidence: Confidence::new(0.86).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-conflict")],
            context: ContextScope::Named("world".to_owned()),
        }))
        .expect("assertion added");
        log.execute(GraphCommand::AddAssertion(AddAssertion {
            id: AssertionId::new("assertion-located-in"),
            subject: EntityId::new("company-b"),
            predicate: PredicateId::new("located_in"),
            object: GraphValue::Entity(EntityId::new("city-c")),
            valid_time: TimeInterval::new(ValidTime::new(2020), None).expect("valid interval"),
            confidence: Confidence::new(0.88).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-employment")],
            context: ContextScope::Named("world".to_owned()),
        }))
        .expect("assertion added");

        InMemoryStorage::replay(log.events()).expect("storage replay")
    }
}

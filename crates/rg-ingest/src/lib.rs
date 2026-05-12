//! Ingestion pipeline interfaces.

use std::collections::BTreeMap;
use std::fmt;

use rg_events::{
    AddAssertion, AssertionId, Confidence, ContextScope, CreateEntity, EntityId, EntityType,
    GraphCommand, GraphEvent, GraphValue, PredicateId, PropertyMap, SourceId, TimeInterval,
    ValidTime,
};
use rg_storage::{GraphStore, StorageError};

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

string_newtype!(DocumentId);
string_newtype!(ReviewerId);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentInput {
    pub id: DocumentId,
    pub source_id: SourceId,
    pub uri: Option<String>,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentChunk {
    pub document_id: DocumentId,
    pub source_id: SourceId,
    pub uri: Option<String>,
    pub index: usize,
    pub text: String,
    pub byte_start: usize,
    pub byte_end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceExcerpt {
    pub source_id: SourceId,
    pub uri: Option<String>,
    pub text: String,
    pub byte_start: usize,
    pub byte_end: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateAssertion {
    pub subject_text: String,
    pub predicate_text: String,
    pub object_text: String,
    pub valid_time: Option<TimeInterval<ValidTime>>,
    pub confidence: Confidence,
    pub source_excerpt: SourceExcerpt,
    pub extraction_model: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExtractionBatch {
    pub document_id: DocumentId,
    pub chunks: Vec<DocumentChunk>,
    pub candidates: Vec<CandidateAssertion>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IngestError {
    MalformedCandidateLine {
        line: String,
    },
    InvalidValidTime {
        value: String,
    },
    InvalidConfidence {
        value: String,
    },
    EvidenceNotFound {
        evidence: String,
    },
    CandidateRequiresApproval {
        subject_text: String,
        predicate_text: String,
        object_text: String,
    },
    MissingValidTime {
        subject_text: String,
        predicate_text: String,
        object_text: String,
    },
    ExternalLlmDisabled {
        provider: String,
    },
}

impl fmt::Display for IngestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedCandidateLine { line } => {
                write!(formatter, "malformed candidate line: {line}")
            }
            Self::InvalidValidTime { value } => write!(formatter, "invalid valid time: {value}"),
            Self::InvalidConfidence { value } => write!(formatter, "invalid confidence: {value}"),
            Self::EvidenceNotFound { evidence } => {
                write!(formatter, "evidence text was not found in chunk: {evidence}")
            }
            Self::CandidateRequiresApproval {
                subject_text,
                predicate_text,
                object_text,
            } => write!(
                formatter,
                "candidate requires approval before commit: {subject_text} {predicate_text} {object_text}"
            ),
            Self::MissingValidTime {
                subject_text,
                predicate_text,
                object_text,
            } => write!(
                formatter,
                "approved candidate is missing valid time: {subject_text} {predicate_text} {object_text}"
            ),
            Self::ExternalLlmDisabled { provider } => {
                write!(formatter, "{provider} LLM extraction is disabled")
            }
        }
    }
}

impl std::error::Error for IngestError {}

pub trait DocumentChunker {
    fn chunk(&self, document: &DocumentInput) -> Result<Vec<DocumentChunk>, IngestError>;
}

pub trait CandidateExtractor {
    fn extract_candidates(
        &self,
        document: &DocumentInput,
        chunk: &DocumentChunk,
    ) -> Result<Vec<CandidateAssertion>, IngestError>;
}

pub struct IngestionPipeline<C, E> {
    chunker: C,
    extractor: E,
}

impl<C, E> IngestionPipeline<C, E>
where
    C: DocumentChunker,
    E: CandidateExtractor,
{
    pub fn new(chunker: C, extractor: E) -> Self {
        Self { chunker, extractor }
    }

    pub fn extract(&self, document: &DocumentInput) -> Result<ExtractionBatch, IngestError> {
        let chunks = self.chunker.chunk(document)?;
        let mut candidates = Vec::new();
        for chunk in &chunks {
            candidates.extend(self.extractor.extract_candidates(document, chunk)?);
        }
        Ok(ExtractionBatch {
            document_id: document.id.clone(),
            chunks,
            candidates,
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LineChunker;

impl LineChunker {
    pub fn new() -> Self {
        Self
    }
}

impl DocumentChunker for LineChunker {
    fn chunk(&self, document: &DocumentInput) -> Result<Vec<DocumentChunk>, IngestError> {
        let mut chunks = Vec::new();
        let mut offset = 0;
        for raw_line in document.content.split_inclusive('\n') {
            let line_start = offset;
            offset += raw_line.len();
            let line_without_newline = raw_line.trim_end_matches(['\r', '\n']);
            let trimmed = line_without_newline.trim();
            if trimmed.is_empty() {
                continue;
            }
            let leading_whitespace =
                line_without_newline.len() - line_without_newline.trim_start().len();
            let byte_start = line_start + leading_whitespace;
            chunks.push(DocumentChunk {
                document_id: document.id.clone(),
                source_id: document.source_id.clone(),
                uri: document.uri.clone(),
                index: chunks.len(),
                text: trimmed.to_owned(),
                byte_start,
                byte_end: byte_start + trimmed.len(),
            });
        }
        Ok(chunks)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeterministicFixtureExtractor {
    extraction_model: String,
}

impl DeterministicFixtureExtractor {
    pub fn new(extraction_model: impl Into<String>) -> Self {
        Self {
            extraction_model: extraction_model.into(),
        }
    }
}

impl CandidateExtractor for DeterministicFixtureExtractor {
    fn extract_candidates(
        &self,
        _document: &DocumentInput,
        chunk: &DocumentChunk,
    ) -> Result<Vec<CandidateAssertion>, IngestError> {
        let Some(line) = chunk.text.strip_prefix("candidate:") else {
            return Ok(Vec::new());
        };
        let parts = line.split('|').map(str::trim).collect::<Vec<_>>();
        if parts.len() != 6 {
            return Err(IngestError::MalformedCandidateLine {
                line: chunk.text.clone(),
            });
        }

        let valid_time = parse_valid_time(strip_field(parts[3], "valid=")?)?;
        let confidence = parse_confidence(strip_field(parts[4], "confidence=")?)?;
        let evidence = strip_field(parts[5], "evidence=")?;
        let relative_evidence_start =
            chunk
                .text
                .find(evidence)
                .ok_or_else(|| IngestError::EvidenceNotFound {
                    evidence: evidence.to_owned(),
                })?;
        let byte_start = chunk.byte_start + relative_evidence_start;

        Ok(vec![CandidateAssertion {
            subject_text: parts[0].to_owned(),
            predicate_text: parts[1].to_owned(),
            object_text: parts[2].to_owned(),
            valid_time: Some(valid_time),
            confidence,
            source_excerpt: SourceExcerpt {
                source_id: chunk.source_id.clone(),
                uri: chunk.uri.clone(),
                text: evidence.to_owned(),
                byte_start,
                byte_end: byte_start + evidence.len(),
            },
            extraction_model: self.extraction_model.clone(),
        }])
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewedCandidate {
    pub candidate: CandidateAssertion,
    pub status: ReviewStatus,
    pub reviewer_id: Option<ReviewerId>,
    pub note: Option<String>,
}

impl ReviewedCandidate {
    pub fn pending(candidate: CandidateAssertion) -> Self {
        Self {
            candidate,
            status: ReviewStatus::Pending,
            reviewer_id: None,
            note: None,
        }
    }

    pub fn approve(candidate: CandidateAssertion, reviewer_id: ReviewerId) -> Self {
        Self {
            candidate,
            status: ReviewStatus::Approved,
            reviewer_id: Some(reviewer_id),
            note: None,
        }
    }

    pub fn reject(
        candidate: CandidateAssertion,
        reviewer_id: ReviewerId,
        note: impl Into<String>,
    ) -> Self {
        Self {
            candidate,
            status: ReviewStatus::Rejected,
            reviewer_id: Some(reviewer_id),
            note: Some(note.into()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateCommitPlanner {
    context: ContextScope,
    entity_type: EntityType,
}

impl CandidateCommitPlanner {
    pub fn new(context: ContextScope) -> Self {
        Self {
            context,
            entity_type: EntityType::Concept,
        }
    }

    pub fn commands_for_reviewed(
        &self,
        reviewed: &[ReviewedCandidate],
    ) -> Result<Vec<GraphCommand>, IngestError> {
        let mut entities = BTreeMap::new();
        let mut assertions = Vec::new();

        for reviewed_candidate in reviewed {
            match reviewed_candidate.status {
                ReviewStatus::Pending => {
                    return Err(candidate_requires_approval(&reviewed_candidate.candidate));
                }
                ReviewStatus::Rejected => continue,
                ReviewStatus::Approved => {
                    let candidate = &reviewed_candidate.candidate;
                    let valid_time = candidate
                        .valid_time
                        .clone()
                        .ok_or_else(|| missing_valid_time(candidate))?;
                    let subject = entity_id_for_text(&candidate.subject_text);
                    let object = entity_id_for_text(&candidate.object_text);
                    entities
                        .entry(subject.clone())
                        .or_insert_with(|| CreateEntity {
                            id: subject.clone(),
                            entity_type: self.entity_type.clone(),
                            canonical_name: Some(candidate.subject_text.clone()),
                            properties: PropertyMap::default(),
                        });
                    entities
                        .entry(object.clone())
                        .or_insert_with(|| CreateEntity {
                            id: object.clone(),
                            entity_type: self.entity_type.clone(),
                            canonical_name: Some(candidate.object_text.clone()),
                            properties: PropertyMap::default(),
                        });
                    let predicate = PredicateId::new(slugify(&candidate.predicate_text));
                    assertions.push(GraphCommand::AddAssertion(AddAssertion {
                        id: assertion_id_for_candidate(candidate, &subject, &predicate, &object),
                        subject,
                        predicate,
                        object: GraphValue::Entity(object),
                        valid_time,
                        confidence: candidate.confidence,
                        source_ids: vec![candidate.source_excerpt.source_id.clone()],
                        context: self.context.clone(),
                    }));
                }
            }
        }

        let mut commands = entities
            .into_values()
            .map(GraphCommand::CreateEntity)
            .collect::<Vec<_>>();
        commands.extend(assertions);
        Ok(commands)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LlmExtractionRequest {
    pub document: DocumentInput,
    pub chunks: Vec<DocumentChunk>,
}

pub trait LlmProvider {
    fn extract_candidates(
        &self,
        request: &LlmExtractionRequest,
    ) -> Result<Vec<CandidateAssertion>, IngestError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenAiLlmProvider {
    model: String,
}

impl OpenAiLlmProvider {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

impl LlmProvider for OpenAiLlmProvider {
    fn extract_candidates(
        &self,
        _request: &LlmExtractionRequest,
    ) -> Result<Vec<CandidateAssertion>, IngestError> {
        Err(IngestError::ExternalLlmDisabled {
            provider: "openai".to_owned(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalModelLlmProvider {
    model: String,
}

impl LocalModelLlmProvider {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

impl LlmProvider for LocalModelLlmProvider {
    fn extract_candidates(
        &self,
        _request: &LlmExtractionRequest,
    ) -> Result<Vec<CandidateAssertion>, IngestError> {
        Err(IngestError::ExternalLlmDisabled {
            provider: "local".to_owned(),
        })
    }
}

pub fn ingest_event(store: &mut impl GraphStore, event: GraphEvent) -> Result<(), StorageError> {
    store.append(event)
}

fn strip_field<'a>(value: &'a str, prefix: &str) -> Result<&'a str, IngestError> {
    value
        .strip_prefix(prefix)
        .map(str::trim)
        .ok_or_else(|| IngestError::MalformedCandidateLine {
            line: value.to_owned(),
        })
}

fn parse_valid_time(value: &str) -> Result<TimeInterval<ValidTime>, IngestError> {
    let Some((start, end)) = value.split_once("..") else {
        return Err(IngestError::InvalidValidTime {
            value: value.to_owned(),
        });
    };
    let start = start
        .parse::<i64>()
        .map_err(|_| IngestError::InvalidValidTime {
            value: value.to_owned(),
        })?;
    let end = if end.is_empty() {
        None
    } else {
        Some(
            end.parse::<i64>()
                .map_err(|_| IngestError::InvalidValidTime {
                    value: value.to_owned(),
                })?,
        )
    };
    TimeInterval::new(ValidTime::new(start), end.map(ValidTime::new)).map_err(|_| {
        IngestError::InvalidValidTime {
            value: value.to_owned(),
        }
    })
}

fn parse_confidence(value: &str) -> Result<Confidence, IngestError> {
    let parsed = value
        .parse::<f32>()
        .map_err(|_| IngestError::InvalidConfidence {
            value: value.to_owned(),
        })?;
    Confidence::new(parsed).map_err(|_| IngestError::InvalidConfidence {
        value: value.to_owned(),
    })
}

fn candidate_requires_approval(candidate: &CandidateAssertion) -> IngestError {
    IngestError::CandidateRequiresApproval {
        subject_text: candidate.subject_text.clone(),
        predicate_text: candidate.predicate_text.clone(),
        object_text: candidate.object_text.clone(),
    }
}

fn missing_valid_time(candidate: &CandidateAssertion) -> IngestError {
    IngestError::MissingValidTime {
        subject_text: candidate.subject_text.clone(),
        predicate_text: candidate.predicate_text.clone(),
        object_text: candidate.object_text.clone(),
    }
}

fn entity_id_for_text(text: &str) -> EntityId {
    EntityId::new(format!("entity-{}", slugify(text)))
}

fn assertion_id_for_candidate(
    candidate: &CandidateAssertion,
    subject: &EntityId,
    predicate: &PredicateId,
    object: &EntityId,
) -> AssertionId {
    AssertionId::new(format!(
        "assertion-{}-{}-{}-{}",
        subject.as_str(),
        predicate.as_str(),
        object.as_str(),
        slugify(candidate.source_excerpt.source_id.as_str())
    ))
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            previous_separator = false;
        } else if character == '_' {
            slug.push('_');
            previous_separator = false;
        } else if !previous_separator {
            slug.push('-');
            previous_separator = true;
        }
    }
    let slug = slug.trim_matches('-').to_owned();
    if slug.is_empty() {
        "unknown".to_owned()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_events::{
        AddSource, AssertionId, Confidence, ContentHash, ContextScope, EntityId, EntityType,
        EventLog, GraphCommand, GraphValue, PredicateId, SourceId, SourceType, TimeInterval,
        TxTime, ValidTime,
    };
    use rg_storage::InMemoryStore;

    #[test]
    fn ingest_event_appends_to_store() {
        let mut log = EventLog::new(TxTime::new(0));
        let event = log
            .execute(GraphCommand::AddSource(AddSource {
                id: SourceId::new("source-1"),
                source_type: SourceType::Document,
                uri: None,
                content_hash: ContentHash::new("sha256:source"),
                trust_score: None,
            }))
            .expect("event created");
        let mut store = InMemoryStore::new();

        ingest_event(&mut store, event.clone()).expect("ingest succeeds");

        assert_eq!(store.events(), &[event]);
    }

    #[test]
    fn deterministic_pipeline_extracts_candidate_assertions_from_fixture_document() {
        let document = DocumentInput {
            id: DocumentId::new("doc-employment"),
            source_id: SourceId::new("source-employment"),
            uri: Some("file://tests/fixtures/messy_employment_document.txt".to_owned()),
            content: include_str!("../../../tests/fixtures/messy_employment_document.txt")
                .to_owned(),
        };
        let pipeline = IngestionPipeline::new(
            LineChunker::new(),
            DeterministicFixtureExtractor::new("fixture-extractor-v1"),
        );

        let batch = pipeline.extract(&document).expect("extract candidates");

        assert_eq!(batch.document_id, DocumentId::new("doc-employment"));
        assert_eq!(batch.candidates.len(), 2);
        assert_eq!(batch.candidates[0].subject_text, "Person A");
        assert_eq!(batch.candidates[0].predicate_text, "worked_at");
        assert_eq!(batch.candidates[0].object_text, "Company B");
        assert_eq!(
            batch.candidates[0].valid_time,
            Some(TimeInterval::new(ValidTime::new(2021), Some(ValidTime::new(2025))).unwrap())
        );
        assert_eq!(
            batch.candidates[0].confidence,
            Confidence::new(0.92).unwrap()
        );
        assert_eq!(
            batch.candidates[0].source_excerpt,
            SourceExcerpt {
                source_id: SourceId::new("source-employment"),
                uri: Some("file://tests/fixtures/messy_employment_document.txt".to_owned()),
                text: "Person A worked at Company B from 2021 through 2024.".to_owned(),
                byte_start: 233,
                byte_end: 285,
            }
        );
        assert_eq!(
            batch
                .candidates
                .iter()
                .map(|candidate| candidate.extraction_model.as_str())
                .collect::<Vec<_>>(),
            vec!["fixture-extractor-v1", "fixture-extractor-v1"]
        );
    }

    #[test]
    fn commit_planner_requires_review_before_graph_commands_are_created() {
        let candidate = candidate_assertion();
        let planner = CandidateCommitPlanner::new(ContextScope::Named("world".to_owned()));

        let pending = ReviewedCandidate::pending(candidate.clone());
        assert_eq!(
            planner.commands_for_reviewed(&[pending]),
            Err(IngestError::CandidateRequiresApproval {
                subject_text: "Person A".to_owned(),
                predicate_text: "worked_at".to_owned(),
                object_text: "Company B".to_owned()
            })
        );

        let rejected = ReviewedCandidate::reject(
            candidate.clone(),
            ReviewerId::new("agent-reviewer"),
            "source was stale",
        );
        assert!(planner
            .commands_for_reviewed(&[rejected])
            .expect("rejected candidates are skipped")
            .is_empty());

        let approved = ReviewedCandidate::approve(candidate, ReviewerId::new("agent-reviewer"));
        let commands = planner
            .commands_for_reviewed(&[approved])
            .expect("approved candidate creates commands");

        assert_eq!(
            commands,
            vec![
                GraphCommand::CreateEntity(rg_events::CreateEntity {
                    id: EntityId::new("entity-company-b"),
                    entity_type: EntityType::Concept,
                    canonical_name: Some("Company B".to_owned()),
                    properties: rg_events::PropertyMap::default(),
                }),
                GraphCommand::CreateEntity(rg_events::CreateEntity {
                    id: EntityId::new("entity-person-a"),
                    entity_type: EntityType::Concept,
                    canonical_name: Some("Person A".to_owned()),
                    properties: rg_events::PropertyMap::default(),
                }),
                GraphCommand::AddAssertion(rg_events::AddAssertion {
                    id: AssertionId::new(
                        "assertion-entity-person-a-worked_at-entity-company-b-source-employment"
                    ),
                    subject: EntityId::new("entity-person-a"),
                    predicate: PredicateId::new("worked_at"),
                    object: GraphValue::Entity(EntityId::new("entity-company-b")),
                    valid_time: TimeInterval::new(ValidTime::new(2021), Some(ValidTime::new(2025)))
                        .unwrap(),
                    confidence: Confidence::new(0.92).unwrap(),
                    source_ids: vec![SourceId::new("source-employment")],
                    context: ContextScope::Named("world".to_owned()),
                }),
            ]
        );
    }

    #[test]
    fn commit_planner_rejects_approved_candidates_without_valid_time() {
        let mut candidate = candidate_assertion();
        candidate.valid_time = None;
        let planner = CandidateCommitPlanner::new(ContextScope::Named("world".to_owned()));
        let approved = ReviewedCandidate::approve(candidate, ReviewerId::new("agent-reviewer"));

        assert_eq!(
            planner.commands_for_reviewed(&[approved]),
            Err(IngestError::MissingValidTime {
                subject_text: "Person A".to_owned(),
                predicate_text: "worked_at".to_owned(),
                object_text: "Company B".to_owned()
            })
        );
    }

    #[test]
    fn llm_provider_adapters_are_interfaces_that_do_not_call_models_yet() {
        let request = LlmExtractionRequest {
            document: DocumentInput {
                id: DocumentId::new("doc-llm"),
                source_id: SourceId::new("source-llm"),
                uri: None,
                content: "Person A worked at Company B.".to_owned(),
            },
            chunks: Vec::new(),
        };

        let openai = OpenAiLlmProvider::new("openai-placeholder-model");
        assert_eq!(
            openai.extract_candidates(&request),
            Err(IngestError::ExternalLlmDisabled {
                provider: "openai".to_owned()
            })
        );

        let local = LocalModelLlmProvider::new("local-placeholder-model");
        assert_eq!(
            local.extract_candidates(&request),
            Err(IngestError::ExternalLlmDisabled {
                provider: "local".to_owned()
            })
        );
    }

    fn candidate_assertion() -> CandidateAssertion {
        CandidateAssertion {
            subject_text: "Person A".to_owned(),
            predicate_text: "worked_at".to_owned(),
            object_text: "Company B".to_owned(),
            valid_time: Some(
                TimeInterval::new(ValidTime::new(2021), Some(ValidTime::new(2025))).unwrap(),
            ),
            confidence: Confidence::new(0.92).unwrap(),
            source_excerpt: SourceExcerpt {
                source_id: SourceId::new("source-employment"),
                uri: Some("file://employment.md".to_owned()),
                text: "Person A worked at Company B from 2021 through 2024.".to_owned(),
                byte_start: 10,
                byte_end: 70,
            },
            extraction_model: "fixture-extractor-v1".to_owned(),
        }
    }
}

//! Multimodal ingestion adapters for Reality Graph.

use std::collections::BTreeMap;
use std::fmt;

use rg_core::{
    CausalLinkId, Confidence, ContentHash, EntityType, EventId, Source, SourceId, SourceType,
    TimeInterval, TxTime, ValidTime,
};

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

string_newtype!(EpisodeId);
string_newtype!(CandidateId);
string_newtype!(EvidenceId);
string_newtype!(EmbeddingId);
string_newtype!(ReviewTaskId);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SourceModality {
    Text,
    Pdf,
    Csv,
    Json,
    Html,
    ImageMetadata,
    Transcript,
    CodeRepository,
    DatabaseSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceContent {
    Text(String),
    Bytes(Vec<u8>),
}

impl SourceContent {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Text(text) => text.as_bytes(),
            Self::Bytes(bytes) => bytes,
        }
    }

    fn to_lossy_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceInput {
    pub id: SourceId,
    pub modality: SourceModality,
    pub uri: Option<String>,
    pub observed_at: TxTime,
    pub trust_score: Option<f32>,
    pub content: SourceContent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceRecord {
    pub source: Source,
    pub modality: SourceModality,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Episode {
    pub id: EpisodeId,
    pub source_id: SourceId,
    pub modality: SourceModality,
    pub observed_at: TxTime,
    pub summary: String,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvidenceLocator {
    ByteRange { start: usize, end: usize },
    Row { row: usize },
    JsonPointer(String),
    HtmlSelector(String),
    RepositoryPath(String),
    DatabaseTable(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceSnippet {
    pub id: EvidenceId,
    pub source_id: SourceId,
    pub uri: Option<String>,
    pub text: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub locator: EvidenceLocator,
    pub content_hash: ContentHash,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateStatus {
    PendingReview,
    Approved,
    Rejected,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateEntity {
    pub id: CandidateId,
    pub source_id: SourceId,
    pub name: String,
    pub entity_type: Option<EntityType>,
    pub confidence: Confidence,
    pub evidence_id: EvidenceId,
    pub status: CandidateStatus,
    pub extraction_model: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateAssertion {
    pub id: CandidateId,
    pub source_id: SourceId,
    pub subject_text: String,
    pub predicate_text: String,
    pub object_text: String,
    pub valid_time: Option<TimeInterval<ValidTime>>,
    pub confidence: Confidence,
    pub evidence_id: EvidenceId,
    pub status: CandidateStatus,
    pub extraction_model: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateEvent {
    pub id: CandidateId,
    pub source_id: SourceId,
    pub event_text: String,
    pub valid_time: Option<ValidTime>,
    pub confidence: Confidence,
    pub evidence_id: EvidenceId,
    pub status: CandidateStatus,
    pub extraction_model: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateCausalLink {
    pub id: CandidateId,
    pub source_id: SourceId,
    pub cause_event_text: String,
    pub effect_event_text: String,
    pub mechanism: Option<String>,
    pub confidence: Confidence,
    pub evidence_id: EvidenceId,
    pub status: CandidateStatus,
    pub extraction_model: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmbeddingTarget {
    Source(SourceId),
    Evidence(EvidenceId),
    Candidate(CandidateId),
    Event(EventId),
    CausalLink(CausalLinkId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddingRecord {
    pub id: EmbeddingId,
    pub target: EmbeddingTarget,
    pub model: String,
    pub vector: Vec<f32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewTaskTarget {
    Source(SourceId),
    CandidateEntity(CandidateId),
    CandidateAssertion(CandidateId),
    CandidateEvent(CandidateId),
    CandidateCausalLink(CandidateId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewTaskStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewTask {
    pub id: ReviewTaskId,
    pub target: ReviewTaskTarget,
    pub status: ReviewTaskStatus,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MultimodalIngestBatch {
    pub source: SourceRecord,
    pub episode: Episode,
    pub candidate_entities: Vec<CandidateEntity>,
    pub candidate_assertions: Vec<CandidateAssertion>,
    pub candidate_events: Vec<CandidateEvent>,
    pub candidate_causal_links: Vec<CandidateCausalLink>,
    pub evidence_snippets: Vec<EvidenceSnippet>,
    pub embeddings: Vec<EmbeddingRecord>,
    pub review_tasks: Vec<ReviewTask>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultimodalIngestError {
    NoAdapterRegistered {
        modality: SourceModality,
    },
    AdapterMismatch {
        expected: SourceModality,
        actual: SourceModality,
    },
    MalformedDirective {
        line: String,
    },
    EvidenceNotFound {
        evidence: String,
    },
    InvalidConfidence {
        value: String,
    },
    InvalidValidTime {
        value: String,
    },
}

impl fmt::Display for MultimodalIngestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapterRegistered { modality } => {
                write!(formatter, "no adapter registered for {modality:?}")
            }
            Self::AdapterMismatch { expected, actual } => {
                write!(
                    formatter,
                    "adapter mismatch: expected {expected:?}, got {actual:?}"
                )
            }
            Self::MalformedDirective { line } => {
                write!(formatter, "malformed deterministic directive: {line}")
            }
            Self::EvidenceNotFound { evidence } => {
                write!(formatter, "evidence was not found in source: {evidence}")
            }
            Self::InvalidConfidence { value } => write!(formatter, "invalid confidence: {value}"),
            Self::InvalidValidTime { value } => write!(formatter, "invalid valid time: {value}"),
        }
    }
}

impl std::error::Error for MultimodalIngestError {}

pub trait SourceAdapter {
    fn modality(&self) -> SourceModality;

    fn ingest(&self, input: SourceInput) -> Result<MultimodalIngestBatch, MultimodalIngestError>;
}

#[derive(Default)]
pub struct AdapterRegistry {
    adapters: BTreeMap<SourceModality, Box<dyn SourceAdapter>>,
}

impl AdapterRegistry {
    pub fn new(adapters: Vec<Box<dyn SourceAdapter>>) -> Self {
        Self {
            adapters: adapters
                .into_iter()
                .map(|adapter| (adapter.modality(), adapter))
                .collect(),
        }
    }

    pub fn with_default_adapters() -> Self {
        Self::new(all_default_adapters())
    }

    pub fn ingest(
        &self,
        input: SourceInput,
    ) -> Result<MultimodalIngestBatch, MultimodalIngestError> {
        let Some(adapter) = self.adapters.get(&input.modality) else {
            return Err(MultimodalIngestError::NoAdapterRegistered {
                modality: input.modality,
            });
        };
        adapter.ingest(input)
    }
}

macro_rules! source_adapter {
    ($name:ident, $modality:expr) => {
        #[derive(Clone, Debug, Default, Eq, PartialEq)]
        pub struct $name;

        impl SourceAdapter for $name {
            fn modality(&self) -> SourceModality {
                $modality
            }

            fn ingest(
                &self,
                input: SourceInput,
            ) -> Result<MultimodalIngestBatch, MultimodalIngestError> {
                ingest_with_modality($modality, input)
            }
        }
    };
}

source_adapter!(TextSourceAdapter, SourceModality::Text);
source_adapter!(PdfSourceAdapter, SourceModality::Pdf);
source_adapter!(CsvSourceAdapter, SourceModality::Csv);
source_adapter!(JsonSourceAdapter, SourceModality::Json);
source_adapter!(HtmlSourceAdapter, SourceModality::Html);
source_adapter!(ImageMetadataSourceAdapter, SourceModality::ImageMetadata);
source_adapter!(TranscriptSourceAdapter, SourceModality::Transcript);
source_adapter!(CodeRepositorySourceAdapter, SourceModality::CodeRepository);
source_adapter!(
    DatabaseSnapshotSourceAdapter,
    SourceModality::DatabaseSnapshot
);

pub fn all_default_adapters() -> Vec<Box<dyn SourceAdapter>> {
    vec![
        Box::new(TextSourceAdapter),
        Box::new(PdfSourceAdapter),
        Box::new(CsvSourceAdapter),
        Box::new(JsonSourceAdapter),
        Box::new(HtmlSourceAdapter),
        Box::new(ImageMetadataSourceAdapter),
        Box::new(TranscriptSourceAdapter),
        Box::new(CodeRepositorySourceAdapter),
        Box::new(DatabaseSnapshotSourceAdapter),
    ]
}

pub fn content_hash_for_bytes(bytes: &[u8]) -> ContentHash {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    ContentHash::new(format!("fnv1a64:{hash:016x}"))
}

fn ingest_with_modality(
    modality: SourceModality,
    input: SourceInput,
) -> Result<MultimodalIngestBatch, MultimodalIngestError> {
    if input.modality != modality {
        return Err(MultimodalIngestError::AdapterMismatch {
            expected: modality,
            actual: input.modality,
        });
    }

    let text = input.content.to_lossy_text();
    let source_hash = content_hash_for_bytes(input.content.as_bytes());
    let source = SourceRecord {
        source: Source {
            id: input.id.clone(),
            source_type: source_type_for(modality),
            uri: input.uri.clone(),
            content_hash: source_hash.clone(),
            observed_at: input.observed_at,
            trust_score: input.trust_score,
        },
        modality,
    };
    let mut evidence_snippets = vec![full_source_evidence(&input, source_hash.clone(), &text)];
    let mut candidate_entities = Vec::new();
    let mut candidate_assertions = Vec::new();
    let mut candidate_events = Vec::new();
    let mut candidate_causal_links = Vec::new();

    extract_directives(
        &input,
        &text,
        &mut evidence_snippets,
        &mut candidate_entities,
        &mut candidate_assertions,
        &mut candidate_events,
        &mut candidate_causal_links,
    )?;
    extract_structured_candidates(
        &input,
        &text,
        &mut evidence_snippets,
        &mut candidate_entities,
        &mut candidate_assertions,
    )?;

    evidence_snippets.sort_by(|left, right| left.id.cmp(&right.id));
    evidence_snippets.dedup_by(|left, right| left.id == right.id);

    let evidence_ids = evidence_snippets
        .iter()
        .map(|snippet| snippet.id.clone())
        .collect::<Vec<_>>();
    let episode = Episode {
        id: EpisodeId::new(format!("episode-{}", input.id)),
        source_id: input.id.clone(),
        modality,
        observed_at: input.observed_at,
        summary: format!(
            "{modality:?} source observed with {} evidence snippets",
            evidence_ids.len()
        ),
        evidence_ids,
    };

    let embeddings = embeddings_for(&input.id, &evidence_snippets, &text);
    let review_tasks = review_tasks_for(
        &source.source.id,
        &candidate_entities,
        &candidate_assertions,
        &candidate_events,
        &candidate_causal_links,
    );

    Ok(MultimodalIngestBatch {
        source,
        episode,
        candidate_entities,
        candidate_assertions,
        candidate_events,
        candidate_causal_links,
        evidence_snippets,
        embeddings,
        review_tasks,
    })
}

fn extract_directives(
    input: &SourceInput,
    text: &str,
    evidence_snippets: &mut Vec<EvidenceSnippet>,
    candidate_entities: &mut Vec<CandidateEntity>,
    candidate_assertions: &mut Vec<CandidateAssertion>,
    candidate_events: &mut Vec<CandidateEvent>,
    candidate_causal_links: &mut Vec<CandidateCausalLink>,
) -> Result<(), MultimodalIngestError> {
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(body) = line.strip_prefix("entity:") {
            let parts = split_directive(body);
            if parts.is_empty() {
                return Err(MultimodalIngestError::MalformedDirective {
                    line: line.to_owned(),
                });
            }
            let evidence = required_field(&parts, "evidence", line)?;
            let evidence_id = add_exact_evidence(input, text, evidence, evidence_snippets)?;
            let name = parts[0].to_owned();
            candidate_entities.push(CandidateEntity {
                id: candidate_id("entity", &input.id, &[&name]),
                source_id: input.id.clone(),
                name,
                entity_type: optional_field(&parts, "type").map(parse_entity_type),
                confidence: confidence_field(&parts, 0.6)?,
                evidence_id,
                status: CandidateStatus::PendingReview,
                extraction_model: deterministic_model(input.modality),
            });
        } else if let Some(body) = line.strip_prefix("assertion:") {
            let parts = split_directive(body);
            if parts.len() < 3 {
                return Err(MultimodalIngestError::MalformedDirective {
                    line: line.to_owned(),
                });
            }
            let evidence = required_field(&parts, "evidence", line)?;
            let evidence_id = add_exact_evidence(input, text, evidence, evidence_snippets)?;
            let subject = parts[0].to_owned();
            let predicate = parts[1].to_owned();
            let object = parts[2].to_owned();
            candidate_assertions.push(CandidateAssertion {
                id: candidate_id("assertion", &input.id, &[&subject, &predicate, &object]),
                source_id: input.id.clone(),
                subject_text: subject,
                predicate_text: predicate,
                object_text: object,
                valid_time: optional_field(&parts, "valid")
                    .map(parse_valid_time)
                    .transpose()?,
                confidence: confidence_field(&parts, 0.6)?,
                evidence_id,
                status: CandidateStatus::PendingReview,
                extraction_model: deterministic_model(input.modality),
            });
        } else if let Some(body) = line.strip_prefix("event:") {
            let parts = split_directive(body);
            if parts.is_empty() {
                return Err(MultimodalIngestError::MalformedDirective {
                    line: line.to_owned(),
                });
            }
            let evidence = required_field(&parts, "evidence", line)?;
            let evidence_id = add_exact_evidence(input, text, evidence, evidence_snippets)?;
            let event_text = parts[0].to_owned();
            candidate_events.push(CandidateEvent {
                id: candidate_id("event", &input.id, &[&event_text]),
                source_id: input.id.clone(),
                event_text,
                valid_time: optional_field(&parts, "time")
                    .map(parse_instant)
                    .transpose()?,
                confidence: confidence_field(&parts, 0.6)?,
                evidence_id,
                status: CandidateStatus::PendingReview,
                extraction_model: deterministic_model(input.modality),
            });
        } else if let Some(body) = line.strip_prefix("causal:") {
            let parts = split_directive(body);
            if parts.len() < 2 {
                return Err(MultimodalIngestError::MalformedDirective {
                    line: line.to_owned(),
                });
            }
            let evidence = required_field(&parts, "evidence", line)?;
            let evidence_id = add_exact_evidence(input, text, evidence, evidence_snippets)?;
            let cause = parts[0].to_owned();
            let effect = parts[1].to_owned();
            candidate_causal_links.push(CandidateCausalLink {
                id: candidate_id("causal", &input.id, &[&cause, &effect]),
                source_id: input.id.clone(),
                cause_event_text: cause,
                effect_event_text: effect,
                mechanism: optional_field(&parts, "mechanism").map(ToOwned::to_owned),
                confidence: confidence_field(&parts, 0.6)?,
                evidence_id,
                status: CandidateStatus::PendingReview,
                extraction_model: deterministic_model(input.modality),
            });
        }
    }
    Ok(())
}

fn extract_structured_candidates(
    input: &SourceInput,
    text: &str,
    evidence_snippets: &mut Vec<EvidenceSnippet>,
    candidate_entities: &mut Vec<CandidateEntity>,
    candidate_assertions: &mut Vec<CandidateAssertion>,
) -> Result<(), MultimodalIngestError> {
    match input.modality {
        SourceModality::Csv if candidate_assertions.is_empty() => {
            extract_csv_rows(input, text, evidence_snippets, candidate_assertions)?;
        }
        SourceModality::Json if candidate_entities.is_empty() => {
            extract_json_entity(input, text, evidence_snippets, candidate_entities)?;
        }
        SourceModality::Html if candidate_entities.is_empty() => {
            extract_html_title(input, text, evidence_snippets, candidate_entities)?;
        }
        _ => {}
    }
    Ok(())
}

fn extract_csv_rows(
    input: &SourceInput,
    text: &str,
    evidence_snippets: &mut Vec<EvidenceSnippet>,
    candidate_assertions: &mut Vec<CandidateAssertion>,
) -> Result<(), MultimodalIngestError> {
    for row in text.lines().skip(1) {
        let columns = row.split(',').map(str::trim).collect::<Vec<_>>();
        if columns.len() < 4 || columns.iter().any(|column| column.is_empty()) {
            continue;
        }
        let evidence_id = add_exact_evidence(input, text, columns[3], evidence_snippets)?;
        candidate_assertions.push(CandidateAssertion {
            id: candidate_id(
                "assertion",
                &input.id,
                &[columns[0], columns[1], columns[2]],
            ),
            source_id: input.id.clone(),
            subject_text: columns[0].to_owned(),
            predicate_text: columns[1].to_owned(),
            object_text: columns[2].to_owned(),
            valid_time: None,
            confidence: default_confidence(),
            evidence_id,
            status: CandidateStatus::PendingReview,
            extraction_model: deterministic_model(input.modality),
        });
    }
    Ok(())
}

fn extract_json_entity(
    input: &SourceInput,
    text: &str,
    evidence_snippets: &mut Vec<EvidenceSnippet>,
    candidate_entities: &mut Vec<CandidateEntity>,
) -> Result<(), MultimodalIngestError> {
    let Some(name) = json_string_field(text, "entity") else {
        return Ok(());
    };
    let evidence = json_string_field(text, "evidence").unwrap_or_else(|| name.clone());
    let evidence_id = add_exact_evidence(input, text, &evidence, evidence_snippets)?;
    candidate_entities.push(CandidateEntity {
        id: candidate_id("entity", &input.id, &[&name]),
        source_id: input.id.clone(),
        name,
        entity_type: json_string_field(text, "type").map(|value| parse_entity_type(&value)),
        confidence: default_confidence(),
        evidence_id,
        status: CandidateStatus::PendingReview,
        extraction_model: deterministic_model(input.modality),
    });
    Ok(())
}

fn extract_html_title(
    input: &SourceInput,
    text: &str,
    evidence_snippets: &mut Vec<EvidenceSnippet>,
    candidate_entities: &mut Vec<CandidateEntity>,
) -> Result<(), MultimodalIngestError> {
    let Some(title) = between(text, "<title>", "</title>") else {
        return Ok(());
    };
    let evidence_id = add_exact_evidence(input, text, &title, evidence_snippets)?;
    candidate_entities.push(CandidateEntity {
        id: candidate_id("entity", &input.id, &[&title]),
        source_id: input.id.clone(),
        name: title,
        entity_type: Some(EntityType::Document),
        confidence: default_confidence(),
        evidence_id,
        status: CandidateStatus::PendingReview,
        extraction_model: deterministic_model(input.modality),
    });
    Ok(())
}

fn full_source_evidence(
    input: &SourceInput,
    content_hash: ContentHash,
    text: &str,
) -> EvidenceSnippet {
    EvidenceSnippet {
        id: EvidenceId::new(format!("evidence-{}-full", input.id)),
        source_id: input.id.clone(),
        uri: input.uri.clone(),
        text: text.to_owned(),
        byte_start: 0,
        byte_end: input.content.as_bytes().len(),
        locator: EvidenceLocator::ByteRange {
            start: 0,
            end: input.content.as_bytes().len(),
        },
        content_hash,
    }
}

fn add_exact_evidence(
    input: &SourceInput,
    source_text: &str,
    evidence: &str,
    evidence_snippets: &mut Vec<EvidenceSnippet>,
) -> Result<EvidenceId, MultimodalIngestError> {
    let Some(byte_start) = source_text.find(evidence) else {
        return Err(MultimodalIngestError::EvidenceNotFound {
            evidence: evidence.to_owned(),
        });
    };
    let byte_end = byte_start + evidence.len();
    let id = EvidenceId::new(format!("evidence-{}-{byte_start}-{byte_end}", input.id));
    evidence_snippets.push(EvidenceSnippet {
        id: id.clone(),
        source_id: input.id.clone(),
        uri: input.uri.clone(),
        text: evidence.to_owned(),
        byte_start,
        byte_end,
        locator: EvidenceLocator::ByteRange {
            start: byte_start,
            end: byte_end,
        },
        content_hash: content_hash_for_bytes(evidence.as_bytes()),
    });
    Ok(id)
}

fn embeddings_for(
    source_id: &SourceId,
    evidence_snippets: &[EvidenceSnippet],
    text: &str,
) -> Vec<EmbeddingRecord> {
    let mut embeddings = vec![EmbeddingRecord {
        id: EmbeddingId::new(format!("embedding-source-{source_id}")),
        target: EmbeddingTarget::Source(source_id.clone()),
        model: "deterministic-hash-embedding-v1".to_owned(),
        vector: deterministic_embedding(text),
    }];
    embeddings.extend(evidence_snippets.iter().map(|snippet| EmbeddingRecord {
        id: EmbeddingId::new(format!("embedding-{}", snippet.id)),
        target: EmbeddingTarget::Evidence(snippet.id.clone()),
        model: "deterministic-hash-embedding-v1".to_owned(),
        vector: deterministic_embedding(&snippet.text),
    }));
    embeddings.sort_by(|left, right| left.id.cmp(&right.id));
    embeddings
}

fn review_tasks_for(
    source_id: &SourceId,
    candidate_entities: &[CandidateEntity],
    candidate_assertions: &[CandidateAssertion],
    candidate_events: &[CandidateEvent],
    candidate_causal_links: &[CandidateCausalLink],
) -> Vec<ReviewTask> {
    let mut tasks = vec![review_task(
        ReviewTaskTarget::Source(source_id.clone()),
        "Review source metadata, modality, and content hash before promoting extracted candidates.",
    )];
    tasks.extend(candidate_entities.iter().map(|candidate| {
        review_task(
            ReviewTaskTarget::CandidateEntity(candidate.id.clone()),
            "Review uncertain candidate entity before graph commit.",
        )
    }));
    tasks.extend(candidate_assertions.iter().map(|candidate| {
        review_task(
            ReviewTaskTarget::CandidateAssertion(candidate.id.clone()),
            "Review uncertain candidate assertion and evidence before graph commit.",
        )
    }));
    tasks.extend(candidate_events.iter().map(|candidate| {
        review_task(
            ReviewTaskTarget::CandidateEvent(candidate.id.clone()),
            "Review uncertain candidate event before graph commit.",
        )
    }));
    tasks.extend(candidate_causal_links.iter().map(|candidate| {
        review_task(
            ReviewTaskTarget::CandidateCausalLink(candidate.id.clone()),
            "Review uncertain candidate causal link before graph commit.",
        )
    }));
    tasks.sort_by(|left, right| left.id.cmp(&right.id));
    tasks
}

fn review_task(target: ReviewTaskTarget, reason: &str) -> ReviewTask {
    ReviewTask {
        id: ReviewTaskId::new(format!("review-{}", review_target_slug(&target))),
        target,
        status: ReviewTaskStatus::Pending,
        reason: reason.to_owned(),
    }
}

fn review_target_slug(target: &ReviewTaskTarget) -> String {
    match target {
        ReviewTaskTarget::Source(source_id) => format!("source-{source_id}"),
        ReviewTaskTarget::CandidateEntity(candidate_id) => format!("entity-{candidate_id}"),
        ReviewTaskTarget::CandidateAssertion(candidate_id) => format!("assertion-{candidate_id}"),
        ReviewTaskTarget::CandidateEvent(candidate_id) => format!("event-{candidate_id}"),
        ReviewTaskTarget::CandidateCausalLink(candidate_id) => format!("causal-{candidate_id}"),
    }
}

fn split_directive(body: &str) -> Vec<&str> {
    body.split('|').map(str::trim).collect()
}

fn required_field<'a>(
    parts: &'a [&str],
    key: &str,
    line: &str,
) -> Result<&'a str, MultimodalIngestError> {
    optional_field(parts, key).ok_or_else(|| MultimodalIngestError::MalformedDirective {
        line: line.to_owned(),
    })
}

fn optional_field<'a>(parts: &'a [&str], key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=");
    parts
        .iter()
        .find_map(|part| part.strip_prefix(&prefix).map(str::trim))
}

fn confidence_field(parts: &[&str], default: f32) -> Result<Confidence, MultimodalIngestError> {
    let Some(value) = optional_field(parts, "confidence") else {
        return Confidence::new(default).map_err(|_| MultimodalIngestError::InvalidConfidence {
            value: default.to_string(),
        });
    };
    let parsed = value
        .parse::<f32>()
        .map_err(|_| MultimodalIngestError::InvalidConfidence {
            value: value.to_owned(),
        })?;
    Confidence::new(parsed).map_err(|_| MultimodalIngestError::InvalidConfidence {
        value: value.to_owned(),
    })
}

fn default_confidence() -> Confidence {
    Confidence::new(0.6).expect("default confidence is valid")
}

fn parse_valid_time(value: &str) -> Result<TimeInterval<ValidTime>, MultimodalIngestError> {
    let Some((start, end)) = value.split_once("..") else {
        return Err(MultimodalIngestError::InvalidValidTime {
            value: value.to_owned(),
        });
    };
    let start = parse_i64(start, value)?;
    let end = if end.is_empty() {
        None
    } else {
        Some(ValidTime::new(parse_i64(end, value)?))
    };
    TimeInterval::new(ValidTime::new(start), end).map_err(|_| {
        MultimodalIngestError::InvalidValidTime {
            value: value.to_owned(),
        }
    })
}

fn parse_instant(value: &str) -> Result<ValidTime, MultimodalIngestError> {
    parse_i64(value, value).map(ValidTime::new)
}

fn parse_i64(value: &str, original: &str) -> Result<i64, MultimodalIngestError> {
    value
        .parse::<i64>()
        .map_err(|_| MultimodalIngestError::InvalidValidTime {
            value: original.to_owned(),
        })
}

fn parse_entity_type(value: &str) -> EntityType {
    match value {
        "Person" => EntityType::Person,
        "Organization" | "Company" => EntityType::Organization,
        "Place" => EntityType::Place,
        "Event" => EntityType::Event,
        "Document" => EntityType::Document,
        "Concept" => EntityType::Concept,
        other => EntityType::Custom(other.to_owned()),
    }
}

fn json_string_field(text: &str, field: &str) -> Option<String> {
    let pattern = format!("\"{field}\"");
    let start = text.find(&pattern)? + pattern.len();
    let after_colon = text[start..].find(':')? + start + 1;
    let after_open_quote = text[after_colon..].find('"')? + after_colon + 1;
    let close_quote = text[after_open_quote..].find('"')? + after_open_quote;
    Some(text[after_open_quote..close_quote].to_owned())
}

fn between(text: &str, start: &str, end: &str) -> Option<String> {
    let start_index = text.find(start)? + start.len();
    let end_index = text[start_index..].find(end)? + start_index;
    Some(text[start_index..end_index].trim().to_owned())
}

fn deterministic_embedding(text: &str) -> Vec<f32> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return vec![0.0, 0.0, 0.0, 0.0];
    }
    let mut buckets = [0.0_f32; 4];
    for (index, byte) in bytes.iter().enumerate() {
        buckets[index % 4] += f32::from(*byte) / 255.0;
    }
    let length = bytes.len() as f32;
    buckets.iter().map(|value| value / length).collect()
}

fn source_type_for(modality: SourceModality) -> SourceType {
    match modality {
        SourceModality::Text | SourceModality::Pdf => SourceType::Document,
        SourceModality::Csv | SourceModality::DatabaseSnapshot => SourceType::DatabaseRecord,
        SourceModality::Json => SourceType::ApiResponse,
        SourceModality::Html => SourceType::WebPage,
        SourceModality::ImageMetadata => SourceType::Custom("image_metadata".to_owned()),
        SourceModality::Transcript => SourceType::HumanReport,
        SourceModality::CodeRepository => SourceType::Custom("code_repository".to_owned()),
    }
}

fn deterministic_model(modality: SourceModality) -> String {
    format!("deterministic-{modality:?}-adapter-v1")
}

fn candidate_id(kind: &str, source_id: &SourceId, parts: &[&str]) -> CandidateId {
    CandidateId::new(format!(
        "candidate-{kind}-{source_id}-{}",
        slugify(&parts.join("-"))
    ))
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    slug.trim_matches('-').to_owned()
}

//! Zero-copy context serving primitives for Reality Graph.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use rg_ai::EvidencePack;
use rg_context_compression::ContextBudget;
use rg_core::{Assertion, AssertionId, SourceId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBuffer {
    source_id: SourceId,
    bytes: Arc<[u8]>,
}

impl SourceBuffer {
    pub fn new(source_id: SourceId, bytes: impl AsRef<[u8]>) -> Self {
        Self {
            source_id,
            bytes: Arc::<[u8]>::from(bytes.as_ref().to_vec()),
        }
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub fn bytes(&self) -> Arc<[u8]> {
        Arc::clone(&self.bytes)
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn slice(&self, range: Range<usize>) -> Result<ZeroCopySourceSlice, String> {
        ZeroCopySourceSlice::from_arc(self.source_id.clone(), self.bytes(), range)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZeroCopySourceSlice {
    source_id: SourceId,
    bytes: Arc<[u8]>,
    range: Range<usize>,
}

impl ZeroCopySourceSlice {
    pub fn from_arc(
        source_id: SourceId,
        bytes: Arc<[u8]>,
        range: Range<usize>,
    ) -> Result<Self, String> {
        if range.start > range.end {
            return Err("source slice start must be <= end".to_string());
        }
        if range.end > bytes.len() {
            return Err("source slice range is outside the source buffer".to_string());
        }

        Ok(Self {
            source_id,
            bytes,
            range,
        })
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[self.range.clone()]
    }

    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(self.as_bytes()).ok()
    }

    pub fn shares_backing_with(&self, source: &SourceBuffer) -> bool {
        Arc::ptr_eq(&self.bytes, &source.bytes)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextWireFormat {
    GrpcProtobufFrames,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextFrameKind {
    Header,
    Assertion,
    SourceSlice,
    Path,
    Contradiction,
    Footer,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextFrame {
    pub frame_id: String,
    pub kind: ContextFrameKind,
    pub estimated_tokens: usize,
    pub assertion_ids: Vec<AssertionId>,
    pub source_ids: Vec<SourceId>,
    pub payload: ContextFramePayload,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ContextFramePayload {
    Text(String),
    SourceSlice(ZeroCopySourceSlice),
    Summary {
        truncated: bool,
        total_estimated_tokens: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvidencePackStream {
    frames: Vec<ContextFrame>,
    wire_format: ContextWireFormat,
    total_estimated_tokens: usize,
    truncated: bool,
    telemetry_trace: OpenTelemetryTrace,
}

impl EvidencePackStream {
    pub fn frames(&self) -> &[ContextFrame] {
        &self.frames
    }

    pub fn wire_format(&self) -> ContextWireFormat {
        self.wire_format
    }

    pub fn total_estimated_tokens(&self) -> usize {
        self.total_estimated_tokens
    }

    pub fn was_truncated(&self) -> bool {
        self.truncated
    }

    pub fn telemetry_trace(&self) -> &OpenTelemetryTrace {
        &self.telemetry_trace
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatchContextRequest {
    pub batch_id: String,
    pub budget: ContextBudget,
    pub requests: Vec<ContextAssemblyRequest>,
}

impl BatchContextRequest {
    pub fn new(
        batch_id: impl Into<String>,
        budget: ContextBudget,
        requests: Vec<(EvidencePack, Vec<ZeroCopySourceSlice>)>,
    ) -> Self {
        Self {
            batch_id: batch_id.into(),
            budget,
            requests: requests
                .into_iter()
                .map(|(pack, source_slices)| ContextAssemblyRequest {
                    pack,
                    source_slices,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextAssemblyRequest {
    pub pack: EvidencePack,
    pub source_slices: Vec<ZeroCopySourceSlice>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreamingContextAssembler {
    budget: ContextBudget,
    recorder: OpenTelemetryStageRecorder,
}

impl StreamingContextAssembler {
    pub fn new(budget: ContextBudget, recorder: OpenTelemetryStageRecorder) -> Self {
        Self { budget, recorder }
    }

    pub fn assemble(
        &self,
        pack: EvidencePack,
        source_slices: Vec<ZeroCopySourceSlice>,
    ) -> EvidencePackStream {
        self.assemble_with_budget(pack, source_slices, &self.budget, None)
    }

    pub fn assemble_batch(&self, batch: BatchContextRequest) -> Vec<EvidencePackStream> {
        batch
            .requests
            .into_iter()
            .map(|request| {
                self.assemble_with_budget(
                    request.pack,
                    request.source_slices,
                    &batch.budget,
                    Some(batch.batch_id.as_str()),
                )
            })
            .collect()
    }

    fn assemble_with_budget(
        &self,
        pack: EvidencePack,
        source_slices: Vec<ZeroCopySourceSlice>,
        budget: &ContextBudget,
        batch_id: Option<&str>,
    ) -> EvidencePackStream {
        let trace = self.recorder.trace_for_pack(&pack, batch_id);
        let budget_tokens = budget.available_context_tokens();
        let mut total_estimated_tokens = 0;
        let mut truncated = false;
        let mut frames = Vec::new();

        frames.push(header_frame(&pack));
        for assertion in &pack.assertions {
            let frame = assertion_frame(assertion);
            if total_estimated_tokens + frame.estimated_tokens <= budget_tokens {
                total_estimated_tokens += frame.estimated_tokens;
                frames.push(frame);
            } else {
                truncated = true;
            }
        }

        for source_slice in source_slices {
            let estimated_tokens = estimate_tokens(source_slice.as_bytes());
            if total_estimated_tokens + estimated_tokens <= budget_tokens {
                total_estimated_tokens += estimated_tokens;
                frames.push(source_slice_frame(source_slice, estimated_tokens));
            } else {
                truncated = true;
            }
        }

        if !pack.paths.is_empty() {
            let frame = text_frame(
                format!("path_count={}", pack.paths.len()),
                ContextFrameKind::Path,
                "path-0",
            );
            if total_estimated_tokens + frame.estimated_tokens <= budget_tokens {
                total_estimated_tokens += frame.estimated_tokens;
                frames.push(frame);
            } else {
                truncated = true;
            }
        }

        if !pack.contradictions.is_empty() {
            let frame = text_frame(
                format!("contradiction_count={}", pack.contradictions.len()),
                ContextFrameKind::Contradiction,
                "contradiction-0",
            );
            if total_estimated_tokens + frame.estimated_tokens <= budget_tokens {
                total_estimated_tokens += frame.estimated_tokens;
                frames.push(frame);
            } else {
                truncated = true;
            }
        }

        frames.push(ContextFrame {
            frame_id: "footer".to_string(),
            kind: ContextFrameKind::Footer,
            estimated_tokens: 0,
            assertion_ids: Vec::new(),
            source_ids: Vec::new(),
            payload: ContextFramePayload::Summary {
                truncated,
                total_estimated_tokens,
            },
        });

        EvidencePackStream {
            frames,
            wire_format: ContextWireFormat::GrpcProtobufFrames,
            total_estimated_tokens,
            truncated,
            telemetry_trace: trace,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ContextServingStage {
    QueryPlanning,
    VectorRetrieval,
    GraphTraversal,
    TemporalFiltering,
    ContradictionDetection,
    Compression,
    Serialization,
}

impl ContextServingStage {
    pub fn all() -> Vec<Self> {
        vec![
            Self::QueryPlanning,
            Self::VectorRetrieval,
            Self::GraphTraversal,
            Self::TemporalFiltering,
            Self::ContradictionDetection,
            Self::Compression,
            Self::Serialization,
        ]
    }

    fn slug(self) -> &'static str {
        match self {
            Self::QueryPlanning => "query_planning",
            Self::VectorRetrieval => "vector_retrieval",
            Self::GraphTraversal => "graph_traversal",
            Self::TemporalFiltering => "temporal_filtering",
            Self::ContradictionDetection => "contradiction_detection",
            Self::Compression => "compression",
            Self::Serialization => "serialization",
        }
    }
}

impl fmt::Display for ContextServingStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.slug())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryStageRecorder {
    service_name: String,
}

impl OpenTelemetryStageRecorder {
    pub fn new() -> Self {
        Self {
            service_name: "reality_graph.context_serving".to_string(),
        }
    }

    pub fn trace_for_pack(
        &self,
        pack: &EvidencePack,
        batch_id: Option<&str>,
    ) -> OpenTelemetryTrace {
        let spans = ContextServingStage::all()
            .into_iter()
            .map(|stage| {
                let mut attributes = BTreeMap::new();
                attributes.insert("query".to_string(), pack.query.clone());
                attributes.insert(
                    "assertion_count".to_string(),
                    pack.assertions.len().to_string(),
                );
                if let Some(batch_id) = batch_id {
                    attributes.insert("batch_id".to_string(), batch_id.to_string());
                }
                OpenTelemetrySpan {
                    name: format!("{}.{}", self.service_name, stage.slug()),
                    stage,
                    attributes,
                }
            })
            .collect();
        OpenTelemetryTrace { spans }
    }
}

impl Default for OpenTelemetryStageRecorder {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenTelemetrySpan {
    pub name: String,
    pub stage: ContextServingStage,
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenTelemetryTrace {
    spans: Vec<OpenTelemetrySpan>,
}

impl OpenTelemetryTrace {
    pub fn spans(&self) -> &[OpenTelemetrySpan] {
        &self.spans
    }

    pub fn has_stage(&self, stage: ContextServingStage) -> bool {
        self.spans.iter().any(|span| span.stage == stage)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArrowDataType {
    Utf8,
    Int64,
    Float32,
    ListUtf8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrowField {
    pub name: String,
    pub data_type: ArrowDataType,
    pub nullable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrowColumn {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrowRecordBatch {
    pub name: String,
    pub fields: Vec<ArrowField>,
    pub columns: Vec<ArrowColumn>,
    pub row_count: usize,
}

impl ArrowRecordBatch {
    pub fn field(&self, name: &str) -> Option<&ArrowField> {
        self.fields.iter().find(|field| field.name == name)
    }

    pub fn column(&self, name: &str) -> Option<&ArrowColumn> {
        self.columns.iter().find(|column| column.name == name)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArrowEvidenceExporter;

impl ArrowEvidenceExporter {
    pub fn export(pack: &EvidencePack) -> ArrowRecordBatch {
        let assertions = &pack.assertions;
        ArrowRecordBatch {
            name: "reality_graph_evidence_pack".to_string(),
            fields: arrow_fields(),
            columns: arrow_columns(assertions),
            row_count: assertions.len(),
        }
    }
}

fn header_frame(pack: &EvidencePack) -> ContextFrame {
    ContextFrame {
        frame_id: "header".to_string(),
        kind: ContextFrameKind::Header,
        estimated_tokens: 0,
        assertion_ids: Vec::new(),
        source_ids: Vec::new(),
        payload: ContextFramePayload::Text(format!(
            "query={} generated_at={}",
            pack.query,
            pack.generated_at.as_i64()
        )),
    }
}

fn assertion_frame(assertion: &Assertion) -> ContextFrame {
    ContextFrame {
        frame_id: assertion.id.to_string(),
        kind: ContextFrameKind::Assertion,
        estimated_tokens: 1,
        assertion_ids: vec![assertion.id.clone()],
        source_ids: assertion.source_ids.clone(),
        payload: ContextFramePayload::Text(format!(
            "{} {} {:?}",
            assertion.subject, assertion.predicate, assertion.object
        )),
    }
}

fn source_slice_frame(source_slice: ZeroCopySourceSlice, estimated_tokens: usize) -> ContextFrame {
    ContextFrame {
        frame_id: format!("source-slice-{}", source_slice.source_id()),
        kind: ContextFrameKind::SourceSlice,
        estimated_tokens,
        assertion_ids: Vec::new(),
        source_ids: vec![source_slice.source_id().clone()],
        payload: ContextFramePayload::SourceSlice(source_slice),
    }
}

fn text_frame(text: String, kind: ContextFrameKind, frame_id: &str) -> ContextFrame {
    let estimated_tokens = text.split_whitespace().count();
    ContextFrame {
        frame_id: frame_id.to_string(),
        kind,
        estimated_tokens,
        assertion_ids: Vec::new(),
        source_ids: Vec::new(),
        payload: ContextFramePayload::Text(text),
    }
}

fn estimate_tokens(bytes: &[u8]) -> usize {
    std::str::from_utf8(bytes)
        .map(|text| text.split_whitespace().count())
        .unwrap_or_else(|_| bytes.len().div_ceil(4))
}

fn arrow_fields() -> Vec<ArrowField> {
    vec![
        ArrowField {
            name: "assertion_id".to_string(),
            data_type: ArrowDataType::Utf8,
            nullable: false,
        },
        ArrowField {
            name: "subject_id".to_string(),
            data_type: ArrowDataType::Utf8,
            nullable: false,
        },
        ArrowField {
            name: "predicate_id".to_string(),
            data_type: ArrowDataType::Utf8,
            nullable: false,
        },
        ArrowField {
            name: "object".to_string(),
            data_type: ArrowDataType::Utf8,
            nullable: false,
        },
        ArrowField {
            name: "valid_from".to_string(),
            data_type: ArrowDataType::Int64,
            nullable: false,
        },
        ArrowField {
            name: "valid_to".to_string(),
            data_type: ArrowDataType::Int64,
            nullable: true,
        },
        ArrowField {
            name: "tx_from".to_string(),
            data_type: ArrowDataType::Int64,
            nullable: false,
        },
        ArrowField {
            name: "tx_to".to_string(),
            data_type: ArrowDataType::Int64,
            nullable: true,
        },
        ArrowField {
            name: "confidence".to_string(),
            data_type: ArrowDataType::Float32,
            nullable: false,
        },
        ArrowField {
            name: "source_ids".to_string(),
            data_type: ArrowDataType::ListUtf8,
            nullable: false,
        },
    ]
}

fn arrow_columns(assertions: &[Assertion]) -> Vec<ArrowColumn> {
    let mut assertion_id = Vec::new();
    let mut subject_id = Vec::new();
    let mut predicate_id = Vec::new();
    let mut object = Vec::new();
    let mut valid_from = Vec::new();
    let mut valid_to = Vec::new();
    let mut tx_from = Vec::new();
    let mut tx_to = Vec::new();
    let mut confidence = Vec::new();
    let mut source_ids = Vec::new();

    for assertion in assertions {
        assertion_id.push(assertion.id.to_string());
        subject_id.push(assertion.subject.to_string());
        predicate_id.push(assertion.predicate.to_string());
        object.push(format!("{:?}", assertion.object));
        valid_from.push(assertion.valid_time.start.as_i64().to_string());
        valid_to.push(
            assertion
                .valid_time
                .end
                .map_or_else(String::new, |time| time.as_i64().to_string()),
        );
        tx_from.push(assertion.transaction_time.start.as_i64().to_string());
        tx_to.push(
            assertion
                .transaction_time
                .end
                .map_or_else(String::new, |time| time.as_i64().to_string()),
        );
        confidence.push(format!("{:.4}", assertion.confidence.as_f32()));
        source_ids.push(
            assertion
                .source_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }

    vec![
        ArrowColumn {
            name: "assertion_id".to_string(),
            values: assertion_id,
        },
        ArrowColumn {
            name: "subject_id".to_string(),
            values: subject_id,
        },
        ArrowColumn {
            name: "predicate_id".to_string(),
            values: predicate_id,
        },
        ArrowColumn {
            name: "object".to_string(),
            values: object,
        },
        ArrowColumn {
            name: "valid_from".to_string(),
            values: valid_from,
        },
        ArrowColumn {
            name: "valid_to".to_string(),
            values: valid_to,
        },
        ArrowColumn {
            name: "tx_from".to_string(),
            values: tx_from,
        },
        ArrowColumn {
            name: "tx_to".to_string(),
            values: tx_to,
        },
        ArrowColumn {
            name: "confidence".to_string(),
            values: confidence,
        },
        ArrowColumn {
            name: "source_ids".to_string(),
            values: source_ids,
        },
    ]
}

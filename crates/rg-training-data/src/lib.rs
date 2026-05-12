//! Graph Attention Memory training-data exporters for Reality Graph.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{json, Value};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TrainingExampleId(String);

impl TrainingExampleId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TrainingExampleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TrainingTaskKind {
    MemoryRetrieval,
    TemporalReasoning,
    GraphPathSupervision,
    EvidencePackSft,
    BeliefRevisionDpo,
    ToolUseTrace,
    AgentMemoryTrajectory,
}

impl TrainingTaskKind {
    pub fn slug(self) -> &'static str {
        match self {
            Self::MemoryRetrieval => "memory_retrieval",
            Self::TemporalReasoning => "temporal_reasoning",
            Self::GraphPathSupervision => "graph_path_supervision",
            Self::EvidencePackSft => "evidence_pack_sft",
            Self::BeliefRevisionDpo => "belief_revision_dpo",
            Self::ToolUseTrace => "tool_use_trace",
            Self::AgentMemoryTrajectory => "agent_memory_trajectory",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphStateSnapshot {
    pub entity_ids: Vec<String>,
    pub assertion_ids: Vec<String>,
    pub summary: String,
    pub valid_at: Option<i64>,
    pub known_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetrievedEvidence {
    pub evidence_id: String,
    pub text: String,
    pub source_id: String,
    pub assertion_ids: Vec<String>,
    pub score: f32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Citation {
    pub source_id: String,
    pub assertion_id: Option<String>,
    pub uri: Option<String>,
    pub quote: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemporalMetadata {
    pub valid_at: Option<i64>,
    pub known_at: Option<i64>,
    pub valid_window: Option<String>,
    pub transaction_time: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidencePath {
    pub path_id: String,
    pub nodes: Vec<String>,
    pub edges: Vec<String>,
    pub explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeliefRevisionTrace {
    pub previous_belief: String,
    pub revised_belief: String,
    pub reason: String,
    pub known_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolUseTrace {
    pub tool_name: String,
    pub input: String,
    pub output: String,
    pub succeeded: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryTrajectoryStep {
    pub memory_id: String,
    pub state: String,
    pub reason: String,
    pub transaction_time: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphAttentionExample {
    pub id: TrainingExampleId,
    pub task_kind: TrainingTaskKind,
    pub input_task: String,
    pub graph_state: GraphStateSnapshot,
    pub retrieved_evidence: Vec<RetrievedEvidence>,
    pub correct_answer: String,
    pub rejected_answer: Option<String>,
    pub citations: Vec<Citation>,
    pub temporal_metadata: TemporalMetadata,
    pub evidence_paths: Vec<EvidencePath>,
    pub belief_revisions: Vec<BeliefRevisionTrace>,
    pub tool_trace: Option<ToolUseTrace>,
    pub agent_memory_trajectory: Vec<MemoryTrajectoryStep>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphAttentionExampleDraft {
    pub id: TrainingExampleId,
    pub task_kind: TrainingTaskKind,
    pub input_task: String,
    pub graph_state: GraphStateSnapshot,
    pub retrieved_evidence: Vec<RetrievedEvidence>,
    pub correct_answer: String,
    pub citations: Vec<Citation>,
    pub temporal_metadata: TemporalMetadata,
}

impl GraphAttentionExample {
    pub fn new(draft: GraphAttentionExampleDraft) -> Self {
        Self {
            id: draft.id,
            task_kind: draft.task_kind,
            input_task: draft.input_task,
            graph_state: draft.graph_state,
            retrieved_evidence: draft.retrieved_evidence,
            correct_answer: draft.correct_answer,
            rejected_answer: None,
            citations: draft.citations,
            temporal_metadata: draft.temporal_metadata,
            evidence_paths: Vec::new(),
            belief_revisions: Vec::new(),
            tool_trace: None,
            agent_memory_trajectory: Vec::new(),
        }
    }

    pub fn with_rejected_answer(mut self, rejected_answer: impl Into<String>) -> Self {
        self.rejected_answer = Some(rejected_answer.into());
        self
    }

    pub fn with_evidence_path(mut self, evidence_path: EvidencePath) -> Self {
        self.evidence_paths.push(evidence_path);
        self
    }

    pub fn with_belief_revision(mut self, revision: BeliefRevisionTrace) -> Self {
        self.belief_revisions.push(revision);
        self
    }

    pub fn with_tool_trace(mut self, trace: ToolUseTrace) -> Self {
        self.tool_trace = Some(trace);
        self
    }

    pub fn with_memory_step(mut self, step: MemoryTrajectoryStep) -> Self {
        self.agent_memory_trajectory.push(step);
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.input_task.trim().is_empty() {
            return Err("training examples must include an input task".to_string());
        }
        if self.graph_state.summary.trim().is_empty()
            || self.graph_state.entity_ids.is_empty()
            || self.graph_state.assertion_ids.is_empty()
        {
            return Err("training examples must include graph state".to_string());
        }
        if self.retrieved_evidence.is_empty() {
            return Err("training examples must include retrieved evidence".to_string());
        }
        if self.correct_answer.trim().is_empty() {
            return Err("training examples must include a correct answer".to_string());
        }
        if self.citations.is_empty() {
            return Err("training examples must include citations".to_string());
        }
        if self.temporal_metadata.transaction_time < 0 {
            return Err("training examples must include temporal metadata".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportFormat {
    Jsonl,
    OpenAiMessagesJsonl,
    OpenAiDpoJsonl,
    Parquet,
    GraphPathSupervisionJsonl,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportArtifact {
    pub file_name: String,
    pub format: ExportFormat,
    pub bytes: Vec<u8>,
}

impl ExportArtifact {
    pub fn as_text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.bytes)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportBundle {
    pub artifacts: Vec<ExportArtifact>,
}

impl ExportBundle {
    pub fn validate_required_fields(&self) -> Result<(), String> {
        for artifact in &self.artifacts {
            if matches!(artifact.format, ExportFormat::Parquet) {
                continue;
            }
            let text = artifact
                .as_text()
                .map_err(|_| format!("{} is not utf8", artifact.file_name))?;
            for (line_index, line) in text.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let value: Value = serde_json::from_str(line).map_err(|error| {
                    format!(
                        "{} line {} is invalid json: {error}",
                        artifact.file_name,
                        line_index + 1
                    )
                })?;
                if artifact.format == ExportFormat::OpenAiDpoJsonl {
                    require_json_path(&value, &["input", "messages"], &artifact.file_name)?;
                    require_json_path(&value, &["preferred_output"], &artifact.file_name)?;
                    require_json_path(&value, &["non_preferred_output"], &artifact.file_name)?;
                    require_json_path(&value, &["metadata", "citations"], &artifact.file_name)?;
                    require_json_path(
                        &value,
                        &["metadata", "temporal_metadata"],
                        &artifact.file_name,
                    )?;
                } else if artifact.format == ExportFormat::OpenAiMessagesJsonl {
                    require_json_path(&value, &["messages"], &artifact.file_name)?;
                    require_json_path(&value, &["metadata", "citations"], &artifact.file_name)?;
                    require_json_path(
                        &value,
                        &["metadata", "temporal_metadata"],
                        &artifact.file_name,
                    )?;
                } else {
                    for field in REQUIRED_FIELDS {
                        require_json_path(&value, &[field], &artifact.file_name)?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrainingArrowField {
    pub name: String,
    pub data_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrainingArrowBatch {
    pub name: String,
    pub fields: Vec<TrainingArrowField>,
    pub rows: Vec<BTreeMap<String, String>>,
    pub row_count: usize,
}

impl TrainingArrowBatch {
    pub fn field(&self, name: &str) -> Option<&TrainingArrowField> {
        self.fields.iter().find(|field| field.name == name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HuggingFaceDataset {
    files: BTreeMap<String, String>,
}

impl HuggingFaceDataset {
    pub fn file(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(String::as_str)
    }
}

pub struct TrainingDataExporter;

impl TrainingDataExporter {
    pub fn memory_retrieval_examples_jsonl(
        examples: &[GraphAttentionExample],
    ) -> Result<ExportArtifact, String> {
        jsonl_artifact(
            "memory_retrieval_examples.jsonl",
            ExportFormat::Jsonl,
            examples,
            example_value,
        )
    }

    pub fn temporal_reasoning_examples_jsonl(
        examples: &[GraphAttentionExample],
    ) -> Result<ExportArtifact, String> {
        jsonl_artifact(
            "temporal_reasoning_examples.jsonl",
            ExportFormat::Jsonl,
            examples,
            example_value,
        )
    }

    pub fn graph_path_supervision_parquet(
        examples: &[GraphAttentionExample],
    ) -> Result<ExportArtifact, String> {
        validate_examples(examples)?;
        let value = json!({
            "schema": graph_path_schema(),
            "rows": examples.iter().map(graph_path_value).collect::<Vec<_>>(),
        });
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PAR1");
        bytes.extend_from_slice(
            serde_json::to_string(&value)
                .expect("graph path parquet metadata serializes")
                .as_bytes(),
        );
        bytes.extend_from_slice(b"PAR1");
        Ok(ExportArtifact {
            file_name: "graph_path_supervision.parquet".to_string(),
            format: ExportFormat::Parquet,
            bytes,
        })
    }

    pub fn graph_path_supervision_jsonl(
        examples: &[GraphAttentionExample],
    ) -> Result<ExportArtifact, String> {
        jsonl_artifact(
            "graph_path_supervision.jsonl",
            ExportFormat::GraphPathSupervisionJsonl,
            examples,
            graph_path_value,
        )
    }

    pub fn evidence_pack_sft_jsonl(
        examples: &[GraphAttentionExample],
    ) -> Result<ExportArtifact, String> {
        jsonl_artifact(
            "evidence_pack_sft.jsonl",
            ExportFormat::OpenAiMessagesJsonl,
            examples,
            openai_sft_value,
        )
    }

    pub fn belief_revision_dpo_pairs_jsonl(
        examples: &[GraphAttentionExample],
    ) -> Result<ExportArtifact, String> {
        jsonl_artifact(
            "belief_revision_dpo_pairs.jsonl",
            ExportFormat::OpenAiDpoJsonl,
            examples,
            openai_dpo_value,
        )
    }

    pub fn tool_trace_preference_pairs_jsonl(
        examples: &[GraphAttentionExample],
    ) -> Result<ExportArtifact, String> {
        jsonl_artifact(
            "tool_trace_preference_pairs.jsonl",
            ExportFormat::OpenAiDpoJsonl,
            examples,
            openai_tool_trace_dpo_value,
        )
    }

    pub fn arrow_batch(examples: &[GraphAttentionExample]) -> Result<TrainingArrowBatch, String> {
        validate_examples(examples)?;
        let fields = REQUIRED_FIELDS
            .iter()
            .map(|field| TrainingArrowField {
                name: (*field).to_string(),
                data_type: "utf8".to_string(),
            })
            .collect::<Vec<_>>();
        let rows = examples
            .iter()
            .map(|example| {
                let value = example_value(example);
                REQUIRED_FIELDS
                    .iter()
                    .map(|field| ((*field).to_string(), value[field].to_string()))
                    .collect::<BTreeMap<_, _>>()
            })
            .collect::<Vec<_>>();
        Ok(TrainingArrowBatch {
            name: "reality_graph_training_examples".to_string(),
            fields,
            row_count: rows.len(),
            rows,
        })
    }

    pub fn hugging_face_dataset(
        examples: &[GraphAttentionExample],
    ) -> Result<HuggingFaceDataset, String> {
        validate_examples(examples)?;
        let train = jsonl(examples, example_value)?;
        let dataset_info = json!({
            "dataset_name": "reality_graph_training_examples",
            "features": REQUIRED_FIELDS,
            "splits": { "train": { "num_examples": examples.len() } },
        });
        let mut files = BTreeMap::new();
        files.insert(
            "dataset_info.json".to_string(),
            serde_json::to_string(&dataset_info).expect("dataset info serializes"),
        );
        files.insert("train.jsonl".to_string(), train);
        files.insert(
            "README.md".to_string(),
            "# Reality Graph Training Data\n\nGraph Attention Memory supervision exported for model labs.\n"
                .to_string(),
        );
        Ok(HuggingFaceDataset { files })
    }

    pub fn all_named_exports(examples: &[GraphAttentionExample]) -> Result<ExportBundle, String> {
        Ok(ExportBundle {
            artifacts: vec![
                Self::memory_retrieval_examples_jsonl(examples)?,
                Self::temporal_reasoning_examples_jsonl(examples)?,
                Self::graph_path_supervision_parquet(examples)?,
                Self::evidence_pack_sft_jsonl(examples)?,
                Self::belief_revision_dpo_pairs_jsonl(examples)?,
                Self::tool_trace_preference_pairs_jsonl(examples)?,
            ],
        })
    }
}

const REQUIRED_FIELDS: &[&str] = &[
    "input_task",
    "graph_state",
    "retrieved_evidence",
    "correct_answer",
    "rejected_answer",
    "citations",
    "temporal_metadata",
];

fn validate_examples(examples: &[GraphAttentionExample]) -> Result<(), String> {
    if examples.is_empty() {
        return Err("training exports require at least one example".to_string());
    }
    for example in examples {
        example.validate()?;
    }
    Ok(())
}

fn jsonl_artifact(
    file_name: &str,
    format: ExportFormat,
    examples: &[GraphAttentionExample],
    row: fn(&GraphAttentionExample) -> Value,
) -> Result<ExportArtifact, String> {
    Ok(ExportArtifact {
        file_name: file_name.to_string(),
        format,
        bytes: jsonl(examples, row)?.into_bytes(),
    })
}

fn jsonl(
    examples: &[GraphAttentionExample],
    row: fn(&GraphAttentionExample) -> Value,
) -> Result<String, String> {
    validate_examples(examples)?;
    let mut output = String::new();
    for example in examples {
        output.push_str(&serde_json::to_string(&row(example)).map_err(|error| error.to_string())?);
        output.push('\n');
    }
    Ok(output)
}

fn example_value(example: &GraphAttentionExample) -> Value {
    json!({
        "id": example.id.as_str(),
        "task_kind": example.task_kind.slug(),
        "input_task": example.input_task,
        "graph_state": graph_state_value(&example.graph_state),
        "retrieved_evidence": example.retrieved_evidence.iter().map(evidence_value).collect::<Vec<_>>(),
        "correct_answer": example.correct_answer,
        "rejected_answer": example.rejected_answer,
        "citations": example.citations.iter().map(citation_value).collect::<Vec<_>>(),
        "temporal_metadata": temporal_metadata_value(&example.temporal_metadata),
        "evidence_paths": example.evidence_paths.iter().map(evidence_path_value).collect::<Vec<_>>(),
        "belief_revisions": example.belief_revisions.iter().map(belief_revision_value).collect::<Vec<_>>(),
        "tool_trace": example.tool_trace.as_ref().map(tool_trace_value),
        "agent_memory_trajectory": example.agent_memory_trajectory.iter().map(memory_step_value).collect::<Vec<_>>(),
    })
}

fn graph_path_value(example: &GraphAttentionExample) -> Value {
    let mut value = example_value(example);
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "supervision_format".to_string(),
            json!("reality_graph.graph_path_supervision.v1"),
        );
    }
    value
}

fn openai_sft_value(example: &GraphAttentionExample) -> Value {
    json!({
        "messages": [
            {
                "role": "developer",
                "content": "Answer using Reality Graph evidence. Preserve citations, temporal constraints, and contradiction warnings."
            },
            {
                "role": "user",
                "content": example.input_task
            },
            {
                "role": "assistant",
                "content": example.correct_answer
            }
        ],
        "metadata": metadata_value(example),
    })
}

fn openai_dpo_value(example: &GraphAttentionExample) -> Value {
    json!({
        "input": {
            "messages": [
                {
                    "role": "user",
                    "content": example.input_task
                }
            ],
            "tools": [],
            "parallel_tool_calls": true
        },
        "preferred_output": [
            {
                "role": "assistant",
                "content": example.correct_answer
            }
        ],
        "non_preferred_output": [
            {
                "role": "assistant",
                "content": example.rejected_answer.clone().unwrap_or_else(|| "No rejected answer was available for this example.".to_string())
            }
        ],
        "metadata": metadata_value(example),
    })
}

fn openai_tool_trace_dpo_value(example: &GraphAttentionExample) -> Value {
    let mut value = openai_dpo_value(example);
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "tool_trace".to_string(),
            example
                .tool_trace
                .as_ref()
                .map(tool_trace_value)
                .unwrap_or(Value::Null),
        );
    }
    value
}

fn metadata_value(example: &GraphAttentionExample) -> Value {
    json!({
        "example_id": example.id.as_str(),
        "task_kind": example.task_kind.slug(),
        "graph_state": graph_state_value(&example.graph_state),
        "retrieved_evidence": example.retrieved_evidence.iter().map(evidence_value).collect::<Vec<_>>(),
        "citations": example.citations.iter().map(citation_value).collect::<Vec<_>>(),
        "temporal_metadata": temporal_metadata_value(&example.temporal_metadata),
        "evidence_paths": example.evidence_paths.iter().map(evidence_path_value).collect::<Vec<_>>(),
    })
}

fn graph_state_value(state: &GraphStateSnapshot) -> Value {
    json!({
        "entity_ids": state.entity_ids,
        "assertion_ids": state.assertion_ids,
        "summary": state.summary,
        "valid_at": state.valid_at,
        "known_at": state.known_at,
    })
}

fn evidence_value(evidence: &RetrievedEvidence) -> Value {
    json!({
        "evidence_id": evidence.evidence_id,
        "text": evidence.text,
        "source_id": evidence.source_id,
        "assertion_ids": evidence.assertion_ids,
        "score": evidence.score,
    })
}

fn citation_value(citation: &Citation) -> Value {
    json!({
        "source_id": citation.source_id,
        "assertion_id": citation.assertion_id,
        "uri": citation.uri,
        "quote": citation.quote,
    })
}

fn temporal_metadata_value(metadata: &TemporalMetadata) -> Value {
    json!({
        "valid_at": metadata.valid_at,
        "known_at": metadata.known_at,
        "valid_window": metadata.valid_window,
        "transaction_time": metadata.transaction_time,
    })
}

fn evidence_path_value(path: &EvidencePath) -> Value {
    json!({
        "path_id": path.path_id,
        "nodes": path.nodes,
        "edges": path.edges,
        "explanation": path.explanation,
    })
}

fn belief_revision_value(revision: &BeliefRevisionTrace) -> Value {
    json!({
        "previous_belief": revision.previous_belief,
        "revised_belief": revision.revised_belief,
        "reason": revision.reason,
        "known_at": revision.known_at,
    })
}

fn tool_trace_value(trace: &ToolUseTrace) -> Value {
    json!({
        "tool_name": trace.tool_name,
        "input": trace.input,
        "output": trace.output,
        "succeeded": trace.succeeded,
    })
}

fn memory_step_value(step: &MemoryTrajectoryStep) -> Value {
    json!({
        "memory_id": step.memory_id,
        "state": step.state,
        "reason": step.reason,
        "transaction_time": step.transaction_time,
    })
}

fn graph_path_schema() -> Vec<Value> {
    vec![
        json!({"name": "input_task", "type": "utf8"}),
        json!({"name": "graph_state", "type": "json"}),
        json!({"name": "retrieved_evidence", "type": "json"}),
        json!({"name": "correct_answer", "type": "utf8"}),
        json!({"name": "rejected_answer", "type": "utf8"}),
        json!({"name": "citations", "type": "json"}),
        json!({"name": "temporal_metadata", "type": "json"}),
        json!({"name": "evidence_paths", "type": "json"}),
    ]
}

fn require_json_path(value: &Value, path: &[&str], artifact: &str) -> Result<(), String> {
    let mut current = value;
    for key in path {
        current = current
            .get(*key)
            .ok_or_else(|| format!("{artifact} missing {}", path.join(".")))?;
    }
    Ok(())
}

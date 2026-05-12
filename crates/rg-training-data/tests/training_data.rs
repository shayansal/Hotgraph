use serde_json::Value;

use rg_training_data::{
    Citation, EvidencePath, ExportFormat, GraphAttentionExample, GraphAttentionExampleDraft,
    GraphStateSnapshot, HuggingFaceDataset, RetrievedEvidence, TemporalMetadata,
    TrainingArrowBatch, TrainingDataExporter, TrainingExampleId, TrainingTaskKind,
};

#[test]
fn graph_attention_examples_validate_lab_grade_required_fields() {
    let example = fixture_example(TrainingTaskKind::MemoryRetrieval);

    assert_eq!(example.id.as_str(), "example-1");
    assert!(example.validate().is_ok());

    let mut invalid = example.clone();
    invalid.retrieved_evidence.clear();
    assert_eq!(
        invalid.validate().expect_err("missing evidence"),
        "training examples must include retrieved evidence"
    );

    let mut invalid = example;
    invalid.citations.clear();
    assert_eq!(
        invalid.validate().expect_err("missing citations"),
        "training examples must include citations"
    );
}

#[test]
fn jsonl_exporters_emit_required_files_and_fields() {
    let examples = vec![
        fixture_example(TrainingTaskKind::MemoryRetrieval),
        fixture_example(TrainingTaskKind::TemporalReasoning),
    ];

    let memory =
        TrainingDataExporter::memory_retrieval_examples_jsonl(&examples).expect("memory jsonl");
    let temporal =
        TrainingDataExporter::temporal_reasoning_examples_jsonl(&examples).expect("temporal jsonl");

    assert_eq!(memory.file_name, "memory_retrieval_examples.jsonl");
    assert_eq!(temporal.file_name, "temporal_reasoning_examples.jsonl");
    assert_eq!(memory.format, ExportFormat::Jsonl);

    let parsed = parse_first_jsonl(memory.as_text().expect("utf8 jsonl"));
    assert_required_fields(&parsed);
    assert_eq!(parsed["input_task"], "Who did Person A work for in 2023?");
    assert_eq!(parsed["temporal_metadata"]["valid_at"], 2023);
    assert_eq!(parsed["citations"][0]["source_id"], "source-1");
}

#[test]
fn openai_sft_and_dpo_exports_follow_current_message_shapes() {
    let examples = vec![fixture_example(TrainingTaskKind::EvidencePackSft)
        .with_rejected_answer("Person A definitely worked at Company C.")];

    let sft = TrainingDataExporter::evidence_pack_sft_jsonl(&examples).expect("sft jsonl");
    let dpo = TrainingDataExporter::belief_revision_dpo_pairs_jsonl(&examples).expect("dpo jsonl");
    let tool_dpo =
        TrainingDataExporter::tool_trace_preference_pairs_jsonl(&examples).expect("tool dpo jsonl");

    assert_eq!(sft.file_name, "evidence_pack_sft.jsonl");
    assert_eq!(dpo.file_name, "belief_revision_dpo_pairs.jsonl");
    assert_eq!(tool_dpo.file_name, "tool_trace_preference_pairs.jsonl");
    assert_eq!(sft.format, ExportFormat::OpenAiMessagesJsonl);

    let sft_line = parse_first_jsonl(sft.as_text().expect("sft utf8"));
    assert!(sft_line["messages"].as_array().expect("messages").len() >= 2);
    assert_eq!(sft_line["messages"][0]["role"], "developer");
    assert_eq!(sft_line["messages"][1]["role"], "user");
    assert_eq!(sft_line["messages"][2]["role"], "assistant");
    assert_eq!(
        sft_line["metadata"]["citations"][0]["source_id"],
        "source-1"
    );

    let dpo_line = parse_first_jsonl(dpo.as_text().expect("dpo utf8"));
    assert_eq!(dpo_line["input"]["messages"][0]["role"], "user");
    assert_eq!(dpo_line["preferred_output"][0]["role"], "assistant");
    assert_eq!(dpo_line["non_preferred_output"][0]["role"], "assistant");
    assert!(dpo_line["non_preferred_output"][0]["content"]
        .as_str()
        .expect("rejected answer")
        .contains("Company C"));
}

#[test]
fn graph_path_supervision_parquet_and_custom_format_preserve_paths() {
    let examples = vec![fixture_example(TrainingTaskKind::GraphPathSupervision)
        .with_evidence_path(EvidencePath {
            path_id: "path-1".to_string(),
            nodes: vec!["person-a".to_string(), "company-b".to_string()],
            edges: vec!["assertion-1".to_string()],
            explanation: "Employment assertion connects the person to the company.".to_string(),
        })];

    let parquet =
        TrainingDataExporter::graph_path_supervision_parquet(&examples).expect("parquet artifact");
    let custom =
        TrainingDataExporter::graph_path_supervision_jsonl(&examples).expect("custom path jsonl");

    assert_eq!(parquet.file_name, "graph_path_supervision.parquet");
    assert_eq!(parquet.format, ExportFormat::Parquet);
    assert!(parquet.bytes.starts_with(b"PAR1"));
    assert!(parquet.bytes.ends_with(b"PAR1"));
    assert!(String::from_utf8_lossy(&parquet.bytes).contains("path-1"));

    let path_line = parse_first_jsonl(custom.as_text().expect("custom utf8"));
    assert_eq!(path_line["evidence_paths"][0]["nodes"][0], "person-a");
    assert_eq!(path_line["evidence_paths"][0]["edges"][0], "assertion-1");
}

#[test]
fn arrow_and_huggingface_exports_have_dataset_schema() {
    let examples = vec![fixture_example(TrainingTaskKind::AgentMemoryTrajectory)];

    let arrow: TrainingArrowBatch = TrainingDataExporter::arrow_batch(&examples).expect("arrow");
    let hf: HuggingFaceDataset =
        TrainingDataExporter::hugging_face_dataset(&examples).expect("hf dataset");

    assert_eq!(arrow.name, "reality_graph_training_examples");
    assert_eq!(arrow.row_count, 1);
    assert!(arrow.field("input_task").is_some());
    assert!(arrow.field("graph_state").is_some());
    assert!(arrow.field("retrieved_evidence").is_some());
    assert!(arrow.field("correct_answer").is_some());
    assert!(arrow.field("rejected_answer").is_some());
    assert!(arrow.field("citations").is_some());
    assert!(arrow.field("temporal_metadata").is_some());

    assert!(hf
        .file("dataset_info.json")
        .expect("dataset info")
        .contains("features"));
    assert!(hf
        .file("train.jsonl")
        .expect("train split")
        .contains("input_task"));
    assert!(hf
        .file("README.md")
        .expect("dataset card")
        .contains("Reality Graph"));
}

#[test]
fn every_exported_example_includes_citations_temporal_metadata_and_answers() {
    let examples = vec![fixture_example(TrainingTaskKind::ToolUseTrace)
        .with_rejected_answer("Ignored the evidence path.")];
    let bundle = TrainingDataExporter::all_named_exports(&examples).expect("all exports");

    assert_eq!(bundle.artifacts.len(), 6);
    assert!(bundle
        .artifacts
        .iter()
        .any(|artifact| artifact.file_name == "memory_retrieval_examples.jsonl"));
    assert!(bundle
        .artifacts
        .iter()
        .any(|artifact| artifact.file_name == "tool_trace_preference_pairs.jsonl"));
    assert!(bundle.validate_required_fields().is_ok());
}

fn fixture_example(kind: TrainingTaskKind) -> GraphAttentionExample {
    GraphAttentionExample::new(GraphAttentionExampleDraft {
        id: TrainingExampleId::new("example-1"),
        task_kind: kind,
        input_task: "Who did Person A work for in 2023?".to_string(),
        graph_state: GraphStateSnapshot {
            entity_ids: vec!["person-a".to_string(), "company-b".to_string()],
            assertion_ids: vec!["assertion-1".to_string()],
            summary: "Person A employment graph at valid time 2023.".to_string(),
            valid_at: Some(2023),
            known_at: Some(2026),
        },
        retrieved_evidence: vec![RetrievedEvidence {
            evidence_id: "evidence-1".to_string(),
            text: "Person A worked at Company B from 2021 to 2024.".to_string(),
            source_id: "source-1".to_string(),
            assertion_ids: vec!["assertion-1".to_string()],
            score: 0.97,
        }],
        correct_answer: "Person A worked at Company B in 2023.".to_string(),
        citations: vec![Citation {
            source_id: "source-1".to_string(),
            assertion_id: Some("assertion-1".to_string()),
            uri: Some("memory://source-1".to_string()),
            quote: "worked at Company B".to_string(),
        }],
        temporal_metadata: TemporalMetadata {
            valid_at: Some(2023),
            known_at: Some(2026),
            valid_window: Some("2021..2024".to_string()),
            transaction_time: 2026,
        },
    })
}

fn parse_first_jsonl(contents: &str) -> Value {
    serde_json::from_str(contents.lines().next().expect("first jsonl line")).expect("json")
}

fn assert_required_fields(value: &Value) {
    for field in [
        "input_task",
        "graph_state",
        "retrieved_evidence",
        "correct_answer",
        "rejected_answer",
        "citations",
        "temporal_metadata",
    ] {
        assert!(value.get(field).is_some(), "missing {field}");
    }
}

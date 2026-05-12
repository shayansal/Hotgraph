use std::path::PathBuf;

use rg_ai::{EvidencePack, SourceExcerpt};
use rg_context_compression::ContextBudget;
use rg_context_serving::{
    ArrowDataType, ArrowEvidenceExporter, BatchContextRequest, ContextFrameKind,
    ContextServingStage, ContextWireFormat, OpenTelemetryStageRecorder, SourceBuffer,
    StreamingContextAssembler, ZeroCopySourceSlice,
};
use rg_core::{
    Assertion, AssertionId, AssertionStatus, Confidence, ContentHash, ContextScope, EntityId,
    GraphValue, PredicateId, SourceId, SourceType, TimeInterval, TxTime, ValidTime,
};

#[test]
fn zero_copy_source_slices_share_backing_buffer_and_enforce_bounds() {
    let source = SourceBuffer::new(SourceId::new("source-1"), b"alpha beta gamma");
    let slice = source.slice(6..10).expect("valid slice");

    assert_eq!(slice.source_id(), &SourceId::new("source-1"));
    assert_eq!(slice.as_bytes(), b"beta");
    assert_eq!(slice.as_str(), Some("beta"));
    assert!(slice.shares_backing_with(&source));

    assert_eq!(
        source.slice(20..21).expect_err("out-of-bounds slice"),
        "source slice range is outside the source buffer"
    );
    let reversed_start = 9;
    let reversed_end = 3;
    assert_eq!(
        ZeroCopySourceSlice::from_arc(
            SourceId::new("source-1"),
            source.bytes(),
            reversed_start..reversed_end
        )
        .expect_err("invalid range"),
        "source slice start must be <= end"
    );
}

#[test]
fn streaming_context_assembler_respects_token_budget_and_uses_protobuf_frames() {
    let pack = evidence_pack("supplier risk", vec![assertion("assertion-1", "source-1")]);
    let source = SourceBuffer::new(SourceId::new("source-1"), b"alpha beta gamma delta");
    let source_slice = source.slice(0..source.len()).expect("whole source slice");
    let recorder = OpenTelemetryStageRecorder::new();
    let assembler = StreamingContextAssembler::new(ContextBudget::new(4, 1), recorder);

    let stream = assembler.assemble(pack, vec![source_slice]);

    assert_eq!(stream.wire_format(), ContextWireFormat::GrpcProtobufFrames);
    assert!(stream.total_estimated_tokens() <= 3);
    assert!(stream.was_truncated());
    assert!(stream
        .frames()
        .iter()
        .any(|frame| frame.kind == ContextFrameKind::Header));
    assert!(stream
        .frames()
        .iter()
        .any(|frame| frame.kind == ContextFrameKind::Assertion));
}

#[test]
fn batch_context_request_assembles_independent_streams() {
    let pack_a = evidence_pack("first", vec![assertion("assertion-a", "source-a")]);
    let pack_b = evidence_pack("second", vec![assertion("assertion-b", "source-b")]);
    let batch = BatchContextRequest::new(
        "batch-1",
        ContextBudget::new(32, 4),
        vec![
            (
                pack_a,
                vec![SourceBuffer::new(SourceId::new("source-a"), b"a b")
                    .slice(0..3)
                    .unwrap()],
            ),
            (
                pack_b,
                vec![SourceBuffer::new(SourceId::new("source-b"), b"c d")
                    .slice(0..3)
                    .unwrap()],
            ),
        ],
    );

    let assembler =
        StreamingContextAssembler::new(ContextBudget::new(1, 0), OpenTelemetryStageRecorder::new());
    let streams = assembler.assemble_batch(batch);

    assert_eq!(streams.len(), 2);
    assert!(streams.iter().all(|stream| {
        stream.wire_format() == ContextWireFormat::GrpcProtobufFrames
            && stream
                .frames()
                .iter()
                .any(|frame| frame.kind == ContextFrameKind::Footer)
    }));
}

#[test]
fn arrow_export_contains_evidence_pack_schema_and_columns() {
    let pack = evidence_pack("employment", vec![assertion("assertion-1", "source-1")]);

    let batch = ArrowEvidenceExporter::export(&pack);

    assert_eq!(batch.name, "reality_graph_evidence_pack");
    assert_eq!(batch.row_count, 1);
    assert_eq!(
        batch
            .field("assertion_id")
            .expect("assertion_id field")
            .data_type,
        ArrowDataType::Utf8
    );
    assert_eq!(
        batch
            .field("valid_from")
            .expect("valid_from field")
            .data_type,
        ArrowDataType::Int64
    );
    assert_eq!(
        batch
            .column("source_ids")
            .expect("source_ids column")
            .values,
        vec!["source-1".to_string()]
    );
}

#[test]
fn opentelemetry_spans_cover_all_context_serving_stages() {
    let pack = evidence_pack("risk", vec![assertion("assertion-1", "source-1")]);
    let source = SourceBuffer::new(SourceId::new("source-1"), b"alpha beta");
    let recorder = OpenTelemetryStageRecorder::new();
    let assembler = StreamingContextAssembler::new(ContextBudget::new(64, 4), recorder);

    let stream = assembler.assemble(pack, vec![source.slice(0..source.len()).unwrap()]);
    let trace = stream.telemetry_trace();

    for stage in ContextServingStage::all() {
        assert!(
            trace.has_stage(stage),
            "missing OpenTelemetry span for {stage}"
        );
    }
    assert!(trace
        .spans()
        .iter()
        .all(|span| span.name.starts_with("reality_graph.context_serving.")));
}

#[test]
fn protobuf_schema_defines_streaming_evidence_pack_service() {
    let schema_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("schemas")
        .join("protobuf")
        .join("evidence_pack.proto");
    let schema = std::fs::read_to_string(schema_path).expect("protobuf schema");

    assert!(schema.contains("message EvidencePackFrame"));
    assert!(schema.contains("message ZeroCopySourceSlice"));
    assert!(schema.contains("service ContextServingService"));
    assert!(schema.contains("rpc StreamEvidencePack"));
}

fn evidence_pack(query: &str, assertions: Vec<Assertion>) -> EvidencePack {
    EvidencePack {
        query: query.to_string(),
        entities: Vec::new(),
        assertions,
        sources: vec![SourceExcerpt {
            source_id: SourceId::new("source-1"),
            source_type: SourceType::Document,
            uri: Some("memory://source-1".to_string()),
            content_hash: ContentHash::new("sha256:source-1"),
            snippet: "alpha beta".to_string(),
            trust_score: Some(0.9),
        }],
        paths: Vec::new(),
        contradictions: Vec::new(),
        generated_at: TxTime::new(100),
    }
}

fn assertion(assertion_id: &str, source_id: &str) -> Assertion {
    Assertion {
        id: AssertionId::new(assertion_id),
        subject: EntityId::new("entity-1"),
        predicate: PredicateId::new("WORKED_AT"),
        object: GraphValue::Text("Company B".to_string()),
        valid_time: TimeInterval::new(ValidTime::new(10), Some(ValidTime::new(20))).unwrap(),
        transaction_time: TimeInterval::new(TxTime::new(30), None).unwrap(),
        confidence: Confidence::new(0.92).unwrap(),
        source_ids: vec![SourceId::new(source_id)],
        context: ContextScope::Global,
        status: AssertionStatus::Active,
    }
}

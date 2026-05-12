use rg_agent_judge::{
    AgentJudge, AgentJudgeInput, ExpectedEvidence, ExpectedTemporalConstraints, GraphStateSnapshot,
    JudgeDimension, ModelAnswer, OracleAssertion, OracleMemory, RetrievedContext,
    RetrievedContextItem, ToolCall, ToolTrace,
};
use rg_core::{AssertionId, MemoryId, SourceId};

#[test]
fn grades_evidence_backed_temporal_answer_as_high_quality() {
    let report = AgentJudge.judge(good_trace_input());

    assert!(report.scores.correctness >= 0.9);
    assert!(report.scores.evidence_faithfulness >= 0.9);
    assert!(report.scores.temporal_correctness >= 0.9);
    assert!(report.scores.hallucination_score >= 0.9);
    assert!(report.scores.missing_context_score >= 0.9);
    assert!(report.scores.unsafe_memory_use >= 0.9);
    assert!(report.scores.contradiction_handling >= 0.9);
    assert!(report.passed());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.dimension == JudgeDimension::EvidenceFaithfulness));
}

#[test]
fn penalizes_hallucinated_answer_with_missing_expected_evidence() {
    let mut input = good_trace_input();
    input.model_answer = ModelAnswer {
        text: "Alice was definitely CFO of Globex in 2024.".to_owned(),
        cited_source_ids: vec![SourceId::new("source-blog-rumor")],
        cited_assertion_ids: Vec::new(),
        used_memory_ids: Vec::new(),
        stated_unknown: false,
    };
    input.retrieved_context.items.clear();
    input.expected_evidence.forbidden_terms = vec!["CFO".to_owned(), "Globex".to_owned()];

    let report = AgentJudge.judge(input);

    assert!(report.scores.correctness < 0.5);
    assert!(report.scores.evidence_faithfulness < 0.5);
    assert!(report.scores.hallucination_score < 0.5);
    assert!(report.scores.missing_context_score < 0.5);
    assert!(!report.passed());
    assert!(report.findings.iter().any(|finding| {
        finding.dimension == JudgeDimension::Hallucination && finding.message.contains("forbidden")
    }));
}

#[test]
fn rewards_knowing_when_the_graph_has_insufficient_evidence() {
    let mut input = good_trace_input();
    input.model_answer = ModelAnswer {
        text: "I do not know from the available evidence.".to_owned(),
        cited_source_ids: Vec::new(),
        cited_assertion_ids: Vec::new(),
        used_memory_ids: Vec::new(),
        stated_unknown: true,
    };
    input.expected_evidence = ExpectedEvidence {
        answer_terms: Vec::new(),
        expected_unknown: true,
        ..ExpectedEvidence::default()
    };
    input.retrieved_context.items.clear();
    input.graph_state.assertions.clear();

    let report = AgentJudge.judge(input);

    assert!(report.scores.correctness >= 0.9);
    assert!(report.scores.hallucination_score >= 0.9);
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("insufficient evidence")));
}

#[test]
fn catches_temporal_constraint_violations_even_with_relevant_sources() {
    let mut input = good_trace_input();
    input.expected_temporal_constraints = ExpectedTemporalConstraints {
        valid_at: Some(2025),
        known_at: Some(2026),
        requires_temporal_reasoning: true,
    };
    input.model_answer.text = "Alice worked at Oracle in 2025, citing the same source.".to_owned();

    let report = AgentJudge.judge(input);

    assert!(report.scores.evidence_faithfulness >= 0.8);
    assert!(report.scores.temporal_correctness < 0.6);
    assert!(report.findings.iter().any(|finding| {
        finding.dimension == JudgeDimension::TemporalCorrectness
            && finding.message.contains("not valid")
    }));
}

#[test]
fn detects_unsafe_use_of_contradicted_or_superseded_memory() {
    let mut input = good_trace_input();
    input.model_answer = ModelAnswer {
        text: "Use the old preference as current truth and ignore the conflict.".to_owned(),
        cited_source_ids: vec![SourceId::new("source-memory-old")],
        cited_assertion_ids: Vec::new(),
        used_memory_ids: vec![MemoryId::new("memory-old-preference")],
        stated_unknown: false,
    };
    input.retrieved_context.items = vec![RetrievedContextItem {
        id: "memory-old-preference".to_owned(),
        text: "Old preference, later corrected.".to_owned(),
        source_id: Some(SourceId::new("source-memory-old")),
        assertion_id: None,
        memory_id: Some(MemoryId::new("memory-old-preference")),
        valid_from: Some(2020),
        valid_to: None,
        known_at: Some(2020),
        contradicted: true,
        superseded: true,
        trust_score: Some(0.4),
    }];
    input.graph_state.memories = vec![OracleMemory {
        id: MemoryId::new("memory-old-preference"),
        current_truth: false,
        contradicted: true,
        superseded: true,
        source_ids: vec![SourceId::new("source-memory-old")],
    }];
    input.expected_evidence.contradiction_ids = vec!["conflict-memory-001".to_owned()];

    let report = AgentJudge.judge(input);

    assert!(report.scores.unsafe_memory_use < 0.5);
    assert!(report.scores.contradiction_handling < 0.6);
    assert!(report.findings.iter().any(|finding| {
        finding.dimension == JudgeDimension::UnsafeMemoryUse
            && finding.message.contains("superseded")
    }));
}

fn good_trace_input() -> AgentJudgeInput {
    AgentJudgeInput {
        task: "Was Alice employed by Oracle in 2022?".to_owned(),
        model_answer: ModelAnswer {
            text: "Alice worked at Oracle in 2022, supported by source-employment.".to_owned(),
            cited_source_ids: vec![SourceId::new("source-employment")],
            cited_assertion_ids: vec![AssertionId::new("assertion-worked-at")],
            used_memory_ids: Vec::new(),
            stated_unknown: false,
        },
        tool_trace: ToolTrace {
            calls: vec![ToolCall {
                tool_name: "get_evidence_pack".to_owned(),
                returned_source_ids: vec![SourceId::new("source-employment")],
                returned_assertion_ids: vec![AssertionId::new("assertion-worked-at")],
                returned_memory_ids: Vec::new(),
                security_audit_event_id: Some("mcp-audit-000001".to_owned()),
            }],
        },
        retrieved_context: RetrievedContext {
            items: vec![RetrievedContextItem {
                id: "ctx-worked-at".to_owned(),
                text: "Alice worked at Oracle from 2021 to 2024.".to_owned(),
                source_id: Some(SourceId::new("source-employment")),
                assertion_id: Some(AssertionId::new("assertion-worked-at")),
                memory_id: None,
                valid_from: Some(2021),
                valid_to: Some(2024),
                known_at: Some(2026),
                contradicted: false,
                superseded: false,
                trust_score: Some(0.92),
            }],
        },
        graph_state: GraphStateSnapshot {
            assertions: vec![OracleAssertion {
                id: AssertionId::new("assertion-worked-at"),
                source_ids: vec![SourceId::new("source-employment")],
                valid_from: Some(2021),
                valid_to: Some(2024),
                known_at: Some(2026),
                contradicted: false,
                current_truth: true,
            }],
            memories: Vec::new(),
            unresolved_contradiction_ids: Vec::new(),
        },
        expected_evidence: ExpectedEvidence {
            source_ids: vec![SourceId::new("source-employment")],
            assertion_ids: vec![AssertionId::new("assertion-worked-at")],
            memory_ids: Vec::new(),
            contradiction_ids: Vec::new(),
            answer_terms: vec!["Alice".to_owned(), "Oracle".to_owned(), "2022".to_owned()],
            forbidden_terms: Vec::new(),
            expected_unknown: false,
        },
        expected_temporal_constraints: ExpectedTemporalConstraints {
            valid_at: Some(2022),
            known_at: Some(2026),
            requires_temporal_reasoning: true,
        },
    }
}

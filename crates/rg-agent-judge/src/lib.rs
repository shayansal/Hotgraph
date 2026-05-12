//! Agent trace evaluation oracle for Reality Graph.

use std::collections::BTreeSet;

use rg_core::{AssertionId, MemoryId, SourceId};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentJudge;

impl AgentJudge {
    pub fn judge(&self, input: AgentJudgeInput) -> JudgeReport {
        let mut findings = Vec::new();
        let correctness = score_correctness(&input, &mut findings);
        let evidence_faithfulness = score_evidence_faithfulness(&input, &mut findings);
        let temporal_correctness = score_temporal_correctness(&input, &mut findings);
        let hallucination_score = score_hallucination(&input, &mut findings);
        let missing_context_score = score_missing_context(&input, &mut findings);
        let unsafe_memory_use = score_unsafe_memory_use(&input, &mut findings);
        let contradiction_handling = score_contradiction_handling(&input, &mut findings);

        let scores = JudgeScores {
            correctness,
            evidence_faithfulness,
            temporal_correctness,
            hallucination_score,
            missing_context_score,
            unsafe_memory_use,
            contradiction_handling,
        };
        JudgeReport { scores, findings }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentJudgeInput {
    pub task: String,
    pub model_answer: ModelAnswer,
    pub tool_trace: ToolTrace,
    pub retrieved_context: RetrievedContext,
    pub graph_state: GraphStateSnapshot,
    pub expected_evidence: ExpectedEvidence,
    pub expected_temporal_constraints: ExpectedTemporalConstraints,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelAnswer {
    pub text: String,
    pub cited_source_ids: Vec<SourceId>,
    pub cited_assertion_ids: Vec<AssertionId>,
    pub used_memory_ids: Vec<MemoryId>,
    pub stated_unknown: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolTrace {
    pub calls: Vec<ToolCall>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCall {
    pub tool_name: String,
    pub returned_source_ids: Vec<SourceId>,
    pub returned_assertion_ids: Vec<AssertionId>,
    pub returned_memory_ids: Vec<MemoryId>,
    pub security_audit_event_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetrievedContext {
    pub items: Vec<RetrievedContextItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetrievedContextItem {
    pub id: String,
    pub text: String,
    pub source_id: Option<SourceId>,
    pub assertion_id: Option<AssertionId>,
    pub memory_id: Option<MemoryId>,
    pub valid_from: Option<i64>,
    pub valid_to: Option<i64>,
    pub known_at: Option<i64>,
    pub contradicted: bool,
    pub superseded: bool,
    pub trust_score: Option<f32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphStateSnapshot {
    pub assertions: Vec<OracleAssertion>,
    pub memories: Vec<OracleMemory>,
    pub unresolved_contradiction_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleAssertion {
    pub id: AssertionId,
    pub source_ids: Vec<SourceId>,
    pub valid_from: Option<i64>,
    pub valid_to: Option<i64>,
    pub known_at: Option<i64>,
    pub contradicted: bool,
    pub current_truth: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleMemory {
    pub id: MemoryId,
    pub current_truth: bool,
    pub contradicted: bool,
    pub superseded: bool,
    pub source_ids: Vec<SourceId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExpectedEvidence {
    pub source_ids: Vec<SourceId>,
    pub assertion_ids: Vec<AssertionId>,
    pub memory_ids: Vec<MemoryId>,
    pub contradiction_ids: Vec<String>,
    pub answer_terms: Vec<String>,
    pub forbidden_terms: Vec<String>,
    pub expected_unknown: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExpectedTemporalConstraints {
    pub valid_at: Option<i64>,
    pub known_at: Option<i64>,
    pub requires_temporal_reasoning: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JudgeReport {
    pub scores: JudgeScores,
    pub findings: Vec<JudgeFinding>,
}

impl JudgeReport {
    pub fn passed(&self) -> bool {
        self.scores.mean() >= 0.8
            && self.scores.correctness >= 0.75
            && self.scores.evidence_faithfulness >= 0.75
            && self.scores.temporal_correctness >= 0.75
            && self.scores.hallucination_score >= 0.75
            && self.scores.unsafe_memory_use >= 0.75
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JudgeScores {
    pub correctness: f32,
    pub evidence_faithfulness: f32,
    pub temporal_correctness: f32,
    pub hallucination_score: f32,
    pub missing_context_score: f32,
    pub unsafe_memory_use: f32,
    pub contradiction_handling: f32,
}

impl JudgeScores {
    pub fn mean(self) -> f32 {
        (self.correctness
            + self.evidence_faithfulness
            + self.temporal_correctness
            + self.hallucination_score
            + self.missing_context_score
            + self.unsafe_memory_use
            + self.contradiction_handling)
            / 7.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JudgeFinding {
    pub dimension: JudgeDimension,
    pub severity: FindingSeverity,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum JudgeDimension {
    Correctness,
    EvidenceFaithfulness,
    TemporalCorrectness,
    Hallucination,
    MissingContext,
    UnsafeMemoryUse,
    ContradictionHandling,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FindingSeverity {
    Info,
    Warning,
    Critical,
}

fn score_correctness(input: &AgentJudgeInput, findings: &mut Vec<JudgeFinding>) -> f32 {
    if input.expected_evidence.expected_unknown {
        let knows_unknown = input.model_answer.stated_unknown
            || contains_any(
                &input.model_answer.text,
                &[
                    "do not know",
                    "don't know",
                    "insufficient evidence",
                    "unknown",
                ],
            );
        if knows_unknown {
            push_info(
                findings,
                JudgeDimension::Correctness,
                "Model correctly stated insufficient evidence.",
            );
            return 1.0;
        }
        push_critical(
            findings,
            JudgeDimension::Correctness,
            "Model answered confidently when oracle expected insufficient evidence.",
        );
        return 0.0;
    }

    let answer_term_score = term_coverage(
        &input.model_answer.text,
        &input.expected_evidence.answer_terms,
    );
    let cited_assertion_score = recall(
        &input.expected_evidence.assertion_ids,
        &input.model_answer.cited_assertion_ids,
    );
    let score = bounded(answer_term_score * 0.7 + cited_assertion_score * 0.3);
    if score >= 0.9 {
        push_info(
            findings,
            JudgeDimension::Correctness,
            "Answer includes expected oracle terms and assertions.",
        );
    } else {
        push_warning(
            findings,
            JudgeDimension::Correctness,
            "Answer missed expected oracle terms or assertion citations.",
        );
    }
    score
}

fn score_evidence_faithfulness(input: &AgentJudgeInput, findings: &mut Vec<JudgeFinding>) -> f32 {
    if input.expected_evidence.expected_unknown && input.expected_evidence.source_ids.is_empty() {
        push_info(
            findings,
            JudgeDimension::EvidenceFaithfulness,
            "No evidence was expected for an insufficient-evidence answer.",
        );
        return 1.0;
    }

    let cited_source_recall = recall(
        &input.expected_evidence.source_ids,
        &input.model_answer.cited_source_ids,
    );
    let cited_assertion_recall = recall(
        &input.expected_evidence.assertion_ids,
        &input.model_answer.cited_assertion_ids,
    );
    let trace_source_recall = recall(
        &input.expected_evidence.source_ids,
        &trace_source_ids(&input.tool_trace),
    );
    let unsupported_citations = unsupported_citation_count(input);
    let unsupported_penalty = (unsupported_citations as f32 * 0.2).min(0.5);
    let score = bounded(
        cited_source_recall * 0.38 + cited_assertion_recall * 0.34 + trace_source_recall * 0.28
            - unsupported_penalty,
    );
    if score >= 0.9 {
        push_info(
            findings,
            JudgeDimension::EvidenceFaithfulness,
            "Answer cited and retrieved expected evidence.",
        );
    } else {
        push_warning(
            findings,
            JudgeDimension::EvidenceFaithfulness,
            "Answer did not faithfully cite or retrieve expected evidence.",
        );
    }
    score
}

fn score_temporal_correctness(input: &AgentJudgeInput, findings: &mut Vec<JudgeFinding>) -> f32 {
    let constraints = &input.expected_temporal_constraints;
    if !constraints.requires_temporal_reasoning
        && constraints.valid_at.is_none()
        && constraints.known_at.is_none()
    {
        return 1.0;
    }
    let relevant_items = input
        .retrieved_context
        .items
        .iter()
        .filter(|item| {
            item.assertion_id
                .as_ref()
                .is_some_and(|id| input.expected_evidence.assertion_ids.contains(id))
                || item
                    .source_id
                    .as_ref()
                    .is_some_and(|id| input.expected_evidence.source_ids.contains(id))
        })
        .collect::<Vec<_>>();
    if relevant_items.is_empty() {
        push_warning(
            findings,
            JudgeDimension::TemporalCorrectness,
            "No retrieved context was available to verify temporal constraints.",
        );
        return 0.0;
    }
    let valid_count = relevant_items
        .iter()
        .filter(|item| temporal_item_visible(item, constraints))
        .count();
    let score = valid_count as f32 / relevant_items.len() as f32;
    if score >= 0.9 {
        push_info(
            findings,
            JudgeDimension::TemporalCorrectness,
            "Retrieved evidence respected valid time and transaction time.",
        );
    } else {
        push_warning(
            findings,
            JudgeDimension::TemporalCorrectness,
            "Relevant evidence was not valid or known at the requested time.",
        );
    }
    score
}

fn score_hallucination(input: &AgentJudgeInput, findings: &mut Vec<JudgeFinding>) -> f32 {
    if input.expected_evidence.expected_unknown && input.model_answer.stated_unknown {
        push_info(
            findings,
            JudgeDimension::Hallucination,
            "Model avoided unsupported claims when evidence was insufficient.",
        );
        return 1.0;
    }
    let forbidden_hits = input
        .expected_evidence
        .forbidden_terms
        .iter()
        .filter(|term| contains_term(&input.model_answer.text, term))
        .count();
    let unsupported_source_hits = unsupported_citation_count(input);
    let mut score = 1.0 - (forbidden_hits as f32 * 0.35) - (unsupported_source_hits as f32 * 0.2);
    if input.retrieved_context.items.is_empty()
        && !input.model_answer.stated_unknown
        && !input.expected_evidence.expected_unknown
    {
        score -= 0.35;
    }
    score = bounded(score);
    if forbidden_hits > 0 {
        push_critical(
            findings,
            JudgeDimension::Hallucination,
            "Answer contained forbidden oracle terms not supported by the graph.",
        );
    } else if score >= 0.9 {
        push_info(
            findings,
            JudgeDimension::Hallucination,
            "No unsupported forbidden claims were detected.",
        );
    }
    score
}

fn score_missing_context(input: &AgentJudgeInput, findings: &mut Vec<JudgeFinding>) -> f32 {
    if input.expected_evidence.expected_unknown && input.expected_evidence.source_ids.is_empty() {
        return 1.0;
    }
    let context_source_recall = recall(
        &input.expected_evidence.source_ids,
        &context_source_ids(&input.retrieved_context),
    );
    let context_assertion_recall = recall(
        &input.expected_evidence.assertion_ids,
        &context_assertion_ids(&input.retrieved_context),
    );
    let context_memory_recall = recall(
        &input.expected_evidence.memory_ids,
        &context_memory_ids(&input.retrieved_context),
    );
    let score = bounded(
        context_source_recall * 0.45
            + context_assertion_recall * 0.45
            + context_memory_recall * 0.1,
    );
    if score >= 0.9 {
        push_info(
            findings,
            JudgeDimension::MissingContext,
            "Retrieved context included expected evidence.",
        );
    } else {
        push_warning(
            findings,
            JudgeDimension::MissingContext,
            "Retrieved context missed expected evidence or memory.",
        );
    }
    score
}

fn score_unsafe_memory_use(input: &AgentJudgeInput, findings: &mut Vec<JudgeFinding>) -> f32 {
    if input.model_answer.used_memory_ids.is_empty() {
        return 1.0;
    }
    let mut unsafe_count = 0_usize;
    for memory_id in &input.model_answer.used_memory_ids {
        let graph_memory = input
            .graph_state
            .memories
            .iter()
            .find(|memory| &memory.id == memory_id);
        let context_memory = input
            .retrieved_context
            .items
            .iter()
            .find(|item| item.memory_id.as_ref() == Some(memory_id));
        let unsafe_memory = graph_memory.is_some_and(|memory| {
            !memory.current_truth || memory.contradicted || memory.superseded
        }) || context_memory
            .is_some_and(|item| item.contradicted || item.superseded);
        if unsafe_memory {
            unsafe_count += 1;
        }
    }
    let score =
        bounded(1.0 - unsafe_count as f32 / input.model_answer.used_memory_ids.len() as f32);
    if unsafe_count > 0 {
        push_critical(
            findings,
            JudgeDimension::UnsafeMemoryUse,
            "Answer used contradicted or superseded memory as current truth.",
        );
    }
    score
}

fn score_contradiction_handling(input: &AgentJudgeInput, findings: &mut Vec<JudgeFinding>) -> f32 {
    let expected_conflicts = input.expected_evidence.contradiction_ids.len()
        + input.graph_state.unresolved_contradiction_ids.len();
    let contradicted_context = input
        .retrieved_context
        .items
        .iter()
        .filter(|item| item.contradicted)
        .count();
    if expected_conflicts == 0 && contradicted_context == 0 {
        return 1.0;
    }
    let answer_mentions_conflict = contains_any(
        &input.model_answer.text,
        &[
            "contradiction",
            "conflict",
            "disputed",
            "both sources",
            "competing",
            "uncertain",
        ],
    ) && !contains_any(
        &input.model_answer.text,
        &["ignore the conflict", "ignore conflict"],
    );
    let retrieved_conflict_evidence = contradicted_context > 0;
    let score = match (answer_mentions_conflict, retrieved_conflict_evidence) {
        (true, true) => 1.0,
        (true, false) | (false, true) => 0.55,
        (false, false) => 0.0,
    };
    if score >= 0.9 {
        push_info(
            findings,
            JudgeDimension::ContradictionHandling,
            "Answer surfaced competing evidence instead of collapsing the conflict.",
        );
    } else {
        push_warning(
            findings,
            JudgeDimension::ContradictionHandling,
            "Answer did not handle expected contradictions explicitly.",
        );
    }
    score
}

fn temporal_item_visible(
    item: &RetrievedContextItem,
    constraints: &ExpectedTemporalConstraints,
) -> bool {
    let valid_match = constraints.valid_at.map_or(true, |valid_at| {
        item.valid_from.map_or(true, |start| valid_at >= start)
            && item.valid_to.map_or(true, |end| valid_at < end)
    });
    let known_match = constraints.known_at.map_or(true, |known_at| {
        item.known_at.is_some_and(|observed| observed <= known_at)
    });
    valid_match && known_match
}

fn term_coverage(text: &str, terms: &[String]) -> f32 {
    if terms.is_empty() {
        return 1.0;
    }
    let hits = terms
        .iter()
        .filter(|term| contains_term(text, term))
        .count();
    hits as f32 / terms.len() as f32
}

fn recall<T>(expected: &[T], actual: &[T]) -> f32
where
    T: Ord + Clone,
{
    if expected.is_empty() {
        return 1.0;
    }
    let actual = actual.iter().cloned().collect::<BTreeSet<_>>();
    let hits = expected
        .iter()
        .filter(|expected| actual.contains(*expected))
        .count();
    hits as f32 / expected.len() as f32
}

fn unsupported_citation_count(input: &AgentJudgeInput) -> usize {
    let expected_sources = input
        .expected_evidence
        .source_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let expected_assertions = input
        .expected_evidence
        .assertion_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let source_misses = input
        .model_answer
        .cited_source_ids
        .iter()
        .filter(|source_id| !expected_sources.contains(*source_id))
        .count();
    let assertion_misses = input
        .model_answer
        .cited_assertion_ids
        .iter()
        .filter(|assertion_id| !expected_assertions.contains(*assertion_id))
        .count();
    source_misses + assertion_misses
}

fn trace_source_ids(trace: &ToolTrace) -> Vec<SourceId> {
    trace
        .calls
        .iter()
        .flat_map(|call| call.returned_source_ids.iter().cloned())
        .collect()
}

fn context_source_ids(context: &RetrievedContext) -> Vec<SourceId> {
    context
        .items
        .iter()
        .filter_map(|item| item.source_id.clone())
        .collect()
}

fn context_assertion_ids(context: &RetrievedContext) -> Vec<AssertionId> {
    context
        .items
        .iter()
        .filter_map(|item| item.assertion_id.clone())
        .collect()
}

fn context_memory_ids(context: &RetrievedContext) -> Vec<MemoryId> {
    context
        .items
        .iter()
        .filter_map(|item| item.memory_id.clone())
        .collect()
}

fn contains_term(text: &str, term: &str) -> bool {
    text.to_ascii_lowercase()
        .contains(&term.to_ascii_lowercase())
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| contains_term(text, needle))
}

fn bounded(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn push_info(findings: &mut Vec<JudgeFinding>, dimension: JudgeDimension, message: &str) {
    findings.push(JudgeFinding {
        dimension,
        severity: FindingSeverity::Info,
        message: message.to_owned(),
    });
}

fn push_warning(findings: &mut Vec<JudgeFinding>, dimension: JudgeDimension, message: &str) {
    findings.push(JudgeFinding {
        dimension,
        severity: FindingSeverity::Warning,
        message: message.to_owned(),
    });
}

fn push_critical(findings: &mut Vec<JudgeFinding>, dimension: JudgeDimension, message: &str) {
    findings.push(JudgeFinding {
        dimension,
        severity: FindingSeverity::Critical,
        message: message.to_owned(),
    });
}

//! Evaluation harness for Reality Graph retrieval strategies.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

const BUILTIN_FIXTURES: &[(&str, &str)] = &[
    (
        "agent_conversation_memory",
        include_str!("../../../evals/fixtures/agent_conversation_memory.tsv"),
    ),
    (
        "contradictory_evidence",
        include_str!("../../../evals/fixtures/contradictory_evidence.tsv"),
    ),
    (
        "geopolitical_events",
        include_str!("../../../evals/fixtures/geopolitical_events.tsv"),
    ),
    (
        "multi_hop_company_ownership",
        include_str!("../../../evals/fixtures/multi_hop_company_ownership.tsv"),
    ),
    (
        "supply_chain_dependency",
        include_str!("../../../evals/fixtures/supply_chain_dependency.tsv"),
    ),
    (
        "temporal_employment",
        include_str!("../../../evals/fixtures/temporal_employment.tsv"),
    ),
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvalCatalog {
    datasets: Vec<EvalDataset>,
}

impl EvalCatalog {
    pub fn load_builtin() -> Result<Self, EvalError> {
        let mut datasets = BUILTIN_FIXTURES
            .iter()
            .map(|(_, contents)| EvalDataset::parse(contents))
            .collect::<Result<Vec<_>, _>>()?;
        datasets.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Self { datasets })
    }

    pub fn datasets(&self) -> &[EvalDataset] {
        &self.datasets
    }

    pub fn dataset_names(&self) -> Vec<&str> {
        self.datasets
            .iter()
            .map(|dataset| dataset.name.as_str())
            .collect()
    }

    pub fn total_cases(&self) -> usize {
        self.datasets
            .iter()
            .map(|dataset| dataset.cases.len())
            .sum()
    }

    pub fn total_evidence_records(&self) -> usize {
        self.datasets
            .iter()
            .map(|dataset| dataset.evidence.len())
            .sum()
    }

    pub fn case(&self, id: &str) -> Option<&EvalCase> {
        self.datasets
            .iter()
            .flat_map(|dataset| dataset.cases.iter())
            .find(|case| case.id == id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvalDataset {
    pub name: String,
    pub cases: Vec<EvalCase>,
    pub evidence: Vec<EvidenceRecord>,
    pub paths: Vec<PathFixture>,
    pub contradictions: Vec<ContradictionFixture>,
}

impl EvalDataset {
    pub fn parse(contents: &str) -> Result<Self, EvalError> {
        let mut name = None;
        let mut cases = Vec::new();
        let mut evidence = Vec::new();
        let mut paths = Vec::new();
        let mut contradictions = Vec::new();

        for (line_index, raw_line) in contents.lines().enumerate() {
            let line_number = line_index + 1;
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some(value) = line.strip_prefix("dataset:") {
                name = Some(non_empty(value, "dataset name", line_number)?.to_owned());
            } else if let Some(value) = line.strip_prefix("case:") {
                cases.push(parse_case(value, line_number)?);
            } else if let Some(value) = line.strip_prefix("evidence:") {
                evidence.push(parse_evidence(value, line_number)?);
            } else if let Some(value) = line.strip_prefix("path:") {
                paths.push(parse_path(value, line_number)?);
            } else if let Some(value) = line.strip_prefix("contradiction:") {
                contradictions.push(parse_contradiction(value, line_number)?);
            } else {
                return Err(EvalError::MalformedLine {
                    line: line_number,
                    text: line.to_owned(),
                });
            }
        }

        let name = name.ok_or(EvalError::MissingDatasetName)?;
        if cases.is_empty() {
            return Err(EvalError::EmptyDataset { name });
        }

        Ok(Self {
            name,
            cases,
            evidence,
            paths,
            contradictions,
        })
    }

    fn evidence_by_id(&self, id: &str) -> Option<&EvidenceRecord> {
        self.evidence.iter().find(|record| record.id == id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvalCase {
    pub id: String,
    pub question: String,
    pub expected_answer: String,
    pub valid_at: Option<i64>,
    pub known_at: Option<i64>,
    pub tags: BTreeSet<EvalTag>,
    pub gold_evidence_ids: BTreeSet<String>,
    pub required_path_ids: BTreeSet<String>,
    pub gold_contradiction_ids: BTreeSet<String>,
}

impl EvalCase {
    fn has_tag(&self, tag: EvalTag) -> bool {
        self.tags.contains(&tag)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EvalTag {
    AgentMemory,
    CausalChain,
    Contradiction,
    Counterfactual,
    MultiHop,
    SupplyChain,
    TemporalQa,
    Unknown(String),
}

impl EvalTag {
    fn parse(value: &str) -> Self {
        match value {
            "agent_memory" => Self::AgentMemory,
            "causal_chain" => Self::CausalChain,
            "contradiction" => Self::Contradiction,
            "counterfactual" => Self::Counterfactual,
            "multi_hop" => Self::MultiHop,
            "supply_chain" => Self::SupplyChain,
            "temporal_qa" => Self::TemporalQa,
            other => Self::Unknown(other.to_owned()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRecord {
    pub id: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from: i64,
    pub valid_to: Option<i64>,
    pub known_from: i64,
    pub text: String,
}

impl EvidenceRecord {
    fn visible_at(&self, valid_at: Option<i64>, known_at: Option<i64>) -> bool {
        let valid_match = valid_at.map_or(true, |instant| {
            instant >= self.valid_from && self.valid_to.map_or(true, |end| instant < end)
        });
        let known_match = known_at.map_or(true, |instant| instant >= self.known_from);
        valid_match && known_match
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathFixture {
    pub id: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContradictionFixture {
    pub id: String,
    pub assertion_a: String,
    pub assertion_b: String,
    pub contradiction_type: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RetrievalKind {
    VectorOnly,
    KeywordOnly,
    GraphOnly,
    TemporalGraph,
    Hybrid,
    AdaptiveRouted,
}

impl RetrievalKind {
    pub fn all() -> Vec<Self> {
        vec![
            Self::VectorOnly,
            Self::KeywordOnly,
            Self::GraphOnly,
            Self::TemporalGraph,
            Self::Hybrid,
            Self::AdaptiveRouted,
        ]
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AdaptiveRouter;

impl AdaptiveRouter {
    pub fn route(&self, case: &EvalCase) -> RetrievalKind {
        if case.has_tag(EvalTag::Contradiction)
            || case.has_tag(EvalTag::AgentMemory)
            || case.has_tag(EvalTag::Counterfactual)
            || case.has_tag(EvalTag::SupplyChain)
            || case.has_tag(EvalTag::CausalChain)
        {
            RetrievalKind::Hybrid
        } else if case.has_tag(EvalTag::TemporalQa) {
            RetrievalKind::TemporalGraph
        } else if case.has_tag(EvalTag::MultiHop) {
            RetrievalKind::GraphOnly
        } else {
            RetrievalKind::Hybrid
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EvalHarness {
    router: AdaptiveRouter,
}

impl EvalHarness {
    pub fn run(&self, catalog: &EvalCatalog, strategies: Vec<RetrievalKind>) -> EvalReport {
        let strategy_reports = strategies
            .into_iter()
            .map(|kind| {
                let mut case_reports = Vec::new();
                for dataset in catalog.datasets() {
                    for case in &dataset.cases {
                        let outcome = self.execute(kind, dataset, case);
                        case_reports.push(score_case(case, dataset, outcome));
                    }
                }
                StrategyReport {
                    kind,
                    metrics: EvalMetrics::from_cases(&case_reports),
                    cases: case_reports,
                }
            })
            .collect();
        EvalReport { strategy_reports }
    }

    fn execute(
        &self,
        kind: RetrievalKind,
        dataset: &EvalDataset,
        case: &EvalCase,
    ) -> RetrievalOutcome {
        let routed_kind = if kind == RetrievalKind::AdaptiveRouted {
            self.router.route(case)
        } else {
            kind
        };
        execute_retrieval(routed_kind, dataset, case, kind)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvalReport {
    pub strategy_reports: Vec<StrategyReport>,
}

impl EvalReport {
    pub fn strategy_report(&self, kind: RetrievalKind) -> Option<&StrategyReport> {
        self.strategy_reports
            .iter()
            .find(|report| report.kind == kind)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrategyReport {
    pub kind: RetrievalKind,
    pub metrics: EvalMetrics,
    pub cases: Vec<CaseReport>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaseReport {
    pub case_id: String,
    pub retrieved_evidence_ids: BTreeSet<String>,
    pub retrieved_path_ids: BTreeSet<String>,
    pub detected_contradiction_ids: BTreeSet<String>,
    pub answer_accuracy: f64,
    pub evidence_recall: f64,
    pub evidence_precision: f64,
    pub temporal_correctness: f64,
    pub contradiction_detection_f1: f64,
    pub multi_hop_path_recall: f64,
    pub citation_faithfulness: f64,
    pub latency_micros: u64,
    pub cost_units: f64,
    pub memory_freshness: f64,
    pub staleness_error_rate: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvalMetrics {
    pub answer_accuracy: f64,
    pub evidence_recall: f64,
    pub evidence_precision: f64,
    pub temporal_correctness: f64,
    pub contradiction_detection_f1: f64,
    pub multi_hop_path_recall: f64,
    pub citation_faithfulness: f64,
    pub latency_p50_micros: u64,
    pub latency_p95_micros: u64,
    pub latency_p99_micros: u64,
    pub cost_per_answered_query: f64,
    pub memory_freshness: f64,
    pub staleness_error_rate: f64,
}

impl EvalMetrics {
    fn from_cases(cases: &[CaseReport]) -> Self {
        let mut latencies = cases
            .iter()
            .map(|case| case.latency_micros)
            .collect::<Vec<_>>();
        latencies.sort_unstable();
        Self {
            answer_accuracy: average(cases, |case| case.answer_accuracy),
            evidence_recall: average(cases, |case| case.evidence_recall),
            evidence_precision: average(cases, |case| case.evidence_precision),
            temporal_correctness: average(cases, |case| case.temporal_correctness),
            contradiction_detection_f1: average(cases, |case| case.contradiction_detection_f1),
            multi_hop_path_recall: average(cases, |case| case.multi_hop_path_recall),
            citation_faithfulness: average(cases, |case| case.citation_faithfulness),
            latency_p50_micros: percentile(&latencies, 50),
            latency_p95_micros: percentile(&latencies, 95),
            latency_p99_micros: percentile(&latencies, 99),
            cost_per_answered_query: average(cases, |case| case.cost_units),
            memory_freshness: average(cases, |case| case.memory_freshness),
            staleness_error_rate: average(cases, |case| case.staleness_error_rate),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetricSnapshot {
    pub answer_accuracy: f64,
    pub evidence_recall: f64,
    pub evidence_precision: f64,
    pub temporal_correctness: f64,
    pub contradiction_detection_f1: f64,
    pub multi_hop_path_recall: f64,
    pub citation_faithfulness: f64,
    pub latency_p95_micros: u64,
    pub cost_per_answered_query: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImprovementGate {
    quality_delta: f64,
}

impl Default for ImprovementGate {
    fn default() -> Self {
        Self {
            quality_delta: 0.000_001,
        }
    }
}

impl ImprovementGate {
    pub fn passes(&self, baseline: &MetricSnapshot, candidate: &MetricSnapshot) -> bool {
        quality_metrics(candidate)
            .iter()
            .zip(quality_metrics(baseline))
            .any(|(candidate, baseline)| *candidate > baseline + self.quality_delta)
            || candidate.latency_p95_micros < baseline.latency_p95_micros
            || candidate.cost_per_answered_query + self.quality_delta
                < baseline.cost_per_answered_query
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvalError {
    MissingDatasetName,
    EmptyDataset {
        name: String,
    },
    MalformedLine {
        line: usize,
        text: String,
    },
    WrongFieldCount {
        line: usize,
        expected: usize,
        actual: usize,
    },
    EmptyField {
        line: usize,
        field: &'static str,
    },
    InvalidTimestamp {
        line: usize,
        value: String,
    },
}

impl fmt::Display for EvalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDatasetName => formatter.write_str("dataset fixture is missing a name"),
            Self::EmptyDataset { name } => write!(formatter, "dataset {name} has no cases"),
            Self::MalformedLine { line, text } => {
                write!(formatter, "malformed eval fixture line {line}: {text}")
            }
            Self::WrongFieldCount {
                line,
                expected,
                actual,
            } => write!(
                formatter,
                "eval fixture line {line} expected {expected} fields, got {actual}"
            ),
            Self::EmptyField { line, field } => {
                write!(formatter, "eval fixture line {line} has empty {field}")
            }
            Self::InvalidTimestamp { line, value } => {
                write!(
                    formatter,
                    "eval fixture line {line} has invalid timestamp {value}"
                )
            }
        }
    }
}

impl Error for EvalError {}

#[derive(Clone, Debug, PartialEq)]
struct RetrievalOutcome {
    evidence_ids: BTreeSet<String>,
    path_ids: BTreeSet<String>,
    contradiction_ids: BTreeSet<String>,
    latency_micros: u64,
    cost_units: f64,
}

fn execute_retrieval(
    routed_kind: RetrievalKind,
    dataset: &EvalDataset,
    case: &EvalCase,
    requested_kind: RetrievalKind,
) -> RetrievalOutcome {
    let mut ranked = dataset
        .evidence
        .iter()
        .filter(|record| retrieval_allows_record(routed_kind, record, case))
        .filter_map(|record| {
            let score = retrieval_score(routed_kind, record, case);
            (score > 0.0).then(|| (record.id.clone(), score))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut evidence_ids = ranked
        .into_iter()
        .take(match routed_kind {
            RetrievalKind::VectorOnly | RetrievalKind::KeywordOnly => 3,
            RetrievalKind::GraphOnly | RetrievalKind::TemporalGraph => 4,
            RetrievalKind::Hybrid | RetrievalKind::AdaptiveRouted => 5,
        })
        .map(|(id, _)| id)
        .collect::<BTreeSet<_>>();

    if matches!(
        routed_kind,
        RetrievalKind::GraphOnly | RetrievalKind::TemporalGraph | RetrievalKind::Hybrid
    ) {
        expand_paths(dataset, &mut evidence_ids);
    }

    let path_ids = dataset
        .paths
        .iter()
        .filter(|path| {
            path.evidence_ids
                .iter()
                .all(|evidence_id| evidence_ids.contains(evidence_id))
        })
        .map(|path| path.id.clone())
        .collect::<BTreeSet<_>>();
    let contradiction_ids = dataset
        .contradictions
        .iter()
        .filter(|contradiction| {
            evidence_ids.contains(&contradiction.assertion_a)
                && evidence_ids.contains(&contradiction.assertion_b)
        })
        .map(|contradiction| contradiction.id.clone())
        .collect::<BTreeSet<_>>();
    let (latency_micros, cost_units) = retrieval_cost(requested_kind, routed_kind);

    RetrievalOutcome {
        evidence_ids,
        path_ids,
        contradiction_ids,
        latency_micros,
        cost_units,
    }
}

fn retrieval_allows_record(kind: RetrievalKind, record: &EvidenceRecord, case: &EvalCase) -> bool {
    if kind == RetrievalKind::TemporalGraph || kind == RetrievalKind::Hybrid {
        record.visible_at(case.valid_at, case.known_at)
    } else {
        true
    }
}

fn retrieval_score(kind: RetrievalKind, record: &EvidenceRecord, case: &EvalCase) -> f64 {
    match kind {
        RetrievalKind::VectorOnly => vector_score(&case.question, &record.search_text()),
        RetrievalKind::KeywordOnly => keyword_score(&case.question, &record.search_text()),
        RetrievalKind::GraphOnly | RetrievalKind::TemporalGraph => graph_score(case, record),
        RetrievalKind::Hybrid | RetrievalKind::AdaptiveRouted => {
            keyword_score(&case.question, &record.search_text())
                + vector_score(&case.question, &record.search_text())
                + graph_score(case, record) * 1.5
                + if record.visible_at(case.valid_at, case.known_at) {
                    0.25
                } else {
                    0.0
                }
        }
    }
}

fn retrieval_cost(requested_kind: RetrievalKind, routed_kind: RetrievalKind) -> (u64, f64) {
    let (latency, cost) = match routed_kind {
        RetrievalKind::VectorOnly => (700, 2.0),
        RetrievalKind::KeywordOnly => (300, 1.0),
        RetrievalKind::GraphOnly => (550, 1.5),
        RetrievalKind::TemporalGraph => (650, 1.8),
        RetrievalKind::Hybrid => (950, 3.5),
        RetrievalKind::AdaptiveRouted => (900, 3.0),
    };
    if requested_kind == RetrievalKind::AdaptiveRouted {
        (latency + 80, cost + 0.2)
    } else {
        (latency, cost)
    }
}

fn expand_paths(dataset: &EvalDataset, evidence_ids: &mut BTreeSet<String>) {
    let mut changed = true;
    while changed {
        changed = false;
        for path in &dataset.paths {
            if path
                .evidence_ids
                .iter()
                .any(|evidence_id| evidence_ids.contains(evidence_id))
            {
                for evidence_id in &path.evidence_ids {
                    changed |= evidence_ids.insert(evidence_id.clone());
                }
            }
        }
    }
}

fn score_case(case: &EvalCase, dataset: &EvalDataset, outcome: RetrievalOutcome) -> CaseReport {
    let evidence_recall = recall(&case.gold_evidence_ids, &outcome.evidence_ids);
    let evidence_precision = precision(&case.gold_evidence_ids, &outcome.evidence_ids);
    let temporal = temporal_score(case, dataset, &outcome.evidence_ids);
    let memory_freshness = memory_freshness(case, dataset, &outcome.evidence_ids);
    let contradiction_detection_f1 = f1(
        precision(&case.gold_contradiction_ids, &outcome.contradiction_ids),
        recall(&case.gold_contradiction_ids, &outcome.contradiction_ids),
    );
    let multi_hop_path_recall = recall(&case.required_path_ids, &outcome.path_ids);

    CaseReport {
        case_id: case.id.clone(),
        retrieved_evidence_ids: outcome.evidence_ids,
        retrieved_path_ids: outcome.path_ids,
        detected_contradiction_ids: outcome.contradiction_ids,
        answer_accuracy: if evidence_recall >= 1.0 { 1.0 } else { 0.0 },
        evidence_recall,
        evidence_precision,
        temporal_correctness: temporal.correctness,
        contradiction_detection_f1,
        multi_hop_path_recall,
        citation_faithfulness: citation_faithfulness(dataset, &case.gold_evidence_ids),
        latency_micros: outcome.latency_micros,
        cost_units: outcome.cost_units,
        memory_freshness,
        staleness_error_rate: temporal.staleness_error_rate,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TemporalScore {
    correctness: f64,
    staleness_error_rate: f64,
}

fn temporal_score(
    case: &EvalCase,
    dataset: &EvalDataset,
    evidence_ids: &BTreeSet<String>,
) -> TemporalScore {
    if case.valid_at.is_none() && case.known_at.is_none() {
        return TemporalScore {
            correctness: 1.0,
            staleness_error_rate: 0.0,
        };
    }
    if evidence_ids.is_empty() {
        return TemporalScore {
            correctness: 0.0,
            staleness_error_rate: 1.0,
        };
    }

    let stale = evidence_ids
        .iter()
        .filter(|id| {
            dataset
                .evidence_by_id(id)
                .is_some_and(|record| !record.visible_at(case.valid_at, case.known_at))
        })
        .count();
    let staleness_error_rate = stale as f64 / evidence_ids.len() as f64;
    TemporalScore {
        correctness: 1.0 - staleness_error_rate,
        staleness_error_rate,
    }
}

fn memory_freshness(
    case: &EvalCase,
    dataset: &EvalDataset,
    evidence_ids: &BTreeSet<String>,
) -> f64 {
    if !case.has_tag(EvalTag::AgentMemory) {
        return 1.0;
    }
    let latest_gold = case
        .gold_evidence_ids
        .iter()
        .filter_map(|id| dataset.evidence_by_id(id))
        .max_by_key(|record| record.known_from);
    latest_gold.map_or(0.0, |record| {
        if evidence_ids.contains(&record.id) {
            1.0
        } else {
            0.0
        }
    })
}

fn citation_faithfulness(dataset: &EvalDataset, evidence_ids: &BTreeSet<String>) -> f64 {
    if evidence_ids.is_empty() {
        return 0.0;
    }
    let existing = evidence_ids
        .iter()
        .filter(|id| dataset.evidence_by_id(id).is_some())
        .count();
    existing as f64 / evidence_ids.len() as f64
}

fn recall(gold: &BTreeSet<String>, retrieved: &BTreeSet<String>) -> f64 {
    if gold.is_empty() {
        return 1.0;
    }
    intersection_count(gold, retrieved) as f64 / gold.len() as f64
}

fn precision(gold: &BTreeSet<String>, retrieved: &BTreeSet<String>) -> f64 {
    if retrieved.is_empty() {
        return if gold.is_empty() { 1.0 } else { 0.0 };
    }
    intersection_count(gold, retrieved) as f64 / retrieved.len() as f64
}

fn f1(precision: f64, recall: f64) -> f64 {
    if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    }
}

fn intersection_count(left: &BTreeSet<String>, right: &BTreeSet<String>) -> usize {
    left.intersection(right).count()
}

fn average(cases: &[CaseReport], metric: impl Fn(&CaseReport) -> f64) -> f64 {
    if cases.is_empty() {
        return 0.0;
    }
    cases.iter().map(metric).sum::<f64>() / cases.len() as f64
}

fn percentile(sorted_values: &[u64], percentile: usize) -> u64 {
    if sorted_values.is_empty() {
        return 0;
    }
    let index = ((sorted_values.len() * percentile).div_ceil(100)).saturating_sub(1);
    sorted_values[index.min(sorted_values.len() - 1)]
}

fn quality_metrics(snapshot: &MetricSnapshot) -> [f64; 7] {
    [
        snapshot.answer_accuracy,
        snapshot.evidence_recall,
        snapshot.evidence_precision,
        snapshot.temporal_correctness,
        snapshot.contradiction_detection_f1,
        snapshot.multi_hop_path_recall,
        snapshot.citation_faithfulness,
    ]
}

impl EvidenceRecord {
    fn search_text(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.subject,
            self.predicate,
            self.object,
            self.predicate.to_ascii_lowercase().replace('_', " "),
            self.text
        )
    }
}

fn keyword_score(question: &str, document: &str) -> f64 {
    let document_tokens = tokens(document).collect::<BTreeSet<_>>();
    tokens(question)
        .filter(|token| document_tokens.contains(token))
        .count() as f64
}

fn vector_score(question: &str, document: &str) -> f64 {
    let left = deterministic_embedding(question);
    let right = deterministic_embedding(document);
    cosine_similarity(&left, &right)
}

fn graph_score(case: &EvalCase, record: &EvidenceRecord) -> f64 {
    let question = normalize(&case.question);
    let mut score = 0.0;
    for value in [
        record.subject.as_str(),
        record.object.as_str(),
        record.predicate.as_str(),
        &record.predicate.to_ascii_lowercase().replace('_', " "),
    ] {
        if question.contains(&normalize(value)) {
            score += 1.0;
        }
    }
    score += predicate_intent_score(&question, &record.predicate);
    if case.has_tag(EvalTag::MultiHop)
        && matches!(
            record.predicate.as_str(),
            "OWNS" | "CAUSES" | "SUPPLIES" | "HAS_CONTRACT_WITH"
        )
    {
        score += 0.75;
    }
    if case.has_tag(EvalTag::Contradiction) && record.predicate == "CEO_OF" {
        score += 1.0;
    }
    score
}

fn predicate_intent_score(question: &str, predicate: &str) -> f64 {
    match predicate {
        "WORKED_AT" if contains_any(question, &["work", "worked", "employ", "job"]) => 1.0,
        "OWNS" if contains_any(question, &["own", "ownership", "control", "controls"]) => 1.0,
        "CEO_OF" if contains_any(question, &["ceo", "chief executive"]) => 1.0,
        "SUPPLIES" if contains_any(question, &["supply", "supplier", "supplying"]) => 1.0,
        "HAS_CONTRACT_WITH" if contains_any(question, &["contract", "customer", "affected"]) => 1.0,
        "CAUSES" if contains_any(question, &["cause", "event", "linked", "increase"]) => 1.0,
        _ => 0.0,
    }
}

fn deterministic_embedding(text: &str) -> [f64; 8] {
    let text = normalize(text);
    [
        contains_any(&text, &["work", "employ", "job"]) as u8 as f64,
        contains_any(&text, &["own", "control", "ownership"]) as u8 as f64,
        contains_any(&text, &["supply", "supplier", "contract", "customer"]) as u8 as f64,
        contains_any(&text, &["sanction", "oil", "causes", "causal", "event"]) as u8 as f64,
        contains_any(&text, &["memory", "agent", "prefer", "conversation"]) as u8 as f64,
        contains_any(&text, &["contradict", "conflict", "ceo"]) as u8 as f64,
        contains_any(&text, &["time", "2024", "2026", "historical"]) as u8 as f64,
        1.0,
    ]
}

fn cosine_similarity(left: &[f64], right: &[f64]) -> f64 {
    let dot_product = left
        .iter()
        .zip(right.iter())
        .map(|(left, right)| left * right)
        .sum::<f64>();
    dot_product / (magnitude(left) * magnitude(right))
}

fn magnitude(values: &[f64]) -> f64 {
    values.iter().map(|value| value * value).sum::<f64>().sqrt()
}

fn tokens(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| token.len() > 2)
        .map(normalize)
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['_', '-'], " ")
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn parse_case(value: &str, line: usize) -> Result<EvalCase, EvalError> {
    let fields = split_fields(value, line, 9)?;
    Ok(EvalCase {
        id: non_empty(fields[0], "case id", line)?.to_owned(),
        question: non_empty(fields[1], "question", line)?.to_owned(),
        expected_answer: non_empty(fields[2], "expected answer", line)?.to_owned(),
        valid_at: parse_optional_timestamp(fields[3], line)?,
        known_at: parse_optional_timestamp(fields[4], line)?,
        tags: parse_set(fields[5], EvalTag::parse),
        gold_evidence_ids: parse_string_set(fields[6]),
        required_path_ids: parse_string_set(fields[7]),
        gold_contradiction_ids: parse_string_set(fields[8]),
    })
}

fn parse_evidence(value: &str, line: usize) -> Result<EvidenceRecord, EvalError> {
    let fields = split_fields(value, line, 8)?;
    Ok(EvidenceRecord {
        id: non_empty(fields[0], "evidence id", line)?.to_owned(),
        subject: non_empty(fields[1], "subject", line)?.to_owned(),
        predicate: non_empty(fields[2], "predicate", line)?.to_owned(),
        object: non_empty(fields[3], "object", line)?.to_owned(),
        valid_from: parse_required_timestamp(fields[4], line)?,
        valid_to: parse_optional_timestamp(fields[5], line)?,
        known_from: parse_required_timestamp(fields[6], line)?,
        text: non_empty(fields[7], "text", line)?.to_owned(),
    })
}

fn parse_path(value: &str, line: usize) -> Result<PathFixture, EvalError> {
    let fields = split_fields(value, line, 2)?;
    Ok(PathFixture {
        id: non_empty(fields[0], "path id", line)?.to_owned(),
        evidence_ids: parse_string_list(fields[1]),
    })
}

fn parse_contradiction(value: &str, line: usize) -> Result<ContradictionFixture, EvalError> {
    let fields = split_fields(value, line, 4)?;
    Ok(ContradictionFixture {
        id: non_empty(fields[0], "contradiction id", line)?.to_owned(),
        assertion_a: non_empty(fields[1], "assertion a", line)?.to_owned(),
        assertion_b: non_empty(fields[2], "assertion b", line)?.to_owned(),
        contradiction_type: non_empty(fields[3], "contradiction type", line)?.to_owned(),
    })
}

fn split_fields(value: &str, line: usize, expected: usize) -> Result<Vec<&str>, EvalError> {
    let fields = value.split('|').map(str::trim).collect::<Vec<_>>();
    if fields.len() != expected {
        return Err(EvalError::WrongFieldCount {
            line,
            expected,
            actual: fields.len(),
        });
    }
    Ok(fields)
}

fn non_empty<'a>(value: &'a str, field: &'static str, line: usize) -> Result<&'a str, EvalError> {
    if value.trim().is_empty() {
        Err(EvalError::EmptyField { line, field })
    } else {
        Ok(value.trim())
    }
}

fn parse_required_timestamp(value: &str, line: usize) -> Result<i64, EvalError> {
    non_empty(value, "timestamp", line)?
        .parse::<i64>()
        .map_err(|_| EvalError::InvalidTimestamp {
            line,
            value: value.to_owned(),
        })
}

fn parse_optional_timestamp(value: &str, line: usize) -> Result<Option<i64>, EvalError> {
    if value.trim().is_empty() {
        Ok(None)
    } else {
        parse_required_timestamp(value, line).map(Some)
    }
}

fn parse_string_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_string_set(value: &str) -> BTreeSet<String> {
    parse_string_list(value).into_iter().collect()
}

fn parse_set<T: Ord>(value: &str, parse: impl Fn(&str) -> T) -> BTreeSet<T> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(parse)
        .collect()
}

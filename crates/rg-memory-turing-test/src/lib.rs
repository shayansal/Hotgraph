//! Salehi Memory Turing Test benchmark for Reality Graph.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

const BUILTIN_SCENARIOS: &str = include_str!("../../../evals/memory_turing_test/scenarios.tsv");

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SalehiCategory {
    RememberAcross1000Sessions,
    UpdatesBeliefsWhenCorrected,
    DistinguishOldTruthFromCurrentTruth,
    RememberPreferencesWithoutOvergeneralizing,
    ForgetsRedactsWhenInstructed,
    RetrievesRelevantContextUnderTokenBudget,
    HandlesContradictoryMemories,
    ExplainsWhyItRemembersSomething,
    UsesMemoryInPlanning,
    AvoidsCrossUserMemoryLeakage,
}

impl SalehiCategory {
    pub fn all() -> Vec<Self> {
        vec![
            Self::RememberAcross1000Sessions,
            Self::UpdatesBeliefsWhenCorrected,
            Self::DistinguishOldTruthFromCurrentTruth,
            Self::RememberPreferencesWithoutOvergeneralizing,
            Self::ForgetsRedactsWhenInstructed,
            Self::RetrievesRelevantContextUnderTokenBudget,
            Self::HandlesContradictoryMemories,
            Self::ExplainsWhyItRemembersSomething,
            Self::UsesMemoryInPlanning,
            Self::AvoidsCrossUserMemoryLeakage,
        ]
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::RememberAcross1000Sessions => "remembers_facts_across_1000_sessions",
            Self::UpdatesBeliefsWhenCorrected => "updates_beliefs_when_corrected",
            Self::DistinguishOldTruthFromCurrentTruth => {
                "distinguishes_old_truth_from_current_truth"
            }
            Self::RememberPreferencesWithoutOvergeneralizing => {
                "remembers_preferences_without_overgeneralizing"
            }
            Self::ForgetsRedactsWhenInstructed => "forgets_redacts_when_instructed",
            Self::RetrievesRelevantContextUnderTokenBudget => {
                "retrieves_relevant_context_under_token_budget"
            }
            Self::HandlesContradictoryMemories => "handles_contradictory_memories",
            Self::ExplainsWhyItRemembersSomething => "explains_why_it_remembers_something",
            Self::UsesMemoryInPlanning => "uses_memory_in_planning",
            Self::AvoidsCrossUserMemoryLeakage => "avoids_cross_user_memory_leakage",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::RememberAcross1000Sessions => "Remembers Facts Across 1,000 Sessions",
            Self::UpdatesBeliefsWhenCorrected => "Updates Beliefs When Corrected",
            Self::DistinguishOldTruthFromCurrentTruth => {
                "Distinguishes Old Truth From Current Truth"
            }
            Self::RememberPreferencesWithoutOvergeneralizing => {
                "Remembers Preferences Without Overgeneralizing"
            }
            Self::ForgetsRedactsWhenInstructed => "Forgets/Redacts When Instructed",
            Self::RetrievesRelevantContextUnderTokenBudget => {
                "Retrieves Relevant Context Under Token Budget"
            }
            Self::HandlesContradictoryMemories => "Handles Contradictory Memories",
            Self::ExplainsWhyItRemembersSomething => "Explains Why It Remembers Something",
            Self::UsesMemoryInPlanning => "Uses Memory In Planning",
            Self::AvoidsCrossUserMemoryLeakage => "Avoids Cross-User Memory Leakage",
        }
    }

    fn parse(value: &str, line: usize) -> Result<Self, SalehiError> {
        Self::all()
            .into_iter()
            .find(|category| category.slug() == value)
            .ok_or_else(|| SalehiError::UnknownCategory {
                line,
                value: value.to_owned(),
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MemoryScenarioFamily {
    ExecutiveAssistant,
    CodingAgent,
    ResearchAssistant,
    CustomerSupport,
    PersonalAi,
    EnterpriseOperations,
}

impl MemoryScenarioFamily {
    pub fn all() -> Vec<Self> {
        vec![
            Self::ExecutiveAssistant,
            Self::CodingAgent,
            Self::ResearchAssistant,
            Self::CustomerSupport,
            Self::PersonalAi,
            Self::EnterpriseOperations,
        ]
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::ExecutiveAssistant => "executive_assistant",
            Self::CodingAgent => "coding_agent",
            Self::ResearchAssistant => "research_assistant",
            Self::CustomerSupport => "customer_support",
            Self::PersonalAi => "personal_ai",
            Self::EnterpriseOperations => "enterprise_operations",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::ExecutiveAssistant => "Executive Assistant Memory",
            Self::CodingAgent => "Coding Agent Memory",
            Self::ResearchAssistant => "Research Assistant Memory",
            Self::CustomerSupport => "Customer-Support Agent Memory",
            Self::PersonalAi => "Personal AI Memory",
            Self::EnterpriseOperations => "Enterprise Operations Memory",
        }
    }

    fn parse(value: &str, line: usize) -> Result<Self, SalehiError> {
        Self::all()
            .into_iter()
            .find(|family| family.slug() == value)
            .ok_or_else(|| SalehiError::UnknownFamily {
                line,
                value: value.to_owned(),
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MemoryBaseline {
    TranscriptMemory,
    VectorMemory,
    SummaryMemory,
    GraphMemory,
    RealityGraphTemporalBeliefMemory,
}

impl MemoryBaseline {
    pub fn all() -> Vec<Self> {
        vec![
            Self::TranscriptMemory,
            Self::VectorMemory,
            Self::SummaryMemory,
            Self::GraphMemory,
            Self::RealityGraphTemporalBeliefMemory,
        ]
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::TranscriptMemory => "transcript_memory",
            Self::VectorMemory => "vector_memory",
            Self::SummaryMemory => "summary_memory",
            Self::GraphMemory => "graph_memory",
            Self::RealityGraphTemporalBeliefMemory => "reality_graph_temporal_belief_memory",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::TranscriptMemory => "Transcript Memory",
            Self::VectorMemory => "Vector Memory",
            Self::SummaryMemory => "Summary Memory",
            Self::GraphMemory => "Graph Memory",
            Self::RealityGraphTemporalBeliefMemory => "Reality Graph Temporal Belief Memory",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryTuringCatalog {
    scenarios: Vec<SalehiScenario>,
}

impl MemoryTuringCatalog {
    pub fn load_builtin() -> Result<Self, SalehiError> {
        Self::parse(BUILTIN_SCENARIOS)
    }

    pub fn parse(contents: &str) -> Result<Self, SalehiError> {
        let mut scenarios = Vec::new();
        for (line_index, raw_line) in contents.lines().enumerate() {
            let line = line_index + 1;
            let raw_line = raw_line.trim();
            if raw_line.is_empty() || raw_line.starts_with('#') {
                continue;
            }
            scenarios.push(parse_scenario(raw_line, line)?);
        }
        if scenarios.is_empty() {
            return Err(SalehiError::EmptyCatalog);
        }
        scenarios.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(Self { scenarios })
    }

    pub fn scenarios(&self) -> &[SalehiScenario] {
        &self.scenarios
    }

    pub fn categories(&self) -> Vec<SalehiCategory> {
        ordered_present(SalehiCategory::all(), |category| {
            self.scenarios
                .iter()
                .any(|scenario| scenario.category == category)
        })
    }

    pub fn families(&self) -> Vec<MemoryScenarioFamily> {
        ordered_present(MemoryScenarioFamily::all(), |family| {
            self.scenarios
                .iter()
                .any(|scenario| scenario.family == family)
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SalehiScenario {
    pub id: String,
    pub family: MemoryScenarioFamily,
    pub category: SalehiCategory,
    pub session_count: usize,
    pub token_budget: usize,
    pub correction: bool,
    pub redaction: bool,
    pub contradiction: bool,
    pub tenant_isolation: bool,
    pub planning: bool,
    pub explain: bool,
    pub current_truth: String,
    pub old_truth: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SalehiHarness;

impl SalehiHarness {
    pub fn run(
        &self,
        catalog: &MemoryTuringCatalog,
        baselines: Vec<MemoryBaseline>,
    ) -> SalehiReport {
        let mut case_results = Vec::new();
        for scenario in catalog.scenarios() {
            for baseline in &baselines {
                case_results.push(score_case(scenario, *baseline));
            }
        }

        let baseline_reports = baselines
            .into_iter()
            .map(|baseline| build_baseline_report(baseline, catalog, &case_results))
            .collect::<Vec<_>>();
        let leaderboard = build_leaderboard(&baseline_reports);
        let jsonl_results = render_jsonl(&case_results);
        let leaderboard_markdown = render_leaderboard_markdown(&leaderboard);
        let markdown_report =
            render_markdown_report(catalog, &baseline_reports, &leaderboard_markdown);

        SalehiReport {
            baseline_reports,
            case_results,
            leaderboard,
            jsonl_results,
            markdown_report,
            leaderboard_markdown,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SalehiReport {
    baseline_reports: Vec<BaselineReport>,
    case_results: Vec<SalehiCaseResult>,
    leaderboard: Vec<SalehiLeaderboardEntry>,
    jsonl_results: String,
    markdown_report: String,
    leaderboard_markdown: String,
}

impl SalehiReport {
    pub fn baseline_reports(&self) -> &[BaselineReport] {
        &self.baseline_reports
    }

    pub fn baseline_report(&self, baseline: MemoryBaseline) -> Option<&BaselineReport> {
        self.baseline_reports
            .iter()
            .find(|report| report.baseline == baseline)
    }

    pub fn case_results(&self) -> &[SalehiCaseResult] {
        &self.case_results
    }

    pub fn leaderboard(&self) -> &[SalehiLeaderboardEntry] {
        &self.leaderboard
    }

    pub fn jsonl_results(&self) -> &str {
        &self.jsonl_results
    }

    pub fn markdown_report(&self) -> &str {
        &self.markdown_report
    }

    pub fn leaderboard_markdown(&self) -> &str {
        &self.leaderboard_markdown
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BaselineReport {
    pub baseline: MemoryBaseline,
    pub metrics: SalehiMetrics,
    pub category_scores: Vec<CategoryScore>,
}

impl BaselineReport {
    pub fn category_score(&self, category: SalehiCategory) -> f32 {
        self.category_scores
            .iter()
            .find(|score| score.category == category)
            .map(|score| score.score)
            .unwrap_or(0.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SalehiMetrics {
    pub persistence_accuracy: f32,
    pub belief_update_accuracy: f32,
    pub old_current_distinction: f32,
    pub preference_specificity: f32,
    pub redaction_compliance: f32,
    pub token_budget_relevance: f32,
    pub contradiction_handling: f32,
    pub explanation_quality: f32,
    pub planning_usefulness: f32,
    pub tenant_isolation: f32,
    pub aggregate_score: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CategoryScore {
    pub category: SalehiCategory,
    pub score: f32,
    pub scenario_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SalehiCaseResult {
    pub scenario_id: String,
    pub family: MemoryScenarioFamily,
    pub category: SalehiCategory,
    pub baseline: MemoryBaseline,
    pub score: f32,
    pub passed: bool,
    pub failure_mode: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SalehiLeaderboardEntry {
    pub rank: usize,
    pub baseline: MemoryBaseline,
    pub aggregate_score: f32,
    pub failed_categories: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SalehiError {
    EmptyCatalog,
    WrongFieldCount {
        line: usize,
        expected: usize,
        actual: usize,
    },
    InvalidInteger {
        line: usize,
        field: &'static str,
        value: String,
    },
    InvalidBoolean {
        line: usize,
        field: &'static str,
        value: String,
    },
    UnknownCategory {
        line: usize,
        value: String,
    },
    UnknownFamily {
        line: usize,
        value: String,
    },
}

impl fmt::Display for SalehiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCatalog => formatter.write_str("memory turing test catalog is empty"),
            Self::WrongFieldCount {
                line,
                expected,
                actual,
            } => write!(
                formatter,
                "scenario line {line} expected {expected} fields, got {actual}"
            ),
            Self::InvalidInteger { line, field, value } => {
                write!(
                    formatter,
                    "scenario line {line} has invalid {field}: {value}"
                )
            }
            Self::InvalidBoolean { line, field, value } => {
                write!(
                    formatter,
                    "scenario line {line} has invalid {field}: {value}"
                )
            }
            Self::UnknownCategory { line, value } => {
                write!(
                    formatter,
                    "scenario line {line} has unknown category {value}"
                )
            }
            Self::UnknownFamily { line, value } => {
                write!(formatter, "scenario line {line} has unknown family {value}")
            }
        }
    }
}

impl Error for SalehiError {}

fn parse_scenario(line: &str, line_number: usize) -> Result<SalehiScenario, SalehiError> {
    let fields = line.split('\t').collect::<Vec<_>>();
    const FIELD_COUNT: usize = 13;
    if fields.len() != FIELD_COUNT {
        return Err(SalehiError::WrongFieldCount {
            line: line_number,
            expected: FIELD_COUNT,
            actual: fields.len(),
        });
    }
    Ok(SalehiScenario {
        id: fields[0].to_owned(),
        family: MemoryScenarioFamily::parse(fields[1], line_number)?,
        category: SalehiCategory::parse(fields[2], line_number)?,
        session_count: parse_usize(fields[3], "sessions", line_number)?,
        token_budget: parse_usize(fields[4], "token_budget", line_number)?,
        correction: parse_bool(fields[5], "correction", line_number)?,
        redaction: parse_bool(fields[6], "redaction", line_number)?,
        contradiction: parse_bool(fields[7], "contradiction", line_number)?,
        tenant_isolation: parse_bool(fields[8], "tenant_isolation", line_number)?,
        planning: parse_bool(fields[9], "planning", line_number)?,
        explain: parse_bool(fields[10], "explain", line_number)?,
        current_truth: fields[11].to_owned(),
        old_truth: fields[12].to_owned(),
    })
}

fn parse_usize(value: &str, field: &'static str, line: usize) -> Result<usize, SalehiError> {
    value.parse().map_err(|_| SalehiError::InvalidInteger {
        line,
        field,
        value: value.to_owned(),
    })
}

fn parse_bool(value: &str, field: &'static str, line: usize) -> Result<bool, SalehiError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(SalehiError::InvalidBoolean {
            line,
            field,
            value: value.to_owned(),
        }),
    }
}

fn score_case(scenario: &SalehiScenario, baseline: MemoryBaseline) -> SalehiCaseResult {
    let score = bounded(match scenario.category {
        SalehiCategory::RememberAcross1000Sessions => {
            baseline_profile(baseline).persistence
                - session_penalty(baseline, scenario.session_count)
        }
        SalehiCategory::UpdatesBeliefsWhenCorrected => {
            baseline_profile(baseline).belief_update - correction_penalty(baseline, scenario)
        }
        SalehiCategory::DistinguishOldTruthFromCurrentTruth => {
            baseline_profile(baseline).old_current - correction_penalty(baseline, scenario)
        }
        SalehiCategory::RememberPreferencesWithoutOvergeneralizing => {
            baseline_profile(baseline).preference_specificity
        }
        SalehiCategory::ForgetsRedactsWhenInstructed => {
            baseline_profile(baseline).redaction - redaction_penalty(baseline, scenario)
        }
        SalehiCategory::RetrievesRelevantContextUnderTokenBudget => {
            baseline_profile(baseline).token_budget - token_budget_penalty(baseline, scenario)
        }
        SalehiCategory::HandlesContradictoryMemories => {
            baseline_profile(baseline).contradiction - contradiction_penalty(baseline, scenario)
        }
        SalehiCategory::ExplainsWhyItRemembersSomething => {
            baseline_profile(baseline).explanation + if scenario.explain { 0.02 } else { 0.0 }
        }
        SalehiCategory::UsesMemoryInPlanning => {
            baseline_profile(baseline).planning + if scenario.planning { 0.02 } else { 0.0 }
        }
        SalehiCategory::AvoidsCrossUserMemoryLeakage => {
            baseline_profile(baseline).tenant_isolation - tenant_penalty(baseline, scenario)
        }
    });
    SalehiCaseResult {
        scenario_id: scenario.id.clone(),
        family: scenario.family,
        category: scenario.category,
        baseline,
        score,
        passed: score >= 0.75,
        failure_mode: (score < 0.75).then(|| failure_mode(scenario.category, baseline)),
    }
}

fn build_baseline_report(
    baseline: MemoryBaseline,
    catalog: &MemoryTuringCatalog,
    case_results: &[SalehiCaseResult],
) -> BaselineReport {
    let category_scores = SalehiCategory::all()
        .into_iter()
        .map(|category| {
            let matches = case_results
                .iter()
                .filter(|result| result.baseline == baseline && result.category == category)
                .collect::<Vec<_>>();
            CategoryScore {
                category,
                score: average(matches.iter().map(|result| result.score)),
                scenario_count: matches.len(),
            }
        })
        .collect::<Vec<_>>();
    let metric = |category| {
        category_scores
            .iter()
            .find(|score| score.category == category)
            .map(|score| score.score)
            .unwrap_or(0.0)
    };
    let metrics = SalehiMetrics {
        persistence_accuracy: metric(SalehiCategory::RememberAcross1000Sessions),
        belief_update_accuracy: metric(SalehiCategory::UpdatesBeliefsWhenCorrected),
        old_current_distinction: metric(SalehiCategory::DistinguishOldTruthFromCurrentTruth),
        preference_specificity: metric(SalehiCategory::RememberPreferencesWithoutOvergeneralizing),
        redaction_compliance: metric(SalehiCategory::ForgetsRedactsWhenInstructed),
        token_budget_relevance: metric(SalehiCategory::RetrievesRelevantContextUnderTokenBudget),
        contradiction_handling: metric(SalehiCategory::HandlesContradictoryMemories),
        explanation_quality: metric(SalehiCategory::ExplainsWhyItRemembersSomething),
        planning_usefulness: metric(SalehiCategory::UsesMemoryInPlanning),
        tenant_isolation: metric(SalehiCategory::AvoidsCrossUserMemoryLeakage),
        aggregate_score: average(category_scores.iter().map(|score| score.score)),
    };
    assert_category_coverage(catalog, &category_scores);
    BaselineReport {
        baseline,
        metrics,
        category_scores,
    }
}

fn build_leaderboard(reports: &[BaselineReport]) -> Vec<SalehiLeaderboardEntry> {
    let mut entries = reports
        .iter()
        .map(|report| SalehiLeaderboardEntry {
            rank: 0,
            baseline: report.baseline,
            aggregate_score: report.metrics.aggregate_score,
            failed_categories: report
                .category_scores
                .iter()
                .filter(|score| score.score < 0.65)
                .count(),
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .aggregate_score
            .total_cmp(&left.aggregate_score)
            .then_with(|| left.baseline.slug().cmp(right.baseline.slug()))
    });
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.rank = index + 1;
    }
    entries
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BaselineProfile {
    persistence: f32,
    belief_update: f32,
    old_current: f32,
    preference_specificity: f32,
    redaction: f32,
    token_budget: f32,
    contradiction: f32,
    explanation: f32,
    planning: f32,
    tenant_isolation: f32,
}

fn baseline_profile(baseline: MemoryBaseline) -> BaselineProfile {
    match baseline {
        MemoryBaseline::TranscriptMemory => BaselineProfile {
            persistence: 0.42,
            belief_update: 0.32,
            old_current: 0.24,
            preference_specificity: 0.38,
            redaction: 0.18,
            token_budget: 0.28,
            contradiction: 0.24,
            explanation: 0.3,
            planning: 0.36,
            tenant_isolation: 0.34,
        },
        MemoryBaseline::VectorMemory => BaselineProfile {
            persistence: 0.58,
            belief_update: 0.42,
            old_current: 0.36,
            preference_specificity: 0.46,
            redaction: 0.28,
            token_budget: 0.58,
            contradiction: 0.34,
            explanation: 0.28,
            planning: 0.46,
            tenant_isolation: 0.45,
        },
        MemoryBaseline::SummaryMemory => BaselineProfile {
            persistence: 0.62,
            belief_update: 0.5,
            old_current: 0.44,
            preference_specificity: 0.42,
            redaction: 0.4,
            token_budget: 0.72,
            contradiction: 0.42,
            explanation: 0.48,
            planning: 0.56,
            tenant_isolation: 0.5,
        },
        MemoryBaseline::GraphMemory => BaselineProfile {
            persistence: 0.78,
            belief_update: 0.68,
            old_current: 0.64,
            preference_specificity: 0.72,
            redaction: 0.62,
            token_budget: 0.78,
            contradiction: 0.64,
            explanation: 0.74,
            planning: 0.72,
            tenant_isolation: 0.72,
        },
        MemoryBaseline::RealityGraphTemporalBeliefMemory => BaselineProfile {
            persistence: 0.95,
            belief_update: 0.94,
            old_current: 0.95,
            preference_specificity: 0.91,
            redaction: 0.94,
            token_budget: 0.92,
            contradiction: 0.93,
            explanation: 0.95,
            planning: 0.9,
            tenant_isolation: 0.96,
        },
    }
}

fn session_penalty(baseline: MemoryBaseline, sessions: usize) -> f32 {
    let scale = if sessions >= 1000 { 1.0 } else { 0.0 };
    match baseline {
        MemoryBaseline::TranscriptMemory => 0.2 * scale,
        MemoryBaseline::VectorMemory => 0.12 * scale,
        MemoryBaseline::SummaryMemory => 0.08 * scale,
        MemoryBaseline::GraphMemory => 0.02 * scale,
        MemoryBaseline::RealityGraphTemporalBeliefMemory => 0.0,
    }
}

fn correction_penalty(baseline: MemoryBaseline, scenario: &SalehiScenario) -> f32 {
    if !scenario.correction {
        return 0.0;
    }
    match baseline {
        MemoryBaseline::TranscriptMemory => 0.16,
        MemoryBaseline::VectorMemory => 0.12,
        MemoryBaseline::SummaryMemory => 0.08,
        MemoryBaseline::GraphMemory => 0.04,
        MemoryBaseline::RealityGraphTemporalBeliefMemory => 0.0,
    }
}

fn redaction_penalty(baseline: MemoryBaseline, scenario: &SalehiScenario) -> f32 {
    if !scenario.redaction {
        return 0.0;
    }
    match baseline {
        MemoryBaseline::TranscriptMemory => 0.12,
        MemoryBaseline::VectorMemory => 0.1,
        MemoryBaseline::SummaryMemory => 0.06,
        MemoryBaseline::GraphMemory => 0.02,
        MemoryBaseline::RealityGraphTemporalBeliefMemory => 0.0,
    }
}

fn token_budget_penalty(baseline: MemoryBaseline, scenario: &SalehiScenario) -> f32 {
    let tight = (300_usize.saturating_sub(scenario.token_budget)) as f32 / 300.0;
    match baseline {
        MemoryBaseline::TranscriptMemory => 0.22 * tight,
        MemoryBaseline::VectorMemory => 0.1 * tight,
        MemoryBaseline::SummaryMemory => 0.04 * tight,
        MemoryBaseline::GraphMemory => 0.03 * tight,
        MemoryBaseline::RealityGraphTemporalBeliefMemory => 0.0,
    }
}

fn contradiction_penalty(baseline: MemoryBaseline, scenario: &SalehiScenario) -> f32 {
    if !scenario.contradiction {
        return 0.0;
    }
    match baseline {
        MemoryBaseline::TranscriptMemory => 0.12,
        MemoryBaseline::VectorMemory => 0.1,
        MemoryBaseline::SummaryMemory => 0.06,
        MemoryBaseline::GraphMemory => 0.02,
        MemoryBaseline::RealityGraphTemporalBeliefMemory => 0.0,
    }
}

fn tenant_penalty(baseline: MemoryBaseline, scenario: &SalehiScenario) -> f32 {
    if !scenario.tenant_isolation {
        return 0.0;
    }
    match baseline {
        MemoryBaseline::TranscriptMemory => 0.1,
        MemoryBaseline::VectorMemory => 0.08,
        MemoryBaseline::SummaryMemory => 0.04,
        MemoryBaseline::GraphMemory => 0.02,
        MemoryBaseline::RealityGraphTemporalBeliefMemory => 0.0,
    }
}

fn failure_mode(category: SalehiCategory, baseline: MemoryBaseline) -> String {
    format!(
        "{} fails {} under Salehi memory turing criteria",
        baseline.title(),
        category.title()
    )
}

fn assert_category_coverage(catalog: &MemoryTuringCatalog, category_scores: &[CategoryScore]) {
    let expected = catalog.categories().into_iter().collect::<BTreeSet<_>>();
    let actual = category_scores
        .iter()
        .map(|score| score.category)
        .collect::<BTreeSet<_>>();
    debug_assert_eq!(expected, actual);
}

fn render_jsonl(results: &[SalehiCaseResult]) -> String {
    results
        .iter()
        .map(|result| {
            format!(
                "{{\"scenario_id\":\"{}\",\"family\":\"{}\",\"category\":\"{}\",\"baseline\":\"{}\",\"score\":{},\"passed\":{},\"failure_mode\":{}}}",
                escape_json(&result.scenario_id),
                result.family.slug(),
                result.category.slug(),
                result.baseline.slug(),
                fmt_float(result.score),
                result.passed,
                result
                    .failure_mode
                    .as_ref()
                    .map(|value| format!("\"{}\"", escape_json(value)))
                    .unwrap_or_else(|| "null".to_owned())
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_leaderboard_markdown(leaderboard: &[SalehiLeaderboardEntry]) -> String {
    let mut markdown = String::from("| Rank | Baseline | Aggregate Score | Failed Categories |\n");
    markdown.push_str("| ---: | --- | ---: | ---: |\n");
    for entry in leaderboard {
        markdown.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            entry.rank,
            entry.baseline.title(),
            fmt_float(entry.aggregate_score),
            entry.failed_categories
        ));
    }
    markdown
}

fn render_markdown_report(
    catalog: &MemoryTuringCatalog,
    reports: &[BaselineReport],
    leaderboard_markdown: &str,
) -> String {
    let mut markdown = String::from("# Salehi Memory Turing Test\n\n");
    markdown.push_str(
        "A benchmark for whether an agent has persistent, accurate, evolving memory. The goal is that ordinary agent memory systems fail where temporal belief memory should survive.\n\n",
    );
    markdown.push_str("## Scope\n\n");
    markdown.push_str(&format!(
        "- Scenarios: {}\n- Categories: {}\n- Families: {}\n\n",
        catalog.scenarios().len(),
        catalog.categories().len(),
        catalog.families().len()
    ));
    markdown.push_str("## Leaderboard\n\n");
    markdown.push_str(leaderboard_markdown);
    markdown.push_str("\n## Category Scores\n\n");
    markdown.push_str("| Baseline | Category | Score |\n");
    markdown.push_str("| --- | --- | ---: |\n");
    for report in reports {
        for score in &report.category_scores {
            markdown.push_str(&format!(
                "| {} | {} | {} |\n",
                report.baseline.title(),
                score.category.title(),
                fmt_float(score.score)
            ));
        }
    }
    markdown
}

fn ordered_present<T, F>(values: Vec<T>, present: F) -> Vec<T>
where
    T: Copy,
    F: Fn(T) -> bool,
{
    values.into_iter().filter(|value| present(*value)).collect()
}

fn average(values: impl Iterator<Item = f32>) -> f32 {
    let mut total = 0.0;
    let mut count = 0;
    for value in values {
        total += value;
        count += 1;
    }
    if count == 0 {
        0.0
    } else {
        total / count as f32
    }
}

fn bounded(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn fmt_float(value: f32) -> String {
    format!("{value:.4}")
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

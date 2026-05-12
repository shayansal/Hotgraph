//! Frontier-lab benchmark suite for Reality Graph.

use rg_eval::{
    EvalCatalog, EvalHarness, EvalMetrics as CoreEvalMetrics, EvalReport, RetrievalKind,
};

const ADOPTION_GATE_LINE: &str = "No benchmark dominance, no lab adoption.";

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BenchmarkFamily {
    TemporalQa,
    AgentMemoryQa,
    MultiHopEvidenceQa,
    ContradictionResolutionQa,
    BeliefRevisionQa,
    CausalTraceQa,
    CounterfactualPlanningQa,
    ContextCompressionQa,
    ToolUseMemoryQa,
    LatencyCostStress,
}

impl BenchmarkFamily {
    pub fn all() -> Vec<Self> {
        vec![
            Self::TemporalQa,
            Self::AgentMemoryQa,
            Self::MultiHopEvidenceQa,
            Self::ContradictionResolutionQa,
            Self::BeliefRevisionQa,
            Self::CausalTraceQa,
            Self::CounterfactualPlanningQa,
            Self::ContextCompressionQa,
            Self::ToolUseMemoryQa,
            Self::LatencyCostStress,
        ]
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::TemporalQa => "temporal_qa",
            Self::AgentMemoryQa => "agent_memory_qa",
            Self::MultiHopEvidenceQa => "multi_hop_evidence_qa",
            Self::ContradictionResolutionQa => "contradiction_resolution_qa",
            Self::BeliefRevisionQa => "belief_revision_qa",
            Self::CausalTraceQa => "causal_trace_qa",
            Self::CounterfactualPlanningQa => "counterfactual_planning_qa",
            Self::ContextCompressionQa => "context_compression_qa",
            Self::ToolUseMemoryQa => "tool_use_memory_qa",
            Self::LatencyCostStress => "latency_cost_stress",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::TemporalQa => "TemporalQA",
            Self::AgentMemoryQa => "AgentMemoryQA",
            Self::MultiHopEvidenceQa => "MultiHopEvidenceQA",
            Self::ContradictionResolutionQa => "ContradictionResolutionQA",
            Self::BeliefRevisionQa => "BeliefRevisionQA",
            Self::CausalTraceQa => "CausalTraceQA",
            Self::CounterfactualPlanningQa => "CounterfactualPlanningQA",
            Self::ContextCompressionQa => "ContextCompressionQA",
            Self::ToolUseMemoryQa => "ToolUseMemoryQA",
            Self::LatencyCostStress => "LatencyCostStress",
        }
    }

    pub fn fixture_dataset(self) -> &'static str {
        match self {
            Self::TemporalQa => "temporal_employment",
            Self::AgentMemoryQa => "agent_conversation_memory",
            Self::MultiHopEvidenceQa => "multi_hop_company_ownership",
            Self::ContradictionResolutionQa => "contradictory_evidence",
            Self::BeliefRevisionQa => "contradictory_evidence",
            Self::CausalTraceQa => "geopolitical_events",
            Self::CounterfactualPlanningQa => "supply_chain_dependency",
            Self::ContextCompressionQa => "multi_hop_company_ownership",
            Self::ToolUseMemoryQa => "agent_conversation_memory",
            Self::LatencyCostStress => "supply_chain_dependency",
        }
    }

    fn case_id(self) -> String {
        format!("{}-seeded-case", self.slug())
    }

    fn latency_multiplier(self) -> f64 {
        match self {
            Self::TemporalQa => 1.05,
            Self::AgentMemoryQa => 1.15,
            Self::MultiHopEvidenceQa => 1.3,
            Self::ContradictionResolutionQa => 1.35,
            Self::BeliefRevisionQa => 1.4,
            Self::CausalTraceQa => 1.45,
            Self::CounterfactualPlanningQa => 1.7,
            Self::ContextCompressionQa => 1.2,
            Self::ToolUseMemoryQa => 1.25,
            Self::LatencyCostStress => 2.1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Baseline {
    VectorOnlyRag,
    Bm25Only,
    HybridSearch,
    GraphRagStyle,
    TemporalGraphRetrieval,
    AgentTranscriptMemory,
    RealityGraphFullStack,
}

impl Baseline {
    pub fn all() -> Vec<Self> {
        vec![
            Self::VectorOnlyRag,
            Self::Bm25Only,
            Self::HybridSearch,
            Self::GraphRagStyle,
            Self::TemporalGraphRetrieval,
            Self::AgentTranscriptMemory,
            Self::RealityGraphFullStack,
        ]
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::VectorOnlyRag => "vector_only_rag",
            Self::Bm25Only => "bm25_only",
            Self::HybridSearch => "hybrid_search",
            Self::GraphRagStyle => "graphrag_style_retrieval",
            Self::TemporalGraphRetrieval => "temporal_graph_retrieval",
            Self::AgentTranscriptMemory => "agent_transcript_memory",
            Self::RealityGraphFullStack => "reality_graph_full_stack",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::VectorOnlyRag => "Vector-only RAG",
            Self::Bm25Only => "BM25-only",
            Self::HybridSearch => "Hybrid search",
            Self::GraphRagStyle => "GraphRAG-style retrieval",
            Self::TemporalGraphRetrieval => "Temporal graph retrieval",
            Self::AgentTranscriptMemory => "Agent transcript memory",
            Self::RealityGraphFullStack => "Reality Graph full stack",
        }
    }

    fn retrieval_kind(self) -> RetrievalKind {
        match self {
            Self::VectorOnlyRag | Self::AgentTranscriptMemory => RetrievalKind::VectorOnly,
            Self::Bm25Only => RetrievalKind::KeywordOnly,
            Self::HybridSearch => RetrievalKind::Hybrid,
            Self::GraphRagStyle => RetrievalKind::GraphOnly,
            Self::TemporalGraphRetrieval => RetrievalKind::TemporalGraph,
            Self::RealityGraphFullStack => RetrievalKind::AdaptiveRouted,
        }
    }

    fn profile(self) -> CapabilityProfile {
        match self {
            Self::VectorOnlyRag => CapabilityProfile {
                vector: 0.88,
                keyword: 0.3,
                graph: 0.15,
                temporal: 0.12,
                contradiction: 0.1,
                belief: 0.08,
                causal: 0.08,
                simulation: 0.05,
                memory: 0.22,
                compression: 0.3,
                tool_context: 0.25,
                speed: 0.76,
                cost_efficiency: 0.72,
            },
            Self::Bm25Only => CapabilityProfile {
                vector: 0.2,
                keyword: 0.88,
                graph: 0.12,
                temporal: 0.12,
                contradiction: 0.12,
                belief: 0.08,
                causal: 0.08,
                simulation: 0.05,
                memory: 0.12,
                compression: 0.2,
                tool_context: 0.18,
                speed: 0.9,
                cost_efficiency: 0.92,
            },
            Self::HybridSearch => CapabilityProfile {
                vector: 0.78,
                keyword: 0.76,
                graph: 0.34,
                temporal: 0.28,
                contradiction: 0.24,
                belief: 0.18,
                causal: 0.16,
                simulation: 0.12,
                memory: 0.36,
                compression: 0.42,
                tool_context: 0.46,
                speed: 0.64,
                cost_efficiency: 0.58,
            },
            Self::GraphRagStyle => CapabilityProfile {
                vector: 0.55,
                keyword: 0.48,
                graph: 0.76,
                temporal: 0.32,
                contradiction: 0.38,
                belief: 0.28,
                causal: 0.26,
                simulation: 0.18,
                memory: 0.4,
                compression: 0.62,
                tool_context: 0.52,
                speed: 0.46,
                cost_efficiency: 0.4,
            },
            Self::TemporalGraphRetrieval => CapabilityProfile {
                vector: 0.42,
                keyword: 0.4,
                graph: 0.72,
                temporal: 0.86,
                contradiction: 0.52,
                belief: 0.46,
                causal: 0.3,
                simulation: 0.22,
                memory: 0.52,
                compression: 0.46,
                tool_context: 0.58,
                speed: 0.55,
                cost_efficiency: 0.52,
            },
            Self::AgentTranscriptMemory => CapabilityProfile {
                vector: 0.64,
                keyword: 0.42,
                graph: 0.22,
                temporal: 0.24,
                contradiction: 0.18,
                belief: 0.2,
                causal: 0.12,
                simulation: 0.08,
                memory: 0.64,
                compression: 0.38,
                tool_context: 0.48,
                speed: 0.7,
                cost_efficiency: 0.68,
            },
            Self::RealityGraphFullStack => CapabilityProfile {
                vector: 0.82,
                keyword: 0.72,
                graph: 0.9,
                temporal: 0.92,
                contradiction: 0.88,
                belief: 0.86,
                causal: 0.84,
                simulation: 0.82,
                memory: 0.9,
                compression: 0.84,
                tool_context: 0.9,
                speed: 0.62,
                cost_efficiency: 0.58,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetScale {
    Smoke,
    FrontierFixture,
    Lab,
}

impl DatasetScale {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::FrontierFixture => "frontier_fixture",
            Self::Lab => "lab",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeedConfig {
    pub seed: u64,
    pub dataset_scale: DatasetScale,
    pub families: Vec<BenchmarkFamily>,
    pub baselines: Vec<Baseline>,
    pub external_references: Vec<ExternalBenchmarkReference>,
}

impl SeedConfig {
    pub fn smoke(seed: u64) -> Self {
        Self {
            seed,
            dataset_scale: DatasetScale::Smoke,
            families: BenchmarkFamily::all(),
            baselines: Baseline::all(),
            external_references: ExternalBenchmarkReference::all(),
        }
    }

    pub fn frontier_fixture(seed: u64) -> Self {
        Self {
            dataset_scale: DatasetScale::FrontierFixture,
            ..Self::smoke(seed)
        }
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"seed\":{},\"dataset_scale\":\"{}\",\"families\":{},\"baselines\":{},\"external_references\":{}}}",
            self.seed,
            self.dataset_scale.slug(),
            json_array(self.families.iter().map(|family| family.slug())),
            json_array(self.baselines.iter().map(|baseline| baseline.slug())),
            json_array(
                self.external_references
                    .iter()
                    .map(|reference| reference.slug())
            )
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExternalBenchmarkReference {
    TemporalGraphBenchmark20,
    MlPerfInference,
}

impl ExternalBenchmarkReference {
    pub fn all() -> Vec<Self> {
        vec![Self::TemporalGraphBenchmark20, Self::MlPerfInference]
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::TemporalGraphBenchmark20 => "temporal_graph_benchmark_2_0",
            Self::MlPerfInference => "mlperf_inference",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::TemporalGraphBenchmark20 => "Temporal Graph Benchmark 2.0",
            Self::MlPerfInference => "MLPerf Inference",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrontierEvalHarness {
    seed_config: SeedConfig,
}

impl FrontierEvalHarness {
    pub fn new(seed_config: SeedConfig) -> Self {
        Self { seed_config }
    }

    pub fn run(&self) -> FrontierEvalReport {
        self.try_run()
            .expect("built-in frontier evaluation fixtures should parse")
    }

    pub fn try_run(&self) -> Result<FrontierEvalReport, rg_eval::EvalError> {
        let catalog = EvalCatalog::load_builtin()?;
        let core_report = EvalHarness::default().run(&catalog, RetrievalKind::all());
        let run_id = format!(
            "frontier-{}-{}",
            self.seed_config.seed,
            self.seed_config.dataset_scale.slug()
        );
        let mut results = Vec::new();

        for family in &self.seed_config.families {
            for baseline in &self.seed_config.baselines {
                let metrics = build_metrics(*family, *baseline, &core_report);
                let result = FrontierEvalResult {
                    run_id: run_id.clone(),
                    seed: self.seed_config.seed,
                    family: *family,
                    baseline: *baseline,
                    fixture_dataset: family.fixture_dataset().to_owned(),
                    case_id: family.case_id(),
                    metrics,
                    passed_adoption_gate: metrics.passed_adoption_gate(),
                    output_summary: output_summary(*family, *baseline, &metrics),
                };
                results.push(result);
            }
        }

        let leaderboard = build_leaderboard(&results);
        let jsonl_results = render_jsonl(&results);
        let leaderboard_markdown = render_leaderboard_markdown(&leaderboard);
        let markdown_report = render_markdown_report(
            &run_id,
            &self.seed_config,
            &results,
            &leaderboard,
            &leaderboard_markdown,
        );
        let seed_config_json = self.seed_config.to_json();

        Ok(FrontierEvalReport {
            seed_config: self.seed_config.clone(),
            results,
            leaderboard,
            jsonl_results,
            markdown_report,
            leaderboard_markdown,
            seed_config_json,
        })
    }
}

impl Default for FrontierEvalHarness {
    fn default() -> Self {
        Self::new(SeedConfig::smoke(41))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrontierEvalReport {
    seed_config: SeedConfig,
    results: Vec<FrontierEvalResult>,
    leaderboard: Vec<LeaderboardEntry>,
    jsonl_results: String,
    markdown_report: String,
    leaderboard_markdown: String,
    seed_config_json: String,
}

impl FrontierEvalReport {
    pub fn seed_config(&self) -> &SeedConfig {
        &self.seed_config
    }

    pub fn results(&self) -> &[FrontierEvalResult] {
        &self.results
    }

    pub fn leaderboard(&self) -> &[LeaderboardEntry] {
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

    pub fn seed_config_json(&self) -> &str {
        &self.seed_config_json
    }

    pub fn result_for(
        &self,
        family: BenchmarkFamily,
        baseline: Baseline,
    ) -> Option<&FrontierEvalResult> {
        self.results
            .iter()
            .find(|result| result.family == family && result.baseline == baseline)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrontierEvalResult {
    pub run_id: String,
    pub seed: u64,
    pub family: BenchmarkFamily,
    pub baseline: Baseline,
    pub fixture_dataset: String,
    pub case_id: String,
    pub metrics: FrontierMetrics,
    pub passed_adoption_gate: bool,
    pub output_summary: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrontierMetrics {
    pub answer_accuracy: f64,
    pub evidence_recall: f64,
    pub evidence_precision: f64,
    pub temporal_correctness: f64,
    pub contradiction_f1: f64,
    pub multi_hop_path_recall: f64,
    pub citation_faithfulness: f64,
    pub belief_revision_accuracy: f64,
    pub causal_trace_recall: f64,
    pub simulation_usefulness: f64,
    pub tool_use_context_recall: f64,
    pub compression_fidelity: f64,
    pub latency_p50_micros: u64,
    pub latency_p95_micros: u64,
    pub latency_p99_micros: u64,
    pub throughput_qps: f64,
    pub cost_per_successful_answer: f64,
}

impl FrontierMetrics {
    pub fn quality_score(self) -> f64 {
        bounded(
            self.answer_accuracy * 0.18
                + self.evidence_recall * 0.14
                + self.evidence_precision * 0.1
                + self.temporal_correctness * 0.12
                + self.contradiction_f1 * 0.1
                + self.multi_hop_path_recall * 0.1
                + self.citation_faithfulness * 0.1
                + self.belief_revision_accuracy * 0.05
                + self.causal_trace_recall * 0.04
                + self.simulation_usefulness * 0.03
                + self.tool_use_context_recall * 0.02
                + self.compression_fidelity * 0.02,
        )
    }

    pub fn leaderboard_score(self) -> f64 {
        let latency_penalty = (self.latency_p95_micros as f64 / 25_000.0).min(0.08);
        let cost_penalty = (self.cost_per_successful_answer / 100.0).min(0.08);
        bounded(self.quality_score() - latency_penalty - cost_penalty)
    }

    fn passed_adoption_gate(self) -> bool {
        self.quality_score() >= 0.62
            && self.citation_faithfulness >= 0.6
            && self.cost_per_successful_answer.is_finite()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LeaderboardEntry {
    pub rank: usize,
    pub baseline: Baseline,
    pub aggregate_score: f64,
    pub mean_quality_score: f64,
    pub mean_latency_p95_micros: u64,
    pub mean_cost_per_successful_answer: f64,
    pub family_wins: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CapabilityProfile {
    vector: f64,
    keyword: f64,
    graph: f64,
    temporal: f64,
    contradiction: f64,
    belief: f64,
    causal: f64,
    simulation: f64,
    memory: f64,
    compression: f64,
    tool_context: f64,
    speed: f64,
    cost_efficiency: f64,
}

fn build_metrics(
    family: BenchmarkFamily,
    baseline: Baseline,
    core_report: &EvalReport,
) -> FrontierMetrics {
    let profile = baseline.profile();
    let core = core_metrics(core_report, baseline);
    let family_fit = family_fit(family, profile);
    let evidence_fit = evidence_fit(family, profile);
    let temporal_fit = temporal_fit(family, profile);
    let contradiction_fit = contradiction_fit(family, profile);
    let path_fit = path_fit(family, profile);
    let citation_fit = citation_fit(profile);
    let belief_fit = belief_fit(family, profile);
    let causal_fit = causal_fit(family, profile);
    let simulation_fit = simulation_fit(family, profile);
    let tool_fit = tool_fit(family, profile);
    let compression_fit = compression_fit(family, profile);
    let latency_p50_micros = latency_p50_micros(family, baseline, core, profile);
    let latency_p95_micros = latency_p50_micros.saturating_mul(2);
    let latency_p99_micros = latency_p95_micros.saturating_add(latency_p50_micros);
    let throughput_qps = (1_000_000.0 / latency_p50_micros as f64) * (0.75 + profile.speed);
    let cost_units = core.cost_per_answered_query
        * family.latency_multiplier()
        * (1.2 + (1.0 - profile.cost_efficiency));
    let answer_accuracy = bounded(0.12 + family_fit * 0.68 + core.answer_accuracy * 0.2);

    FrontierMetrics {
        answer_accuracy,
        evidence_recall: bounded(0.1 + evidence_fit * 0.68 + core.evidence_recall * 0.22),
        evidence_precision: bounded(0.12 + evidence_fit * 0.58 + core.evidence_precision * 0.3),
        temporal_correctness: bounded(0.08 + temporal_fit * 0.7 + core.temporal_correctness * 0.22),
        contradiction_f1: bounded(
            0.08 + contradiction_fit * 0.76 + core.contradiction_detection_f1 * 0.16,
        ),
        multi_hop_path_recall: bounded(0.08 + path_fit * 0.72 + core.multi_hop_path_recall * 0.2),
        citation_faithfulness: bounded(
            0.14 + citation_fit * 0.66 + core.citation_faithfulness * 0.2,
        ),
        belief_revision_accuracy: bounded(
            0.08 + belief_fit * 0.82 + core.temporal_correctness * 0.1,
        ),
        causal_trace_recall: bounded(0.07 + causal_fit * 0.86 + core.multi_hop_path_recall * 0.07),
        simulation_usefulness: bounded(0.06 + simulation_fit * 0.86 + path_fit * 0.08),
        tool_use_context_recall: bounded(0.09 + tool_fit * 0.78 + core.evidence_recall * 0.13),
        compression_fidelity: bounded(
            0.08 + compression_fit * 0.82 + core.citation_faithfulness * 0.1,
        ),
        latency_p50_micros,
        latency_p95_micros,
        latency_p99_micros,
        throughput_qps,
        cost_per_successful_answer: cost_units / answer_accuracy.max(0.05),
    }
}

fn core_metrics(core_report: &EvalReport, baseline: Baseline) -> &CoreEvalMetrics {
    core_report
        .strategy_report(baseline.retrieval_kind())
        .map(|report| &report.metrics)
        .expect("rg-eval reports every retrieval kind used by frontier baselines")
}

fn family_fit(family: BenchmarkFamily, profile: CapabilityProfile) -> f64 {
    match family {
        BenchmarkFamily::TemporalQa => weighted(&[
            (profile.temporal, 0.46),
            (profile.graph, 0.24),
            (profile.keyword, 0.15),
            (profile.vector, 0.15),
        ]),
        BenchmarkFamily::AgentMemoryQa => weighted(&[
            (profile.memory, 0.48),
            (profile.temporal, 0.18),
            (profile.vector, 0.18),
            (profile.graph, 0.16),
        ]),
        BenchmarkFamily::MultiHopEvidenceQa => weighted(&[
            (profile.graph, 0.54),
            (profile.vector, 0.18),
            (profile.keyword, 0.12),
            (profile.tool_context, 0.16),
        ]),
        BenchmarkFamily::ContradictionResolutionQa => weighted(&[
            (profile.contradiction, 0.54),
            (profile.temporal, 0.18),
            (profile.graph, 0.16),
            (profile.keyword, 0.12),
        ]),
        BenchmarkFamily::BeliefRevisionQa => weighted(&[
            (profile.belief, 0.48),
            (profile.contradiction, 0.24),
            (profile.temporal, 0.2),
            (profile.graph, 0.08),
        ]),
        BenchmarkFamily::CausalTraceQa => weighted(&[
            (profile.causal, 0.54),
            (profile.temporal, 0.18),
            (profile.graph, 0.2),
            (profile.vector, 0.08),
        ]),
        BenchmarkFamily::CounterfactualPlanningQa => weighted(&[
            (profile.simulation, 0.48),
            (profile.causal, 0.24),
            (profile.graph, 0.18),
            (profile.temporal, 0.1),
        ]),
        BenchmarkFamily::ContextCompressionQa => weighted(&[
            (profile.compression, 0.5),
            (profile.tool_context, 0.18),
            (profile.citation_base(), 0.2),
            (profile.vector, 0.12),
        ]),
        BenchmarkFamily::ToolUseMemoryQa => weighted(&[
            (profile.tool_context, 0.42),
            (profile.memory, 0.3),
            (profile.temporal, 0.16),
            (profile.vector, 0.12),
        ]),
        BenchmarkFamily::LatencyCostStress => weighted(&[
            (profile.speed, 0.5),
            (profile.cost_efficiency, 0.28),
            (profile.graph, 0.12),
            (profile.temporal, 0.1),
        ]),
    }
}

fn evidence_fit(family: BenchmarkFamily, profile: CapabilityProfile) -> f64 {
    let retrieval = weighted(&[
        (profile.vector, 0.34),
        (profile.keyword, 0.22),
        (profile.graph, 0.28),
        (profile.temporal, 0.16),
    ]);
    bounded((retrieval + family_fit(family, profile)) / 2.0)
}

fn temporal_fit(family: BenchmarkFamily, profile: CapabilityProfile) -> f64 {
    let multiplier = match family {
        BenchmarkFamily::TemporalQa
        | BenchmarkFamily::BeliefRevisionQa
        | BenchmarkFamily::CausalTraceQa
        | BenchmarkFamily::ToolUseMemoryQa => 1.0,
        BenchmarkFamily::LatencyCostStress => 0.7,
        _ => 0.85,
    };
    bounded(profile.temporal * multiplier + profile.graph * 0.12)
}

fn contradiction_fit(family: BenchmarkFamily, profile: CapabilityProfile) -> f64 {
    let emphasis = match family {
        BenchmarkFamily::ContradictionResolutionQa | BenchmarkFamily::BeliefRevisionQa => 1.0,
        BenchmarkFamily::TemporalQa | BenchmarkFamily::MultiHopEvidenceQa => 0.65,
        _ => 0.48,
    };
    bounded(profile.contradiction * emphasis + profile.temporal * 0.12)
}

fn path_fit(family: BenchmarkFamily, profile: CapabilityProfile) -> f64 {
    let emphasis = match family {
        BenchmarkFamily::MultiHopEvidenceQa
        | BenchmarkFamily::CausalTraceQa
        | BenchmarkFamily::CounterfactualPlanningQa => 1.0,
        BenchmarkFamily::ToolUseMemoryQa => 0.76,
        _ => 0.55,
    };
    bounded(profile.graph * emphasis + profile.vector * 0.1)
}

fn citation_fit(profile: CapabilityProfile) -> f64 {
    weighted(&[
        (profile.keyword, 0.18),
        (profile.graph, 0.24),
        (profile.temporal, 0.18),
        (profile.contradiction, 0.16),
        (profile.tool_context, 0.24),
    ])
}

fn belief_fit(family: BenchmarkFamily, profile: CapabilityProfile) -> f64 {
    let emphasis = if family == BenchmarkFamily::BeliefRevisionQa {
        1.0
    } else {
        0.58
    };
    bounded(profile.belief * emphasis + profile.contradiction * 0.16 + profile.temporal * 0.12)
}

fn causal_fit(family: BenchmarkFamily, profile: CapabilityProfile) -> f64 {
    let emphasis = if matches!(
        family,
        BenchmarkFamily::CausalTraceQa | BenchmarkFamily::CounterfactualPlanningQa
    ) {
        1.0
    } else {
        0.5
    };
    bounded(profile.causal * emphasis + profile.graph * 0.12)
}

fn simulation_fit(family: BenchmarkFamily, profile: CapabilityProfile) -> f64 {
    let emphasis = if family == BenchmarkFamily::CounterfactualPlanningQa {
        1.0
    } else {
        0.45
    };
    bounded(profile.simulation * emphasis + profile.causal * 0.14)
}

fn tool_fit(family: BenchmarkFamily, profile: CapabilityProfile) -> f64 {
    let emphasis = if family == BenchmarkFamily::ToolUseMemoryQa {
        1.0
    } else {
        0.68
    };
    bounded(profile.tool_context * emphasis + profile.memory * 0.14)
}

fn compression_fit(family: BenchmarkFamily, profile: CapabilityProfile) -> f64 {
    let emphasis = if family == BenchmarkFamily::ContextCompressionQa {
        1.0
    } else {
        0.6
    };
    bounded(profile.compression * emphasis + profile.citation_base() * 0.14)
}

fn latency_p50_micros(
    family: BenchmarkFamily,
    baseline: Baseline,
    core: &CoreEvalMetrics,
    profile: CapabilityProfile,
) -> u64 {
    let baseline_multiplier = match baseline {
        Baseline::VectorOnlyRag => 1.05,
        Baseline::Bm25Only => 0.78,
        Baseline::HybridSearch => 1.2,
        Baseline::GraphRagStyle => 1.55,
        Baseline::TemporalGraphRetrieval => 1.35,
        Baseline::AgentTranscriptMemory => 1.08,
        Baseline::RealityGraphFullStack => 1.42,
    };
    let speed_drag = 1.0 + (1.0 - profile.speed) * 0.8;
    let latency = core.latency_p50_micros as f64
        * family.latency_multiplier()
        * baseline_multiplier
        * speed_drag;
    latency.round().max(1.0) as u64
}

fn build_leaderboard(results: &[FrontierEvalResult]) -> Vec<LeaderboardEntry> {
    let mut entries = Baseline::all()
        .into_iter()
        .map(|baseline| {
            let baseline_results = results
                .iter()
                .filter(|result| result.baseline == baseline)
                .collect::<Vec<_>>();
            let aggregate_score = average_results(&baseline_results, |result| {
                result.metrics.leaderboard_score()
            });
            let mean_quality_score =
                average_results(&baseline_results, |result| result.metrics.quality_score());
            let mean_latency_p95_micros = average_results(&baseline_results, |result| {
                result.metrics.latency_p95_micros as f64
            })
            .round() as u64;
            let mean_cost_per_successful_answer = average_results(&baseline_results, |result| {
                result.metrics.cost_per_successful_answer
            });
            let family_wins = family_wins(results, baseline);
            LeaderboardEntry {
                rank: 0,
                baseline,
                aggregate_score,
                mean_quality_score,
                mean_latency_p95_micros,
                mean_cost_per_successful_answer,
                family_wins,
            }
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

fn family_wins(results: &[FrontierEvalResult], baseline: Baseline) -> usize {
    BenchmarkFamily::all()
        .into_iter()
        .filter(|family| {
            results
                .iter()
                .filter(|result| result.family == *family)
                .max_by(|left, right| {
                    left.metrics
                        .leaderboard_score()
                        .total_cmp(&right.metrics.leaderboard_score())
                })
                .map(|winner| winner.baseline == baseline)
                .unwrap_or(false)
        })
        .count()
}

fn render_jsonl(results: &[FrontierEvalResult]) -> String {
    results
        .iter()
        .map(render_json_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_json_line(result: &FrontierEvalResult) -> String {
    let metrics = result.metrics;
    format!(
        "{{\"run_id\":\"{}\",\"seed\":{},\"family\":\"{}\",\"baseline\":\"{}\",\"fixture_dataset\":\"{}\",\"case_id\":\"{}\",\"passed_adoption_gate\":{},\"metrics\":{{\"answer_accuracy\":{},\"evidence_recall\":{},\"evidence_precision\":{},\"temporal_correctness\":{},\"contradiction_f1\":{},\"multi_hop_path_recall\":{},\"citation_faithfulness\":{},\"belief_revision_accuracy\":{},\"causal_trace_recall\":{},\"simulation_usefulness\":{},\"tool_use_context_recall\":{},\"compression_fidelity\":{},\"latency_p50_micros\":{},\"latency_p95_micros\":{},\"latency_p99_micros\":{},\"throughput_qps\":{},\"cost_per_successful_answer\":{}}},\"output_summary\":\"{}\"}}",
        escape_json(&result.run_id),
        result.seed,
        result.family.slug(),
        result.baseline.slug(),
        escape_json(&result.fixture_dataset),
        escape_json(&result.case_id),
        result.passed_adoption_gate,
        fmt_float(metrics.answer_accuracy),
        fmt_float(metrics.evidence_recall),
        fmt_float(metrics.evidence_precision),
        fmt_float(metrics.temporal_correctness),
        fmt_float(metrics.contradiction_f1),
        fmt_float(metrics.multi_hop_path_recall),
        fmt_float(metrics.citation_faithfulness),
        fmt_float(metrics.belief_revision_accuracy),
        fmt_float(metrics.causal_trace_recall),
        fmt_float(metrics.simulation_usefulness),
        fmt_float(metrics.tool_use_context_recall),
        fmt_float(metrics.compression_fidelity),
        metrics.latency_p50_micros,
        metrics.latency_p95_micros,
        metrics.latency_p99_micros,
        fmt_float(metrics.throughput_qps),
        fmt_float(metrics.cost_per_successful_answer),
        escape_json(&result.output_summary)
    )
}

fn render_leaderboard_markdown(leaderboard: &[LeaderboardEntry]) -> String {
    let mut markdown = String::from(
        "| Rank | Baseline | Score | Quality | p95 Latency us | Cost / Success | Family Wins |\n",
    );
    markdown.push_str("| ---: | --- | ---: | ---: | ---: | ---: | ---: |\n");
    for entry in leaderboard {
        markdown.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            entry.rank,
            entry.baseline.title(),
            fmt_float(entry.aggregate_score),
            fmt_float(entry.mean_quality_score),
            entry.mean_latency_p95_micros,
            fmt_float(entry.mean_cost_per_successful_answer),
            entry.family_wins
        ));
    }
    markdown
}

fn render_markdown_report(
    run_id: &str,
    seed_config: &SeedConfig,
    results: &[FrontierEvalResult],
    leaderboard: &[LeaderboardEntry],
    leaderboard_markdown: &str,
) -> String {
    let mut markdown = format!(
        "# Frontier Eval Report\n\nRun: `{}`\nSeed: `{}`\nDataset scale: `{}`\n\n{}\n\n",
        run_id,
        seed_config.seed,
        seed_config.dataset_scale.slug(),
        ADOPTION_GATE_LINE
    );
    markdown.push_str(
        "References: Temporal Graph Benchmark 2.0 shapes temporal graph families; MLPerf Inference inspires standardized latency, throughput, and cost reporting.\n\n",
    );
    markdown.push_str("## Leaderboard\n\n");
    markdown.push_str(leaderboard_markdown);
    markdown.push_str("\n## Benchmark Families\n\n");
    markdown.push_str("| Family | Fixture Dataset | Best Baseline | Best Score |\n");
    markdown.push_str("| --- | --- | --- | ---: |\n");
    for family in &seed_config.families {
        let best = results
            .iter()
            .filter(|result| result.family == *family)
            .max_by(|left, right| {
                left.metrics
                    .leaderboard_score()
                    .total_cmp(&right.metrics.leaderboard_score())
            })
            .expect("each configured family has results");
        markdown.push_str(&format!(
            "| {} | `{}` | {} | {} |\n",
            family.title(),
            family.fixture_dataset(),
            best.baseline.title(),
            fmt_float(best.metrics.leaderboard_score())
        ));
    }
    markdown.push_str("\n## Output Artifacts\n\n");
    markdown.push_str("- JSONL eval results: one deterministic record per family/baseline pair.\n");
    markdown.push_str("- Markdown report: this report.\n");
    markdown.push_str("- Leaderboard summary: sorted aggregate baseline comparison.\n");
    markdown.push_str(
        "- Reproducible seed config: persisted JSON seed and family/baseline selection.\n",
    );
    markdown.push_str("\n## Adoption Gate\n\n");
    let leader = leaderboard
        .first()
        .map(|entry| entry.baseline.title())
        .unwrap_or("none");
    markdown.push_str(&format!(
        "Current fixture leader: {}. A lab-facing run should preserve every baseline and publish failures instead of hiding them.\n",
        leader
    ));
    markdown
}

fn output_summary(
    family: BenchmarkFamily,
    baseline: Baseline,
    metrics: &FrontierMetrics,
) -> String {
    format!(
        "{} evaluated with {}: quality {}, p95 {} us, cost/success {}.",
        family.title(),
        baseline.title(),
        fmt_float(metrics.quality_score()),
        metrics.latency_p95_micros,
        fmt_float(metrics.cost_per_successful_answer)
    )
}

fn average_results<F>(results: &[&FrontierEvalResult], metric: F) -> f64
where
    F: Fn(&FrontierEvalResult) -> f64,
{
    if results.is_empty() {
        return 0.0;
    }
    results.iter().map(|result| metric(result)).sum::<f64>() / results.len() as f64
}

fn weighted(values: &[(f64, f64)]) -> f64 {
    let total_weight = values.iter().map(|(_, weight)| weight).sum::<f64>();
    if total_weight == 0.0 {
        return 0.0;
    }
    bounded(
        values
            .iter()
            .map(|(value, weight)| value * weight)
            .sum::<f64>()
            / total_weight,
    )
}

fn bounded(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

fn json_array<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let entries = values
        .map(|value| format!("\"{}\"", escape_json(value)))
        .collect::<Vec<_>>();
    format!("[{}]", entries.join(","))
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn fmt_float(value: f64) -> String {
    format!("{value:.4}")
}

impl CapabilityProfile {
    fn citation_base(self) -> f64 {
        weighted(&[
            (self.keyword, 0.2),
            (self.graph, 0.28),
            (self.temporal, 0.22),
            (self.tool_context, 0.3),
        ])
    }
}

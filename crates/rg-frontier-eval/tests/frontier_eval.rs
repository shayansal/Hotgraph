use rg_frontier_eval::{
    Baseline, BenchmarkFamily, FrontierEvalHarness, FrontierEvalReport, SeedConfig,
};

#[test]
fn defines_all_frontier_benchmark_families_and_baselines() {
    assert_eq!(
        BenchmarkFamily::all(),
        vec![
            BenchmarkFamily::TemporalQa,
            BenchmarkFamily::AgentMemoryQa,
            BenchmarkFamily::MultiHopEvidenceQa,
            BenchmarkFamily::ContradictionResolutionQa,
            BenchmarkFamily::BeliefRevisionQa,
            BenchmarkFamily::CausalTraceQa,
            BenchmarkFamily::CounterfactualPlanningQa,
            BenchmarkFamily::ContextCompressionQa,
            BenchmarkFamily::ToolUseMemoryQa,
            BenchmarkFamily::LatencyCostStress,
        ]
    );
    assert_eq!(
        Baseline::all(),
        vec![
            Baseline::VectorOnlyRag,
            Baseline::Bm25Only,
            Baseline::HybridSearch,
            Baseline::GraphRagStyle,
            Baseline::TemporalGraphRetrieval,
            Baseline::AgentTranscriptMemory,
            Baseline::RealityGraphFullStack,
        ]
    );
}

#[test]
fn run_outputs_jsonl_markdown_leaderboard_and_seed_config() {
    let report = run_smoke_report();

    assert_eq!(
        report.results().len(),
        BenchmarkFamily::all().len() * Baseline::all().len()
    );
    assert_eq!(
        report.jsonl_results().lines().count(),
        report.results().len()
    );
    assert!(report
        .jsonl_results()
        .contains("\"family\":\"temporal_qa\""));
    assert!(report
        .jsonl_results()
        .contains("\"baseline\":\"reality_graph_full_stack\""));
    assert!(report.markdown_report().contains("# Frontier Eval Report"));
    assert!(report
        .markdown_report()
        .contains("Temporal Graph Benchmark"));
    assert!(report.markdown_report().contains("MLPerf"));
    assert!(report
        .leaderboard_markdown()
        .contains("| Rank | Baseline |"));
    assert!(report.seed_config_json().contains("\"seed\":41"));
    assert!(report
        .seed_config_json()
        .contains("\"dataset_scale\":\"smoke\""));
}

#[test]
fn results_cover_every_family_baseline_pair_with_cost_latency_metrics() {
    let report = run_smoke_report();

    for family in BenchmarkFamily::all() {
        for baseline in Baseline::all() {
            let result = report
                .result_for(family, baseline)
                .expect("family/baseline result");
            assert!((0.0..=1.0).contains(&result.metrics.answer_accuracy));
            assert!((0.0..=1.0).contains(&result.metrics.evidence_recall));
            assert!((0.0..=1.0).contains(&result.metrics.evidence_precision));
            assert!((0.0..=1.0).contains(&result.metrics.temporal_correctness));
            assert!((0.0..=1.0).contains(&result.metrics.contradiction_f1));
            assert!((0.0..=1.0).contains(&result.metrics.multi_hop_path_recall));
            assert!((0.0..=1.0).contains(&result.metrics.citation_faithfulness));
            assert!(result.metrics.latency_p50_micros <= result.metrics.latency_p95_micros);
            assert!(result.metrics.latency_p95_micros <= result.metrics.latency_p99_micros);
            assert!(result.metrics.throughput_qps > 0.0);
            assert!(result.metrics.cost_per_successful_answer > 0.0);
        }
    }
}

#[test]
fn leaderboard_keeps_all_baselines_visible_and_ranks_full_stack_first_on_fixtures() {
    let report = run_smoke_report();
    let leaderboard = report.leaderboard();

    assert_eq!(leaderboard.len(), Baseline::all().len());
    assert_eq!(leaderboard[0].baseline, Baseline::RealityGraphFullStack);
    assert!(leaderboard
        .iter()
        .any(|entry| entry.baseline == Baseline::VectorOnlyRag));
    assert!(leaderboard
        .iter()
        .any(|entry| entry.baseline == Baseline::AgentTranscriptMemory));
}

#[test]
fn seed_configs_make_runs_reproducible() {
    let first = run_smoke_report();
    let second = run_smoke_report();

    assert_eq!(first.jsonl_results(), second.jsonl_results());
    assert_eq!(first.markdown_report(), second.markdown_report());
    assert_eq!(first.leaderboard_markdown(), second.leaderboard_markdown());
    assert_eq!(first.seed_config_json(), second.seed_config_json());
}

fn run_smoke_report() -> FrontierEvalReport {
    FrontierEvalHarness::new(SeedConfig::smoke(41)).run()
}

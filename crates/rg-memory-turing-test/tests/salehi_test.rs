use rg_memory_turing_test::{
    MemoryBaseline, MemoryScenarioFamily, MemoryTuringCatalog, SalehiCategory, SalehiHarness,
    SalehiReport,
};

#[test]
fn loads_salehi_memory_turing_scenarios_from_eval_directory() {
    let catalog = MemoryTuringCatalog::load_builtin().expect("catalog loads");

    assert_eq!(catalog.categories(), SalehiCategory::all());
    assert_eq!(catalog.families(), MemoryScenarioFamily::all());
    assert!(catalog.scenarios().len() >= 60);
    assert!(catalog.scenarios().iter().any(|scenario| scenario.category
        == SalehiCategory::RememberAcross1000Sessions
        && scenario.session_count == 1000));
    assert!(catalog
        .scenarios()
        .iter()
        .any(|scenario| scenario.family == MemoryScenarioFamily::ExecutiveAssistant));
    assert!(catalog
        .scenarios()
        .iter()
        .any(|scenario| scenario.family == MemoryScenarioFamily::EnterpriseOperations));
}

#[test]
fn compares_all_memory_baselines_across_all_categories() {
    let report = run_report();

    assert_eq!(report.baseline_reports().len(), MemoryBaseline::all().len());
    for baseline in MemoryBaseline::all() {
        let baseline_report = report.baseline_report(baseline).expect("baseline");
        assert_eq!(
            baseline_report.category_scores.len(),
            SalehiCategory::all().len()
        );
        assert!((0.0..=1.0).contains(&baseline_report.metrics.aggregate_score));
        assert!((0.0..=1.0).contains(&baseline_report.metrics.tenant_isolation));
    }
}

#[test]
fn reality_graph_temporal_belief_memory_beats_ordinary_memory_on_hard_categories() {
    let report = run_report();
    let reality = report
        .baseline_report(MemoryBaseline::RealityGraphTemporalBeliefMemory)
        .expect("reality graph");
    let transcript = report
        .baseline_report(MemoryBaseline::TranscriptMemory)
        .expect("transcript");
    let vector = report
        .baseline_report(MemoryBaseline::VectorMemory)
        .expect("vector");
    let summary = report
        .baseline_report(MemoryBaseline::SummaryMemory)
        .expect("summary");

    assert!(reality.metrics.aggregate_score > transcript.metrics.aggregate_score);
    assert!(reality.metrics.aggregate_score > vector.metrics.aggregate_score);
    assert!(reality.metrics.aggregate_score > summary.metrics.aggregate_score);
    assert!(
        reality.category_score(SalehiCategory::DistinguishOldTruthFromCurrentTruth)
            > vector.category_score(SalehiCategory::DistinguishOldTruthFromCurrentTruth)
    );
    assert!(
        reality.category_score(SalehiCategory::AvoidsCrossUserMemoryLeakage)
            > transcript.category_score(SalehiCategory::AvoidsCrossUserMemoryLeakage)
    );
}

#[test]
fn ordinary_memory_systems_fail_at_least_one_salehi_category() {
    let report = run_report();

    for baseline in [
        MemoryBaseline::TranscriptMemory,
        MemoryBaseline::VectorMemory,
        MemoryBaseline::SummaryMemory,
        MemoryBaseline::GraphMemory,
    ] {
        let baseline_report = report.baseline_report(baseline).expect("baseline");
        assert!(
            baseline_report
                .category_scores
                .iter()
                .any(|score| score.score < 0.65),
            "{baseline:?} should expose at least one weak memory category"
        );
    }
}

#[test]
fn report_outputs_jsonl_markdown_and_leaderboard() {
    let report = run_report();

    assert!(report
        .jsonl_results()
        .contains("\"category\":\"updates_beliefs_when_corrected\""));
    assert!(report
        .jsonl_results()
        .contains("\"baseline\":\"reality_graph_temporal_belief_memory\""));
    assert!(report
        .markdown_report()
        .contains("# Salehi Memory Turing Test"));
    assert!(report
        .markdown_report()
        .contains("ordinary agent memory systems fail"));
    assert!(report
        .leaderboard_markdown()
        .contains("| Rank | Baseline |"));
    assert_eq!(
        report.leaderboard().first().expect("leader").baseline,
        MemoryBaseline::RealityGraphTemporalBeliefMemory
    );
}

fn run_report() -> SalehiReport {
    let catalog = MemoryTuringCatalog::load_builtin().expect("catalog loads");
    SalehiHarness.run(&catalog, MemoryBaseline::all())
}

use rg_eval::{
    AdaptiveRouter, EvalCatalog, EvalHarness, ImprovementGate, MetricSnapshot, RetrievalKind,
};

#[test]
fn loads_all_phase_23_fixture_datasets() {
    let catalog = EvalCatalog::load_builtin().expect("builtin fixtures load");
    let names = catalog.dataset_names();

    assert_eq!(
        names,
        vec![
            "agent_conversation_memory",
            "contradictory_evidence",
            "geopolitical_events",
            "multi_hop_company_ownership",
            "supply_chain_dependency",
            "temporal_employment",
        ]
    );
    assert_eq!(catalog.datasets().len(), 6);
    assert!(catalog.total_cases() >= 6);
    assert!(catalog.total_evidence_records() >= 12);
}

#[test]
fn harness_compares_all_retrieval_strategies_and_reports_sota_metrics() {
    let catalog = EvalCatalog::load_builtin().expect("builtin fixtures load");
    let report = EvalHarness::default().run(&catalog, RetrievalKind::all());

    assert_eq!(report.strategy_reports.len(), 6);
    assert!(report
        .strategy_reports
        .iter()
        .any(|report| report.kind == RetrievalKind::VectorOnly));
    assert!(report
        .strategy_reports
        .iter()
        .any(|report| report.kind == RetrievalKind::KeywordOnly));
    assert!(report
        .strategy_reports
        .iter()
        .any(|report| report.kind == RetrievalKind::GraphOnly));
    assert!(report
        .strategy_reports
        .iter()
        .any(|report| report.kind == RetrievalKind::TemporalGraph));
    assert!(report
        .strategy_reports
        .iter()
        .any(|report| report.kind == RetrievalKind::Hybrid));
    assert!(report
        .strategy_reports
        .iter()
        .any(|report| report.kind == RetrievalKind::AdaptiveRouted));

    for strategy_report in &report.strategy_reports {
        let metrics = &strategy_report.metrics;
        assert!((0.0..=1.0).contains(&metrics.answer_accuracy));
        assert!((0.0..=1.0).contains(&metrics.evidence_recall));
        assert!((0.0..=1.0).contains(&metrics.evidence_precision));
        assert!((0.0..=1.0).contains(&metrics.temporal_correctness));
        assert!((0.0..=1.0).contains(&metrics.contradiction_detection_f1));
        assert!((0.0..=1.0).contains(&metrics.multi_hop_path_recall));
        assert!((0.0..=1.0).contains(&metrics.citation_faithfulness));
        assert!((0.0..=1.0).contains(&metrics.memory_freshness));
        assert!((0.0..=1.0).contains(&metrics.staleness_error_rate));
        assert!(metrics.latency_p50_micros <= metrics.latency_p95_micros);
        assert!(metrics.latency_p95_micros <= metrics.latency_p99_micros);
        assert!(metrics.cost_per_answered_query >= 0.0);
    }

    let adaptive = report
        .strategy_report(RetrievalKind::AdaptiveRouted)
        .expect("adaptive report");
    let vector = report
        .strategy_report(RetrievalKind::VectorOnly)
        .expect("vector report");
    let keyword = report
        .strategy_report(RetrievalKind::KeywordOnly)
        .expect("keyword report");

    assert!(adaptive.metrics.evidence_recall >= vector.metrics.evidence_recall);
    assert!(adaptive.metrics.evidence_recall >= keyword.metrics.evidence_recall);
}

#[test]
fn adaptive_router_selects_retrieval_mode_by_query_shape() {
    let catalog = EvalCatalog::load_builtin().expect("builtin fixtures load");
    let temporal_case = catalog.case("temporal-employment-2024").expect("case");
    let contradiction_case = catalog.case("contradictory-ceo-overlap").expect("case");
    let memory_case = catalog.case("agent-memory-preference").expect("case");
    let ownership_case = catalog.case("ownership-multihop-control").expect("case");
    let router = AdaptiveRouter;

    assert_eq!(router.route(temporal_case), RetrievalKind::TemporalGraph);
    assert_eq!(router.route(contradiction_case), RetrievalKind::Hybrid);
    assert_eq!(router.route(memory_case), RetrievalKind::Hybrid);
    assert_eq!(router.route(ownership_case), RetrievalKind::GraphOnly);
}

#[test]
fn improvement_gate_requires_quality_latency_or_cost_progress() {
    let baseline = MetricSnapshot {
        answer_accuracy: 0.7,
        evidence_recall: 0.6,
        evidence_precision: 0.65,
        temporal_correctness: 0.7,
        contradiction_detection_f1: 0.5,
        multi_hop_path_recall: 0.4,
        citation_faithfulness: 0.75,
        latency_p95_micros: 1_000,
        cost_per_answered_query: 10.0,
    };
    let unchanged = baseline.clone();
    let better_recall = MetricSnapshot {
        evidence_recall: 0.61,
        ..baseline.clone()
    };
    let lower_latency = MetricSnapshot {
        latency_p95_micros: 900,
        ..baseline.clone()
    };
    let lower_cost = MetricSnapshot {
        cost_per_answered_query: 9.5,
        ..baseline.clone()
    };

    let gate = ImprovementGate::default();
    assert!(!gate.passes(&baseline, &unchanged));
    assert!(gate.passes(&baseline, &better_recall));
    assert!(gate.passes(&baseline, &lower_latency));
    assert!(gate.passes(&baseline, &lower_cost));
}

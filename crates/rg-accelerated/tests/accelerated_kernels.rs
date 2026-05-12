use rg_accelerated::{
    AccelerationBackend, BatchBfsKernel, CsrGraph, GpuAccelerationPolicy, GpuExperiment,
    KernelBenchmarkEvidence, PageRankConfig, Personalization, PersonalizedPageRankKernel,
    RoaringCandidateSet, SimdTimestampFilter,
};

#[test]
fn csr_adjacency_representation_is_sorted_compact_and_deterministic() {
    let graph = CsrGraph::from_edges(5, &[(0, 2), (0, 1), (1, 3), (0, 1), (3, 4)])
        .expect("valid csr graph");

    assert_eq!(graph.node_count(), 5);
    assert_eq!(graph.edge_count(), 4);
    assert_eq!(graph.row_offsets(), &[0, 2, 3, 3, 4, 4]);
    assert_eq!(graph.column_indices(), &[1, 2, 3, 4]);
    assert_eq!(graph.neighbors(0).expect("neighbors"), &[1, 2]);
    assert_eq!(graph.neighbors(2).expect("neighbors"), &[]);
    assert_eq!(
        CsrGraph::from_edges(2, &[(2, 0)]).expect_err("out-of-range edge"),
        "edge endpoint exceeds node_count"
    );
}

#[test]
fn roaring_candidate_sets_are_sorted_deduplicated_and_support_set_ops() {
    let left = RoaringCandidateSet::from_unsorted([7, 1, 3, 3, 8]);
    let right = RoaringCandidateSet::from_unsorted([3, 4, 7, 9]);

    assert_eq!(left.to_vec(), vec![1, 3, 7, 8]);
    assert!(left.contains(7));
    assert_eq!(left.len(), 4);
    assert_eq!(left.intersection(&right).to_vec(), vec![3, 7]);
    assert_eq!(left.union(&right).to_vec(), vec![1, 3, 4, 7, 8, 9]);
    assert_eq!(left.difference(&right).to_vec(), vec![1, 8]);
    assert_eq!(left.iter().copied().collect::<Vec<_>>(), vec![1, 3, 7, 8]);
}

#[test]
fn simd_timestamp_filter_returns_valid_candidates_without_allocating_domain_objects() {
    let valid_from = [10, 20, 30, 40, 50, 60];
    let valid_to = [Some(20), Some(40), None, Some(45), Some(55), None];
    let candidates = RoaringCandidateSet::from_unsorted([0, 1, 2, 3, 4, 5]);

    let all = SimdTimestampFilter::valid_at(&valid_from, &valid_to, 35)
        .expect("matching timestamp vectors");
    let filtered =
        SimdTimestampFilter::valid_at_candidates(&valid_from, &valid_to, 35, &candidates)
            .expect("matching timestamp vectors");

    assert_eq!(all.to_vec(), vec![1, 2]);
    assert_eq!(filtered.to_vec(), vec![1, 2]);
    assert_eq!(
        SimdTimestampFilter::valid_at(&valid_from, &[Some(1)], 35).expect_err("length mismatch"),
        "valid_from and valid_to must have the same length"
    );
}

#[test]
fn batch_bfs_expands_multiple_start_nodes_with_depth_and_parent_trace() {
    let graph = CsrGraph::from_edges(6, &[(0, 1), (0, 2), (1, 3), (2, 3), (3, 4), (4, 5)])
        .expect("valid graph");

    let results = BatchBfsKernel::expand(&graph, &[0, 2], 2);

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].start, 0);
    assert_eq!(
        results[0]
            .reached
            .iter()
            .map(|node| (node.node, node.depth, node.parent))
            .collect::<Vec<_>>(),
        vec![
            (0, 0, None),
            (1, 1, Some(0)),
            (2, 1, Some(0)),
            (3, 2, Some(1))
        ]
    );
    assert_eq!(
        results[1]
            .reached
            .iter()
            .map(|node| (node.node, node.depth))
            .collect::<Vec<_>>(),
        vec![(2, 0), (3, 1), (4, 2)]
    );
}

#[test]
fn batch_personalized_pagerank_keeps_seed_bias_and_returns_sorted_scores() {
    let graph =
        CsrGraph::from_edges(4, &[(0, 1), (1, 2), (2, 0), (2, 3), (3, 2)]).expect("valid graph");
    let kernel = PersonalizedPageRankKernel::new(PageRankConfig {
        damping: 0.85,
        iterations: 24,
        tolerance: 0.000_001,
        top_k: 3,
    });

    let results = kernel.rank_batch(
        &graph,
        &[
            Personalization::single_seed(0),
            Personalization::weighted(vec![(3, 2.0), (2, 1.0)]).expect("valid personalization"),
        ],
    );

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].seed_nodes, vec![0]);
    assert!(results[0].top_scores.iter().any(|score| score.node == 0));
    assert!(results[0].top_scores[0].score >= results[0].top_scores[1].score);
    assert_eq!(results[1].seed_nodes, vec![2, 3]);
    assert!(results[1].top_scores.iter().any(|score| score.node == 3));
}

#[test]
fn gpu_experiments_are_feature_gated_and_require_benchmark_evidence() {
    let experiments = GpuExperiment::available();

    assert!(experiments
        .iter()
        .all(|experiment| experiment.requires_benchmark_evidence));
    assert!(experiments
        .iter()
        .all(|experiment| experiment.backend != AccelerationBackend::Cpu));
    assert!(!GpuAccelerationPolicy::cpu_first().allows_gpu_without_benchmark());

    let policy = GpuAccelerationPolicy::cpu_first();
    let weak_evidence = KernelBenchmarkEvidence {
        cpu_p95_micros: 100,
        accelerated_p95_micros: 95,
        minimum_speedup: 1.25,
    };
    let strong_evidence = KernelBenchmarkEvidence {
        cpu_p95_micros: 100,
        accelerated_p95_micros: 60,
        minimum_speedup: 1.25,
    };

    assert!(!policy.approves_gpu(&weak_evidence));
    assert!(policy.approves_gpu(&strong_evidence));
}

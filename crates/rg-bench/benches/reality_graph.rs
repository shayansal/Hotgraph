use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use rg_ai::{EvidencePackGenerator, EvidencePackRequest, VectorIndex};
use rg_bench::{
    agent_memory_graph, build_storage, build_temporal_index, build_vector_index,
    company_ownership_graph, social_graph, supply_chain_graph, SyntheticGraphConfig,
};
use rg_core::{
    AssertionId, Confidence, ContentHash, ContextScope, EntityId, EntityType, GraphValue,
    PredicateId, PropertyMap, SourceId, SourceType, TimeInterval, TxTime, ValidTime,
};
use rg_events::{AddAssertion, AddSource, CreateEntity, EventLog, GraphCommand};
use rg_query::{EntityPattern, GraphQuery, PathQuery, PredicatePattern, QueryEngine};
use rg_storage::{InMemoryStorage, SnapshotReader, SnapshotWriter};

fn bench_entity_insert_throughput(c: &mut Criterion) {
    let (scale, config) = bench_scale();
    let entity_count = config.entity_count.max(1);
    let mut group = c.benchmark_group("entity_insert_throughput");
    group.throughput(Throughput::Elements(entity_count as u64));
    group.bench_with_input(
        BenchmarkId::new("entities", scale),
        &entity_count,
        |bencher, &entity_count| {
            bencher.iter(|| {
                let mut log = EventLog::new(TxTime::new(0));
                for index in 0..entity_count {
                    log.execute(create_entity_command(index))
                        .expect("entity insert command is valid");
                }
                black_box(log.events().len());
            });
        },
    );
    group.finish();
}

fn bench_assertion_insert_throughput(c: &mut Criterion) {
    let (scale, config) = bench_scale();
    let entity_count = config.entity_count.max(2);
    let assertion_count = config.assertion_count;
    let base_log = seeded_entity_log(entity_count);
    let source_id = SourceId::new("bench-source");

    let mut group = c.benchmark_group("assertion_insert_throughput");
    group.throughput(Throughput::Elements(assertion_count as u64));
    group.bench_with_input(
        BenchmarkId::new("assertions", scale),
        &assertion_count,
        |bencher, &assertion_count| {
            bencher.iter_batched(
                || base_log.clone(),
                |mut log| {
                    for index in 0..assertion_count {
                        log.execute(assertion_command(index, entity_count, &source_id))
                            .expect("assertion insert command is valid");
                    }
                    black_box(log.events().len());
                },
                BatchSize::LargeInput,
            );
        },
    );
    group.finish();
}

fn bench_event_replay_speed(c: &mut Criterion) {
    let (scale, config) = bench_scale();
    let graph = social_graph(config);
    let mut group = c.benchmark_group("event_replay_speed");
    group.throughput(Throughput::Elements(graph.events.len() as u64));
    group.bench_function(BenchmarkId::new("replay", scale), |bencher| {
        bencher.iter(|| {
            black_box(InMemoryStorage::replay(black_box(&graph.events)).expect("replay succeeds"));
        });
    });
    group.finish();
}

fn bench_point_in_time_query_latency(c: &mut Criterion) {
    let (scale, config) = bench_scale();
    let graph = social_graph(config);
    let storage = build_storage(&graph).expect("social graph replays");
    let engine = QueryEngine::from_storage(storage);
    let query = GraphQuery {
        subject: Some(EntityPattern::Id(graph.anchor_entity.clone())),
        predicate: Some(PredicatePattern::Id(PredicateId::new("knows"))),
        object: None,
        valid_at: Some(graph.point_in_time.as_i64()),
        known_at: Some(graph.known_at.as_i64()),
        context: Some(ContextScope::Named("social".to_owned())),
        min_confidence: Some(0.5),
        limit: Some(100),
    };

    let mut group = c.benchmark_group("point_in_time_query_latency");
    group.bench_function(BenchmarkId::new("graph_query", scale), |bencher| {
        bencher.iter(|| {
            black_box(engine.execute_graph(black_box(query.clone())));
        });
    });
    group.finish();
}

fn bench_adjacent_edge_traversal_latency(c: &mut Criterion) {
    let (scale, config) = bench_scale();
    let graph = supply_chain_graph(config);
    let storage = build_storage(&graph).expect("supply graph replays");

    let mut group = c.benchmark_group("adjacent_edge_traversal_latency");
    group.bench_function(BenchmarkId::new("adjacent_edges", scale), |bencher| {
        bencher.iter(|| {
            black_box(storage.adjacent_edges(black_box(&graph.anchor_entity)));
        });
    });
    group.finish();
}

fn bench_path_query_latency(c: &mut Criterion) {
    let (scale, config) = bench_scale();
    let graph = company_ownership_graph(config);
    let storage = build_storage(&graph).expect("ownership graph replays");
    let engine = QueryEngine::from_storage(storage);
    let query = PathQuery {
        start: graph.anchor_entity.clone(),
        end: Some(graph.terminal_entity.clone()),
        predicates: graph.path_predicates.clone(),
        valid_at: Some(graph.point_in_time.as_i64()),
        max_depth: 2,
        min_confidence: Some(0.5),
    };

    let mut group = c.benchmark_group("path_query_latency");
    group.bench_function(BenchmarkId::new("two_hop", scale), |bencher| {
        bencher.iter(|| {
            black_box(engine.execute_path(black_box(query.clone())));
        });
    });
    group.finish();
}

fn bench_contradiction_detection_latency(c: &mut Criterion) {
    let (scale, config) = bench_scale();
    let graph = supply_chain_graph(config);
    let index = build_temporal_index(&graph).expect("supply graph indexes");

    let mut group = c.benchmark_group("contradiction_detection_latency");
    group.bench_function(
        BenchmarkId::new("overlapping_assertions", scale),
        |bencher| {
            bencher.iter(|| {
                black_box(index.contradictions());
            });
        },
    );
    group.finish();
}

fn bench_evidence_pack_generation_latency(c: &mut Criterion) {
    let (scale, config) = bench_scale();
    let graph = supply_chain_graph(config);
    let storage = build_storage(&graph).expect("supply graph replays");
    let generator = EvidencePackGenerator::new(&storage);
    let request = EvidencePackRequest {
        query: "synthetic supply-chain exposure".to_owned(),
        graph_query: GraphQuery {
            subject: Some(EntityPattern::Id(graph.anchor_entity.clone())),
            predicate: Some(PredicatePattern::Id(PredicateId::new("supplies"))),
            object: None,
            valid_at: Some(graph.point_in_time.as_i64()),
            known_at: Some(graph.known_at.as_i64()),
            context: Some(ContextScope::Named("supply-chain".to_owned())),
            min_confidence: Some(0.5),
            limit: Some(50),
        },
        path_query: Some(PathQuery {
            start: graph.anchor_entity.clone(),
            end: Some(graph.terminal_entity.clone()),
            predicates: graph.path_predicates.clone(),
            valid_at: Some(graph.point_in_time.as_i64()),
            max_depth: 2,
            min_confidence: Some(0.5),
        }),
        generated_at: TxTime::new(99),
    };

    let mut group = c.benchmark_group("evidence_pack_generation_latency");
    group.bench_function(BenchmarkId::new("evidence_pack", scale), |bencher| {
        bencher.iter(|| {
            black_box(generator.generate(black_box(request.clone())));
        });
    });
    group.finish();
}

fn bench_snapshot_load_time(c: &mut Criterion) {
    let (scale, config) = bench_scale();
    let graph = social_graph(config);
    let storage = build_storage(&graph).expect("social graph replays");
    let path = snapshot_path("load-time");
    let _ = fs::remove_file(&path);
    SnapshotWriter::write(&path, &storage).expect("snapshot writes");

    let mut group = c.benchmark_group("snapshot_load_time");
    group.bench_function(BenchmarkId::new("snapshot_read", scale), |bencher| {
        bencher.iter(|| {
            black_box(SnapshotReader::read(black_box(&path)).expect("snapshot reads"));
        });
    });
    group.finish();

    let _ = fs::remove_file(path);
}

fn bench_vector_sidecar_retrieval_latency(c: &mut Criterion) {
    let (scale, config) = bench_scale();
    let graph = agent_memory_graph(config);
    let index = build_vector_index(&graph).expect("vector index builds");
    let query = vec![1.0, 0.0, 0.0, 0.0];

    let mut group = c.benchmark_group("vector_sidecar_retrieval_latency");
    group.bench_function(BenchmarkId::new("memory_vectors", scale), |bencher| {
        bencher.iter(|| {
            black_box(index.search(black_box(&query), 25).expect("vector search"));
        });
    });
    group.finish();
}

fn bench_scale() -> (String, SyntheticGraphConfig) {
    let scale = std::env::var("RG_BENCH_SCALE").unwrap_or_else(|_| "standard".to_owned());
    let config = match scale.as_str() {
        "smoke" => SyntheticGraphConfig::smoke(),
        "mvp" => SyntheticGraphConfig::mvp_target(),
        _ => SyntheticGraphConfig::standard(),
    };
    (scale, config)
}

fn seeded_entity_log(entity_count: usize) -> EventLog {
    let mut log = EventLog::new(TxTime::new(0));
    log.execute(GraphCommand::AddSource(AddSource {
        id: SourceId::new("bench-source"),
        source_type: SourceType::Document,
        uri: Some("synthetic://bench".to_owned()),
        content_hash: ContentHash::new("sha256:bench"),
        trust_score: Some(0.9),
    }))
    .expect("source setup is valid");
    for index in 0..entity_count {
        log.execute(create_entity_command(index))
            .expect("entity setup is valid");
    }
    log
}

fn create_entity_command(index: usize) -> GraphCommand {
    GraphCommand::CreateEntity(CreateEntity {
        id: benchmark_entity_id(index),
        entity_type: EntityType::Person,
        canonical_name: Some(format!("Benchmark Entity {index}")),
        properties: PropertyMap::default(),
    })
}

fn assertion_command(index: usize, entity_count: usize, source_id: &SourceId) -> GraphCommand {
    let subject_index = index % entity_count;
    let object_index = (subject_index + 1) % entity_count;
    GraphCommand::AddAssertion(AddAssertion {
        id: AssertionId::new(format!("bench-assertion-{index:06}")),
        subject: benchmark_entity_id(subject_index),
        predicate: PredicateId::new("bench_link"),
        object: GraphValue::Entity(benchmark_entity_id(object_index)),
        valid_time: TimeInterval::new(
            ValidTime::new(2_000 + (index % 20) as i64),
            Some(ValidTime::new(2_050)),
        )
        .expect("valid benchmark interval"),
        confidence: Confidence::new(0.8).expect("valid benchmark confidence"),
        source_ids: vec![source_id.clone()],
        context: ContextScope::Named("bench".to_owned()),
    })
}

fn benchmark_entity_id(index: usize) -> EntityId {
    EntityId::new(format!("bench-entity-{index:06}"))
}

fn snapshot_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "reality-graph-{name}-{}.snapshot",
        std::process::id()
    ));
    path
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(5));
    targets =
        bench_entity_insert_throughput,
        bench_assertion_insert_throughput,
        bench_event_replay_speed,
        bench_point_in_time_query_latency,
        bench_adjacent_edge_traversal_latency,
        bench_path_query_latency,
        bench_contradiction_detection_latency,
        bench_evidence_pack_generation_latency,
        bench_snapshot_load_time,
        bench_vector_sidecar_retrieval_latency
}
criterion_main!(benches);

# Distributed Performance Architecture

## Status

Design only. Distributed mode is not implemented and must not be implemented until the single-node engine has benchmark evidence that justifies the added complexity.

## Purpose

Reality Graph should scale from a correct single-node 4D temporal knowledge graph into a distributed, multi-tenant reality engine without weakening the core invariants:

- Rust core semantics remain the source of truth.
- Writes remain event-sourced.
- Assertions remain bitemporal.
- Every answer remains source-backed.
- Query execution remains deterministic.
- Vector search remains an auxiliary retrieval index.

The distributed architecture is a performance roadmap, not a product promise. The benchmark harness decides when each scaling step is warranted.

## Scale Gates

### Single-Node Optimization First

Before distributed mode, the engine should prove:

- 100M assertions on a single optimized Rust node.
- 1B lightweight event records with compacted snapshots.
- p95 entity state query under 50ms for hot data.
- p95 evidence pack under 1s for common queries.
- 100k+ events/sec replay target after optimization.

Required work includes memory-mapped snapshots, append-only log compaction, hot/cold data separation, columnar historical storage, and benchmark evidence for every serving index.

### Cluster Mode Later

Cluster mode targets:

- 10B+ assertions.
- Multi-tenant isolation.
- Distributed temporal queries.
- Streaming ingestion.
- Online index rebuilds.

Cluster mode should begin only when single-node benchmarks show the bottleneck is capacity, ingestion throughput, or query fanout that cannot be solved by simpler local optimization.

## Roadmap

1. Optimize single-node Rust core and indexes.
2. Add memory-mapped immutable snapshots.
3. Add append-only log compaction with deterministic replay checks.
4. Export historical data into columnar segments for analytical scans.
5. Introduce tenant/context partitioning in design and metadata only.
6. Add entity and temporal partition planning once local partitions are benchmarked.
7. Add distributed query planning and fanout only after local query cost models exist.
8. Scale vector sidecars independently while keeping source IDs linked to graph assertions.
9. Consider GPU acceleration only for measured kernels where CPU profiling proves it helps.

## ADRs

- [ADR 0004: Partitioning Strategy](../adr/0004-partitioning-strategy.md)
- [ADR 0005: Snapshot Format](../adr/0005-snapshot-format.md)
- [ADR 0006: Event Log Compaction](../adr/0006-event-log-compaction.md)
- [ADR 0007: Distributed Index Rebuild](../adr/0007-distributed-index-rebuild.md)
- [ADR 0008: Query Fanout](../adr/0008-query-fanout.md)
- [ADR 0009: Hot and Cold Storage](../adr/0009-hot-cold-storage.md)
- [ADR 0010: Vector Index Scaling](../adr/0010-vector-index-scaling.md)


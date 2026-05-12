# Benchmark Targets

Every graph index needs benchmark coverage.

## Required Index Benchmarks

Benchmark each index for:

- Build time from append-log replay.
- Incremental update cost.
- Point lookup latency.
- Neighborhood expansion latency.
- Point-in-time query latency.
- Temporal range query latency.
- Evidence path retrieval latency.
- Memory footprint or approximate resident size.

## Workloads

Include small, medium, and large synthetic graphs. Use deterministic seeds so benchmark results are comparable across runs.

Cover:

- High-degree nodes.
- Long temporal histories.
- Many sources asserting the same relation.
- Corrections and retractions.
- Mixed hot serving queries and historical analytical queries.

## Storage And Compaction

Measure:

- Append throughput.
- Batch write throughput.
- Snapshot compaction time.
- Replay time from log plus snapshot.
- Query latency before and after compaction.

Keep hot serving indexes separate from historical analytical storage. Benchmarks should make that boundary visible.

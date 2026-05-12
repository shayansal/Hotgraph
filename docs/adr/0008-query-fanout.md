# ADR 0008: Query Fanout

## Status

Proposed. Distributed query planning requires a local cost model first.

## Context

Reality Graph queries may ask for current state, historical state, paths, contradictions, causal chains, evidence packs, or AI context. In cluster mode, those queries may cross tenants, contexts, entity partitions, temporal segments, source stores, and vector sidecars.

Fanout must stay deterministic and budget-aware so AI-facing answers remain reproducible and evidence-backed.

## Decision

Distributed query execution will use a planner that emits an explicit fanout plan:

- Resolve tenant and context scope first.
- Identify required entity partitions and temporal segments.
- Push valid-time, transaction-time, context, predicate, confidence, and limit filters as close to storage as possible.
- Execute partition-local scans and traversals with deterministic local ordering.
- Merge results by stable sort keys: score, transaction time, valid time, assertion ID, source ID, and partition ID.
- Attach a query trace listing partitions contacted, budgets used, and evidence sources returned.

Fanout must have explicit budgets for partitions, depth, time, result count, vector candidates, and evidence pack tokens. Queries that exceed budget should return partial results with warnings rather than silently dropping evidence.

## Consequences

- Determinism remains visible in query plans and traces.
- Historical and AI-facing queries can explain where evidence came from.
- Broad queries may be expensive and need budgeted degradation.
- Path queries crossing partitions require careful cycle detection and stable merge ordering.
- Query planner tests must compare repeated runs over the same partition map.

## Benchmark Gate

Implement distributed fanout only after local query benchmarks expose cost models for point-in-time queries, traversals, contradiction checks, and evidence-pack generation.


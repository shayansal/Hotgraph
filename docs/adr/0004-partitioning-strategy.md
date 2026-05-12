# ADR 0004: Partitioning Strategy

## Status

Proposed. Do not implement distributed partitioning until single-node benchmark targets require it.

## Context

Reality Graph must eventually support multi-tenant isolation, streaming ingestion, distributed temporal queries, and 10B+ assertions. Partitioning must not break event sourcing, bitemporal semantics, deterministic replay, or source-backed AI outputs.

Naive graph partitioning by edge locality is not enough because Reality Graph queries also filter by tenant, context, valid time, transaction time, confidence, source, and evidence paths.

## Decision

Partitioning will use a layered strategy:

1. Tenant or context is the hard isolation boundary.
2. Entity partitions divide hot graph serving indexes inside a tenant or context.
3. Temporal segments divide immutable historical storage and cold analytical scans.
4. Source and evidence storage remains addressable by source ID and content hash, with projections linking sources back to assertions.

The write owner for an assertion is the subject entity partition within its tenant or context. Cross-partition relationships are represented as source-of-truth assertion events in the subject partition and as derived inbound adjacency projections in object partitions.

Partition maps must be versioned and replayable. Queries must record the partition-map version used for planning so results can be reproduced.

## Consequences

- Tenant isolation can be enforced before graph fanout.
- Entity-local traversals remain fast for hot data.
- Cross-partition paths require deterministic fanout and merge ordering.
- Inbound adjacency is a projection and can be rebuilt from events.
- Repartitioning must be auditable and replayable.
- Global queries may fan out across many partitions and need budget limits.

## Benchmark Gate

Implement only after the single-node engine demonstrates that entity-local hot indexes or tenant isolation are the bottleneck, and after local partition simulations show improved p95 query or ingestion behavior.


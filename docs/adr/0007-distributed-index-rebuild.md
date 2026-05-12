# ADR 0007: Distributed Index Rebuild

## Status

Proposed. Online distributed index rebuilds are future work after single-node rebuild correctness is proven.

## Context

Serving indexes are derived projections. Failed index updates, new index formats, ontology changes, and snapshot migrations require deterministic rebuilds. In distributed mode, rebuilds must avoid downtime while preserving consistent point-in-time answers.

## Decision

Distributed index rebuilds will use versioned index generations:

1. Start a rebuild generation from a known snapshot and event high watermark.
2. Replay events into shadow indexes without serving them.
3. Catch up from the high watermark to the current log tail.
4. Validate deterministic checksums and golden query results.
5. Atomically promote the new generation per partition.
6. Retain the prior generation until rollback policy expires.

Index generations must be addressable by tenant, partition, index kind, schema version, event range, and build ID. Query planning must use a single compatible generation per partition for deterministic results.

## Consequences

- Indexes can be rebuilt online.
- Failed rebuilds do not corrupt serving indexes.
- Query fanout must tolerate partitions promoting at different times by selecting compatible generations.
- Storage overhead increases during shadow rebuilds.
- Rebuild reports become part of operational audit history.

## Benchmark Gate

Implement distributed rebuild orchestration only after local index rebuilds have deterministic checksums, golden query validation, and measured rebuild throughput for the major index families.


# ADR 0005: Snapshot Format

## Status

Proposed. Memory-mapped snapshots should be implemented for single-node performance before distributed snapshots.

## Context

Reality Graph uses an append-only event log as the source of truth. Replaying from genesis must remain possible, but large deployments need compacted snapshots for fast startup, fast index rebuild, and hot serving recovery.

Snapshots must preserve deterministic graph state, bitemporal visibility, source references, and projection metadata without treating derived indexes as new truth.

## Decision

Snapshots will be immutable, versioned, memory-mappable segment sets. A snapshot contains:

- A manifest with snapshot ID, schema version, event log range, transaction-time high watermark, partition-map version, content hashes, and build metadata.
- Entity, assertion, source, causal, memory, and contradiction records in stable sorted order.
- Hot serving index segments for subject, predicate, object, context, confidence, valid time, transaction time, and adjacency lookups.
- Dictionaries for repeated IDs, predicates, entity types, source IDs, and context scopes.
- Checksums for every segment and for the manifest.

Snapshot files must be append-free after publication. A snapshot is visible only after its manifest is durably written and verified.

## Consequences

- Startup can map immutable data structures instead of rebuilding everything from events.
- Snapshot publication is atomic at the manifest boundary.
- Rebuilds remain deterministic because ordering and event ranges are explicit.
- Schema changes require migration tests or fallback replay from events.
- Derived index corruption can be repaired by discarding snapshots and replaying events.

## Benchmark Gate

The first implementation should target single-node startup, replay, and p95 query latency. Distributed snapshot coordination is blocked until local snapshot load time and memory-map behavior are benchmarked.


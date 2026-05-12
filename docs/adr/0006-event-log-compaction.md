# ADR 0006: Event Log Compaction

## Status

Proposed. Compaction must preserve auditability and replay semantics.

## Context

Reality Graph writes are event-sourced. At large scale, replaying every event from genesis for every recovery is too expensive, but deleting or rewriting history would violate provenance, belief revision, and transaction-time queries.

Compaction must accelerate replay without changing what the system knew at any transaction time.

## Decision

Event log compaction will be additive, not destructive. The durable history consists of:

- Raw event segments.
- Compacted snapshot manifests.
- Optional archived event segments in cold storage.
- Compaction records that identify source event ranges, snapshot outputs, checksums, and validation results.

The system may skip replaying raw events covered by a verified snapshot for serving recovery. It must still retain or restore raw events for audit, migration, historical transaction-time analysis, and legal hold policies.

Compaction jobs must verify:

- Deterministic state before and after compaction.
- Snapshot checksums.
- Event range continuity.
- Index rebuild equivalence.
- Bitemporal query equivalence over sampled and golden intervals.

## Consequences

- Hot recovery gets faster without weakening the event log as source of truth.
- Cold raw events can move to lower-cost storage after policy checks.
- Compaction failures are repairable because they do not mutate raw history.
- Transaction-time queries must know whether they can answer from snapshots, archived events, or both.

## Benchmark Gate

Implement compaction after replay benchmarks show recovery time or replay throughput is limiting. Target evidence includes replay speed, snapshot build time, snapshot load time, and query equivalence checks.


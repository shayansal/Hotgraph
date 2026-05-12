# ADR 0009: Hot and Cold Storage

## Status

Proposed. Hot/cold separation should begin in single-node storage before distributed storage.

## Context

Reality Graph has mixed workloads: hot entity state queries, path traversals, evidence-pack generation, long historical scans, benchmark exports, and audit replay. A single storage layout cannot serve all of these efficiently at large scale.

## Decision

Reality Graph will separate storage by access pattern:

- Hot serving storage: memory-mapped snapshots, compact adjacency, temporal point indexes, and source metadata needed for low-latency queries.
- Warm replay storage: recent append-only event segments and recent compacted snapshots.
- Cold analytical storage: columnar historical assertion, event, source, and contradiction exports.
- Evidence payload storage: source text, chunks, documents, media metadata, and excerpts stored outside hot graph indexes.
- Vector sidecars: independently scaled candidate retrieval indexes linked back by source, assertion, entity, memory, and event IDs.

Hot storage must contain enough metadata to return source IDs, confidence, temporal windows, and contradiction flags. It should not inline large source text or media payloads.

## Consequences

- Hot graph queries avoid scanning bulky evidence payloads.
- Historical analytics can use columnar scans without disturbing serving indexes.
- Evidence-pack generation joins hot graph results with evidence payloads by ID.
- Backup and restore policies differ by storage tier but must preserve replay correctness.
- Cold storage is not source of truth unless it can be traced back to event ranges and snapshots.

## Benchmark Gate

Implement tier boundaries when benchmarks show hot query latency, memory footprint, or analytical scans are competing for the same storage layout.


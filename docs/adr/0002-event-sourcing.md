# ADR 0002: Event-Sourced Writes

## Status

Accepted

## Context

Reality Graph must preserve provenance, corrections, retractions, historical knowledge, and deterministic replay. Updating graph indexes directly would make it harder to answer what the system knew at prior transaction times.

## Decision

Reality Graph is event-sourced. Writes append immutable events first, then update indexes and projections.

The append log is the source of truth. Serving indexes, vector indexes, snapshots, and analytical stores are derived projections.

## Consequences

- Every write path must produce replayable events.
- Indexes can be rebuilt from the append log and compacted snapshots.
- Corrections and retractions append events rather than overwriting history.
- Batch writes should append event batches before projection updates.
- Benchmarking must include append throughput, replay time, compaction time, and index update cost.

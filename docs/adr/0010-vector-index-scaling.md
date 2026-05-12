# ADR 0010: Vector Index Scaling

## Status

Proposed. Vector scaling is auxiliary and must not change graph truth semantics.

## Context

Reality Graph uses embeddings for candidate retrieval over source documents, chunks, entity descriptions, assertion explanations, event descriptions, and agent memories. Vector similarity is useful for recall, but it is not truth.

As tenants, sources, and memories grow, vector indexes may need sidecar scaling independent of the Rust graph core.

## Decision

Vector indexes will scale as derived sidecars:

- Every vector record links back to graph IDs such as source ID, source chunk ID, assertion ID, entity ID, event ID, or memory ID.
- Vector partitions follow tenant or context boundaries first.
- Large tenants may shard vector records by source collection, memory namespace, or temporal segment.
- Vector search returns candidates only; graph filters then enforce tenant, context, valid time, transaction time, confidence, source trust, contradiction status, and permissions.
- Re-embedding and model changes create new vector generations rather than mutating old records in place.

The retrieval compiler decides when vector search is useful. Graph, keyword, temporal, causal, contradiction, and memory operators remain first-class alternatives.

## Consequences

- Vector search can scale independently from graph serving indexes.
- Embedding drift is managed by generation metadata and evaluation.
- Citation and provenance survive because vector hits map back to source-backed graph records.
- Vector-only answers are not acceptable for non-trivial AI outputs.
- GPU acceleration is considered only for measured vector workloads where it improves latency or cost.

## Benchmark Gate

Scale vector sidecars only when retrieval benchmarks show vector latency, recall, or cost bottlenecks. Any new vector generation or routing policy must improve eval metrics or reduce latency/cost without reducing citation faithfulness.


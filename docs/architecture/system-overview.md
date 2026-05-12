# Reality Graph System Overview

## Purpose

Reality Graph is a single-node 4D temporal knowledge graph engine. It stores source-backed assertions as event-sourced, bitemporal graph facts and serves deterministic current or historical graph answers with evidence.

The four core dimensions are:

- Entities.
- Relationships.
- Valid time: when an assertion is true in the modeled reality.
- Transaction time: when Reality Graph learned, changed, or revoked the assertion.

## MVP Architecture

- `rg-core`: Rust domain model for IDs, typed predicates, facts, confidence, provenance, evidence references, and temporal intervals.
- `rg-events`: append-only write events for sources, entities, assertions, corrections, retractions, snapshots, and index rebuilds.
- `rg-storage`: event log, source/evidence store, compacted snapshots, and storage traits.
- `rg-index`: hot serving indexes for current and point-in-time graph queries.
- `rg-query`: deterministic query execution over bitemporal graph state.
- `rg-api`: API boundary for ingestion, query, evidence retrieval, contradiction detection, and AI-ready context packs.
- `rg-ai`: vector retrieval over evidence as an auxiliary index.
- `rg-ingest`: source document ingestion and source-backed assertion creation.
- `rg-bench`: storage, index, and query benchmark targets.
- `rg-sim`: deterministic synthetic temporal graph workloads.

## Core Workflows

### Source Ingestion

1. Accept a source document, passage, or external record.
2. Store source text and evidence payloads in the evidence store.
3. Generate source IDs and evidence IDs.
4. Optionally create embeddings for evidence retrieval.
5. Append source ingestion events.

### Assertion Write

1. Validate entity IDs, typed predicate, object or value, confidence, source IDs, evidence references, valid-time interval, and transaction-time metadata.
2. Append assertion, correction, or retraction events to the immutable event log.
3. Update hot serving indexes from the accepted events.
4. Leave historical transaction-time answers replayable.

### Current Graph Query

1. Use latest transaction time by explicit API default.
2. Resolve graph patterns against hot serving indexes.
3. Return deterministic bindings with confidence, temporal intervals, source IDs, and evidence paths.

### Historical Graph Query

1. Accept valid-time and transaction-time parameters.
2. Evaluate only assertions visible at those temporal coordinates.
3. Return the graph state that was true and known at that point.

### Evidence Retrieval

1. Retrieve evidence by entity, relationship, assertion, or vector similarity.
2. Join evidence references back to source IDs and stored payloads.
3. Return evidence without treating embeddings or retrieved text as graph facts.

### Contradiction Detection

1. Find assertions with overlapping temporal intervals and incompatible typed predicates or values.
2. Compare confidence, provenance, and source IDs.
3. Return contradiction records with evidence paths for each side.

### AI Context Pack Generation

1. Run graph and evidence retrieval for an entity, relationship, question, or task.
2. Package facts, evidence, confidence, source IDs, and temporal metadata.
3. Use deterministic ordering and include enough provenance for downstream citation.

## System Invariants

- Rust core semantics are authoritative across all APIs and bindings.
- All graph writes are event-sourced.
- Serving indexes and vector indexes are derived projections.
- Every assertion is bitemporal.
- Every stored edge has confidence, source, provenance, and temporal metadata.
- Source text and evidence payloads are separate from graph indexes.
- Vector retrieval is auxiliary and cannot create source-of-truth facts.
- Query execution must be deterministic for identical logs, snapshots, and inputs.
- Replaying the event log plus snapshots must rebuild equivalent serving indexes.
- AI-facing outputs must include evidence paths and source IDs.

## Storage Boundaries

- Event log: immutable source of truth.
- Snapshot store: compacted replay accelerator.
- Graph indexes: hot serving projections for point-in-time traversal.
- Evidence store: source documents, passages, and payloads.
- Vector index: auxiliary evidence retrieval projection.

## API Surface

The MVP API should expose:

- Add source document.
- Add entity.
- Add relationship or assertion.
- Query current graph state.
- Query historical graph state.
- Retrieve evidence.
- Detect contradictions.
- Produce AI-ready context pack.
- Health and metadata endpoints.

## Operational Notes

The MVP runs as a single-node engine. Performance work should prioritize cache-friendly indexes, batched writes, compacted snapshots, deterministic replay, and benchmark coverage for every index.

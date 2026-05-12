# Reality Graph PRD

## Product Summary

Reality Graph is a 4D temporal knowledge graph engine for source-backed facts. It models entities, relationships, valid time, and transaction time so applications and AI agents can ask what is true, when it is true, when the system knew it, and which evidence supports it.

## MVP Scope

The MVP is a single-node 4D knowledge graph engine that can ingest source-backed assertions, store temporal facts, query graph state at arbitrary times, retrieve evidence with vector search, and expose results through an API usable by AI agents.

The MVP must support one local deployment, one authoritative Rust core, append-only event storage, serving indexes for current and historical graph state, source/evidence storage, vector retrieval over evidence, contradiction detection, and AI-ready context packs.

## Non-Goals

- Distributed graph storage or multi-node consensus.
- Multi-tenant authorization and billing.
- Visual graph editing.
- Autonomous fact creation without source-backed assertions.
- Treating embeddings, summaries, or model outputs as source-of-truth facts.
- Large-scale analytical warehouse features beyond compacted snapshots and replayable history.

## Personas

- AI agent: needs grounded answers with source IDs, evidence paths, confidence, and temporal context.
- Application developer: needs stable APIs for adding entities, relationships, assertions, and point-in-time graph queries.
- Knowledge engineer: needs to ingest source documents, attach evidence, assign confidence, and correct or retract assertions without losing history.
- Analyst or researcher: needs to compare current and historical graph states and inspect contradictions across sources and time.

## Core Workflows

### Ingest Source Documents

1. Add a source document or external record.
2. Store source text and evidence payloads outside hot graph indexes.
3. Create evidence references that can be linked to future assertions.
4. Optionally embed evidence passages for vector retrieval.

### Add Source-Backed Assertions

1. Add or resolve entities.
2. Add typed relationships between entities or values.
3. Attach valid-time bounds, transaction-time metadata, source IDs, evidence references, and confidence scores.
4. Append events before updating graph indexes.

### Query Graph State

1. Query current graph state using latest transaction time.
2. Query historical graph state by supplying valid time and transaction time.
3. Return deterministic results with confidence, temporal intervals, source IDs, and evidence paths.

### Retrieve Evidence

1. Retrieve evidence for an entity, relationship, assertion, or natural-language evidence request.
2. Use vector search as an auxiliary retrieval path.
3. Return evidence references and source IDs without converting retrieval results into facts.

### Detect Contradictions

1. Compare assertions with overlapping valid-time and transaction-time windows.
2. Detect conflicting predicates, object values, or mutually exclusive states.
3. Return conflicting assertion IDs, confidence scores, source IDs, and evidence paths.

### Produce AI-Ready Context Packs

1. Gather relevant graph facts, evidence, confidence, and temporal metadata.
2. Include source IDs and evidence paths for every claim.
3. Preserve deterministic ordering so repeated requests produce stable context.

## System Invariants

- The Rust core is the source of truth.
- Every fact has provenance.
- Every assertion supports valid time and transaction time.
- No edge is stored without confidence, source, and temporal metadata.
- Writes append immutable events before updating indexes.
- Graph state is queryable at arbitrary valid-time and transaction-time points.
- Embeddings are auxiliary retrieval indexes, not source-of-truth facts.
- AI-facing responses include evidence paths and source IDs.
- Query execution is deterministic.

## Acceptance Criteria

- Add entities.
- Add relationships.
- Add time-bounded assertions.
- Add source documents.
- Attach confidence scores.
- Query current graph state.
- Query graph state at historical time.
- Retrieve evidence for an entity or relationship.
- Detect contradictory assertions.
- Produce AI-ready context packs.

## Success Metrics

- A source-backed assertion can be ingested, indexed, queried, and returned through the API.
- A correction preserves the answer that was visible at an earlier transaction time.
- Evidence retrieval returns source-backed passages and never creates facts directly.
- Contradiction detection returns the conflicting assertions and their evidence.
- AI-ready context packs contain facts, confidence, source IDs, evidence paths, and temporal metadata.

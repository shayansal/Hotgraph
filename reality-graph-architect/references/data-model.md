# Data Model

The Rust core is the source of truth for Reality Graph's domain model.

## Core Objects

Use explicit domain types:

- Newtypes for graph IDs, node IDs, edge IDs, source IDs, event IDs, assertion IDs, snapshot IDs, and timestamps.
- Typed predicates in the core engine. Do not represent core predicates as unconstrained strings.
- Structured errors for validation, storage, indexing, and query failures.

## Fact Shape

A stored fact or edge must carry:

- Subject ID.
- Typed predicate.
- Object ID or typed value.
- Source IDs.
- Provenance record.
- Confidence.
- Valid-time interval.
- Transaction-time interval.
- Evidence references.

Store large source text, documents, extracted passages, and evidence payloads separately from hot graph indexes. Hot indexes may store compact evidence references.

## Events

Writes append events first, then update indexes. Prefer immutable append logs and compacted snapshots.

Recommended event categories:

- Source ingested.
- Entity observed.
- Fact asserted.
- Fact corrected.
- Fact retracted.
- Snapshot compacted.
- Index rebuilt.

Events must be replayable into deterministic indexes. If a write path cannot be replayed, it is not part of the source-of-truth graph.

## AI-Facing Data

Embeddings, vector stores, model summaries, and reranker scores are retrieval aids only. They can suggest candidate facts or evidence, but source-of-truth assertions still require graph events with provenance, confidence, and temporal metadata.

AI-facing APIs must expose source IDs and evidence paths for answers.

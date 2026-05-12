# Data Model

Reality Graph stores source-backed assertions as bitemporal, provenance-bearing graph facts.

## Core Types

- `GraphId`: identifies a graph namespace.
- `NodeId`: identifies an entity, concept, event, document, or value node.
- `EdgeId`: identifies an asserted relationship instance.
- `SourceId`: identifies a source document, stream, system, or record.
- `EvidenceId`: identifies a stored evidence payload or passage.
- `AssertionId`: identifies a logical assertion across corrections.
- `EventId`: identifies an append-log event.
- `Timestamp`: integer internal timestamp.
- `ValidTime`: modeled-world time interval.
- `TransactionTime`: system-knowledge time interval.
- `Confidence`: calibrated assertion confidence.
- `Predicate`: typed domain relationship, not a core-engine string.

All IDs should be newtypes in Rust.

## Fact

A fact represents an assertion about the world:

- Subject node.
- Typed predicate.
- Object node or typed value.
- Source IDs.
- Evidence references.
- Provenance record.
- Confidence.
- Valid-time interval.
- Transaction-time interval.

No edge may be stored without confidence, source, and temporal metadata.

## Provenance

Provenance records answer:

- Which source produced this assertion?
- Which evidence supports it?
- Which ingest process, extractor, or human produced it?
- What confidence was assigned?
- Which event introduced, corrected, or retracted it?

Source text and evidence payloads are stored separately from graph indexes. Hot indexes keep compact references.

## Events

The event log is the source of truth. Event categories include:

- `SourceIngested`
- `EvidenceStored`
- `NodeObserved`
- `FactAsserted`
- `FactCorrected`
- `FactRetracted`
- `SnapshotCompacted`
- `IndexRebuilt`

Corrections and retractions append new events. They do not mutate historical truth.

## Embeddings

Embeddings are auxiliary retrieval indexes. They can retrieve evidence and rank candidates, but they are not facts. Any fact suggested by vector search must still enter the graph through a source-backed assertion event.

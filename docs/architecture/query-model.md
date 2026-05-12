# Query Model

Reality Graph queries are deterministic reads over bitemporal graph state.

## Time Parameters

Every point-in-time graph query must define:

- `valid_at`: when the fact should be true in the modeled reality.
- `transaction_at`: when the system should have known the fact.

APIs may default `transaction_at` to latest, but must make the default explicit. Valid-time behavior should be explicit for AI-facing queries.

## Query Types

- Node lookup by ID.
- Edge lookup by subject, predicate, object, or time interval.
- Neighborhood expansion from a node.
- Pattern match across typed predicates.
- Evidence path retrieval for returned facts.
- Vector evidence search by text query or embedding.
- Historical comparison across valid time or transaction time.

## Result Shape

AI-facing query results must include:

- Answer payload or graph bindings.
- Source IDs.
- Evidence IDs and evidence paths.
- Confidence values.
- Valid-time and transaction-time intervals.
- Query timestamp parameters.
- Deterministic ordering metadata when multiple results tie.

## Determinism

Identical query inputs over identical event logs and snapshots must produce identical results. Tie-breaking should use stable IDs or event order, not map iteration order or nondeterministic vector-store ordering.

## Vector Retrieval

Vector search retrieves source-backed evidence candidates. The query engine may use vector results to expand or rank evidence, but facts still come from bitemporal graph assertions.

## Temporal Corrections

When a fact is corrected:

- Historical transaction-time queries still show what the system knew before correction.
- Latest transaction-time queries show the corrected state.
- Evidence paths should expose the correcting source and any superseded assertion chain when relevant.

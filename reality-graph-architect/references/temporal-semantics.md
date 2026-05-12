# Temporal Semantics

Reality Graph is bitemporal.

## Required Time Axes

- Valid time: when a fact is true in the modeled reality.
- Transaction time: when Reality Graph learned, changed, or revoked the assertion.

Use integer timestamps internally, preferably microseconds or nanoseconds since Unix epoch. Avoid floating-point time and localized wall-clock strings in the core engine.

## Assertions

Every assertion must support:

- Valid-time start.
- Valid-time end or open-ended interval.
- Transaction-time start.
- Transaction-time end or open-ended interval.
- Provenance and source IDs.
- Confidence.

Corrections must not overwrite history. Append a new event that closes or supersedes the previous transaction interval.

## Query Semantics

Point-in-time queries must define both axes:

- `valid_at`: the modeled-world instant or interval.
- `transaction_at`: the database knowledge instant.

If an API offers a default, prefer `transaction_at = latest` and require the caller to provide or consciously accept the valid-time behavior.

Query execution must be deterministic. Equal inputs and equal snapshots must produce equal outputs, including evidence path ordering.

## Property Tests

Add property-based tests for:

- Corrections preserve historical transaction-time answers.
- Open intervals include their start and exclude closed ends.
- Replaying events into a fresh index yields the same point-in-time answers.
- Event order with equal timestamps has a deterministic tie-breaker.

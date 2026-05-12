# ADR 0003: Bitemporal Model

## Status

Accepted

## Context

Reality Graph needs to answer both what was true in the modeled world and what the system knew at a given moment. A single timestamp cannot represent late-arriving sources, corrections, retractions, and historical analysis.

## Decision

Every assertion supports valid time and transaction time.

- Valid time describes when an assertion is true in the modeled reality.
- Transaction time describes when Reality Graph learned, changed, or revoked the assertion.

Point-in-time queries must account for both axes. Internally, timestamps are stored as integers.

## Consequences

- Facts and edges carry valid-time and transaction-time intervals.
- Corrections preserve historical transaction-time answers.
- Query APIs must accept or clearly default both temporal axes.
- Property-based tests should cover interval boundaries, corrections, replay, and deterministic tie-breaking.
- Indexes must support efficient temporal filtering.

# ADR 0001: Rust Core Is the Source of Truth

## Status

Accepted

## Context

Reality Graph needs deterministic temporal semantics, strong domain typing, replayable storage, cache-friendly indexes, and predictable performance. The repository may include Python tooling, frontend surfaces, schema definitions, and infrastructure, but graph correctness must not be split across languages.

## Decision

The Rust core is the source of truth for graph semantics, domain objects, event validation, temporal query behavior, and index construction.

Companion languages and services may call into Rust, generate inputs, or present outputs. They must not define conflicting fact semantics, temporal rules, or provenance requirements.

## Consequences

- Core IDs, predicates, timestamps, facts, events, and errors are explicit Rust types.
- Public Rust functions require tests.
- Python, frontend, and API layers should treat Rust outputs as authoritative.
- Cross-language schemas must reflect the Rust domain model.
- Unsafe Rust remains forbidden unless a future ADR and benchmarks justify it.

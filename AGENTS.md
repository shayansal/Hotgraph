# AGENTS.md

You are building Reality Graph, a 4D temporal knowledge graph engine.

## Non-negotiable architecture rules

1. The Rust core is the source of truth.
2. Every fact must have provenance.
3. Every assertion must support valid time and transaction time.
4. Never store an edge without confidence, source, and temporal metadata.
5. The system is event-sourced: writes append events first, then update indexes.
6. The graph must be queryable at any point in time.
7. Embeddings are auxiliary retrieval indexes, not source-of-truth facts.
8. APIs must return evidence paths and source IDs when answering AI-facing queries.
9. Unsafe Rust is forbidden unless an ADR justifies it and benchmarks prove necessity.
10. All public functions require tests.

## Performance principles

- Prefer cache-friendly contiguous data structures.
- Batch writes.
- Avoid unnecessary allocations.
- Use immutable append logs and compacted snapshots.
- Separate hot serving indexes from historical analytical storage.
- Use integer timestamps internally.
- Store source text/evidence separately from graph indexes.
- Keep query execution deterministic.

## Coding style

- Explicit types for domain objects.
- No stringly typed predicates inside the core engine.
- Use newtypes for IDs.
- Use structured errors.
- Add property-based tests for temporal semantics.
- Add benchmark tests for every index.

## Core commands

Run before every PR:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo test --all --release
```

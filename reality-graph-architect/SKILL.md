---
name: reality-graph-architect
description: Use when designing, implementing, or reviewing Reality Graph temporal graph core, events, provenance, indexes, APIs, AI retrieval, ingestion, benchmarks, or PR readiness checks.
---

# Reality Graph Architect

## Overview

Apply Reality Graph's architecture rules before changing graph semantics, storage, indexing, query execution, AI-facing APIs, ingestion, or benchmarks. Treat the Rust core as the source of truth and preserve bitemporal, provenance-first behavior.

## Workflow

1. Read `AGENTS.md` before editing code or docs.
2. Identify which boundary is affected: core model, events, storage, index, query, API, AI retrieval, ingest, benchmark, or simulation.
3. Load only the reference needed for the task:
   - Temporal behavior, point-in-time queries, valid time, transaction time: `references/temporal-semantics.md`
   - Facts, events, provenance, IDs, predicates, confidence, evidence: `references/data-model.md`
   - Index performance, compaction, PR benchmark expectations: `references/benchmark-targets.md`
4. Reject designs that make embeddings or generated summaries source-of-truth facts.
5. Add tests for every public function and property-based tests for temporal semantics.
6. Add or update benchmark coverage for every index change.
7. Run `scripts/run_all_checks.sh` before PR handoff when Bash is available.

## Architecture Gates

Before accepting a design or patch, verify:

- Facts and edges include provenance, source IDs, confidence, valid time, transaction time, and temporal metadata.
- Writes append immutable events before updating indexes.
- Query APIs can answer at a requested point in valid time and transaction time.
- AI-facing responses can return evidence paths and source IDs.
- Source text and evidence payloads are stored separately from hot graph indexes.
- Core predicates use explicit domain types, not bare strings.
- IDs use newtypes.
- Errors are structured and domain-specific.
- Query execution is deterministic.
- Unsafe Rust is absent unless an ADR and benchmarks justify it.

## Common Mistakes

- Do not flatten valid time and transaction time into one timestamp.
- Do not store source text inline in serving indexes.
- Do not let vector similarity results become facts without event-sourced assertion.
- Do not add a public Rust API without tests.
- Do not add a graph index without a benchmark target.

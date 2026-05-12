# Self-Revising Graph Mechanics

Reality Graph can inspect its own model of reality, but it must not silently rewrite truth. Self-revision jobs produce reviewable suggestions with audit entries. A suggestion may recommend a merge, recalibration, invalidation, consolidation, or refinement, but applying that change must happen through explicit graph events or operator approval.

## Jobs

The kernel self-revision engine scans for:

- entity deduplication candidates
- source trust recalibration signals
- ontology drift
- contradiction clusters
- summary invalidation
- memory consolidation
- stale beliefs
- dependency invalidation
- causal hypothesis refinement

Each job can run alone or as part of a full self-revision pass. Runs accept an incremental cursor so background workers can avoid rescanning old transaction-time changes.

## Suggestions

Every suggestion includes:

- stable suggestion ID
- job and suggestion kind
- target
- explanation
- supporting evidence
- dependency trace when relevant
- review requirement
- audit event ID

Suggestions default to review-required and never auto-apply. Destructive operations such as entity deduplication are only recommendations; they do not merge entities, delete atoms, rewrite belief history, or collapse contradictions.

## Audit Semantics

Each report contains an audit log entry for the run and an audit entry for every suggestion. The audit log states that the run emitted suggestions only. This preserves the invariant that no belief revision deletes history and no contradiction is silently collapsed.

## Relationship To The Kernel

Self-revision operates over `RealityAtom`, `ConflictSet`, `DependencyGraph`, belief revisions, summaries, agent memories, source provenance, and causal atoms. It is a model-improvement layer, not a truth mutation path. If a suggestion is approved, the eventual write path should append explicit events that can be replayed and audited.

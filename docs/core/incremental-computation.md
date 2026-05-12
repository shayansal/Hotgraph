# Incremental Computation

Reality Graph must update maintained knowledge continuously as events arrive. The
kernel does not recompute the whole world after every source, belief revision,
conflict, or dependency change.

## Flow

```text
KernelEvent
  -> append sequenced event
  -> apply deterministic delta
  -> update maintained views
  -> return IncrementalDelta
```

The append step gives every update a monotonic `IncrementalSequence`. The delta
then records which atoms, entities, sources, and maintained views changed.

## Maintained Views

The kernel maintains small deterministic views:

- graph adjacency by subject
- source-to-atom provenance lookup
- agent-scoped working set
- current belief state
- contradiction membership
- summary dependency staleness
- source trust scores
- causal and dependency view versions

Each view has its own last-updated sequence. Tests assert that a conflict update
can move contradiction and summary views without moving the graph adjacency view.

## Staleness

Summary atoms are tracked by the atoms they depend on. When a dependency is
revised, superseded, refuted, or contradicted, only summaries depending on the
affected atoms are marked stale.

This is the kernel-level version of the future maintained summary system:

```text
atom changed
  -> dependent summaries become stale
  -> retrieval/compiler can avoid stale summaries
  -> recomputation can be scheduled only for affected summaries
```

## Risk Propagation

Belief revisions walk the dependency graph and return impacted nodes in the
`IncrementalDelta`. Plan-like atoms are reported as `risky_plans` when an
upstream belief changes.

This keeps the strategy layer honest: downstream risk is dependency reasoning,
not a new fact about the world.

## Differential Dataflow Fit

Differential/incremental computation is the right long-term execution model for
high-throughput maintained views. The current kernel deliberately implements the
same shape locally first:

- sequenced input changes
- deterministic deltas
- maintained arrangements/views
- incremental invalidation
- replayable event log

A later Differential Dataflow backend should preserve these semantics and
accelerate view maintenance. It must not replace the kernel's bitemporal,
provenance, belief, contradiction, and truth-maintenance invariants.

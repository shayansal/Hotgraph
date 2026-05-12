# Reality Query VM

The first Reality Query VM is intentionally minimal. It proves the kernel
contract before API routes, UI, vector search, or GraphRAG exist.

## Supported Queries

The initial VM supports entity state:

```text
EntityState(entity, valid_at, known_at, ai_facing)
```

It also exposes the four native bitemporal questions that define Reality Kernel
truth:

```text
WhatIsTrueNow(entity, BitemporalTruth(valid_at, known_at), ai_facing)
WhatWasTrueAt(entity, valid_at, known_at, ai_facing)
WhatDidWeBelieveAt(entity, valid_at, believed_at, ai_facing)
WhenDidBeliefChange(atom_id)
WhatCaused(event_id, max_depth)
WhatMightHappenNext(event_id, max_depth)
WhatBreaksIfEventDoesNotOccur(event_id, max_depth)
```

The VM returns:

- supported atoms
- visible conflict sets
- evidence spans for returned atoms
- unsupported conclusions filtered out of AI-facing results
- belief changes for transaction-time revision queries
- causal paths and causal impact reports for strategy queries

Every state query returns the `BitemporalTruth` it executed against, so callers
can distinguish world-time truth from transaction-time belief.

## Native Operator VM

The kernel also exposes a native reasoning VM for AI-facing reality questions.
It is not Cypher, SPARQL, or RGQL. It is a compact operator model that compiles
directly to graph, temporal, belief, evidence, contradiction, permission,
dependency, causal, and simulation operations.

The first native query forms are:

```text
VerifyClaim(ClaimPattern)
WhatBreaksIfFalse(atom_id)
CausesOf(event_id, max_depth)
EffectsOf(event_id, max_depth)
```

Operators include:

```text
ValidAt(timestamp)
KnownAt(timestamp)
BeliefIn(states)
RequireEvidence
IncludeContradictions
AllowPermissions(labels)
DependencyTrace
CausalCauses(event_id, max_depth)
CausalEffects(event_id, max_depth)
CounterfactualAtomFalse(atom_id)
SimulationOnly
```

Return fields include:

```text
Belief
Evidence
Contradictions
DependencyTrace
AffectedBeliefs
Plans
Memories
Summaries
Agents
CausalPaths
SimulationImpact
```

This supports queries shaped like:

```text
VERIFY claim("A worked at B in 2023")
USING accepted_sources
VALID_AT "2023-06-01"
KNOWN_AT "2026-05-12"
RETURN belief, evidence, contradictions, dependency_trace
```

and:

```text
WHAT_BREAKS IF atom_123 IS_FALSE
RETURN affected_beliefs, plans, memories, summaries, agents
```

`WhatBreaksIfFalse` is dependency invalidation and counterfactual reasoning. It
must never be presented as a factual conclusion.

## Planning

The native VM emits a `NativeRealityPlan` with an execution strategy and trace.
Fully-bound conjunctive claim verification is marked as a
`LeapfrogTriejoinCandidate`, because worst-case optimal joins are a strong fit
for future graph-like conjunctive workloads.

The first implementation does not claim to implement Leapfrog Triejoin. It
records the correct lowering boundary while executing deterministically over
`PhysicalGraphStore` candidate sets, trie indexes, and temporal filters. Future
query planning can replace that execution step with a worst-case optimal join
implementation without changing the operator semantics.

## AI-Facing Behavior

When `ai_facing` is true, the VM filters out atoms that are not supported for AI
use. Unsupported atoms are returned in a separate field so the caller can inspect
why a conclusion was excluded.

Historical AI-facing queries evaluate support using the atom's belief state at
`known_at`. A claim that is superseded today can still be returned as an accepted
historical belief if it was supported at the requested transaction time.

This preserves the invariant:

No AI-facing query returns unsupported conclusions.

## Causal Behavior

Causal VM queries run on `CausalAtom` indexes, not normal graph adjacency.
`WhatBreaksIfEventDoesNotOccur` is always returned as counterfactual impact
analysis and must not be presented as fact.

## Future Work

The VM can later grow path queries and compiled RMP/RGQL plans. Those layers must
preserve the same kernel invariants.

# Dependency Semantics

Dependency edges make the Reality Kernel a truth-maintenance system rather than
a passive graph store. They record which atoms, answers, memories, plans,
summaries, and simulations rely on which upstream claims.

## Direction

Dependencies point from prerequisite to dependent:

```text
source atom -> claim atom -> derived belief -> answer -> simulation
```

If the source is retracted, the kernel walks downstream from the source and
returns every dependent node that needs review.

## Dependency Types

`DependencyType` captures why one node depends on another:

- `SupportedBy`: an upstream atom supports a claim.
- `DerivedFrom`: a downstream atom is derived from one or more upstream atoms.
- `ContradictedBy`: claims participate in a conflict.
- `SupersededBy`: one atom replaced another without deleting history.
- `Assumes`: a plan, answer, or simulation relies on an assumption.
- `Causes`: a causal event contributes to another event.
- `Enables`: an upstream condition enables a downstream claim or action.
- `Invalidates`: one atom invalidates or weakens another.

Each edge carries a strength in `0.0..=1.0`. Strength is dependency weight, not
a direct truth probability.

## Support Sets

`explain_support(atom_id)` returns the direct support set for an atom:

- supporting atom IDs,
- source IDs,
- evidence spans,
- dependency trace.

This is the minimum object an AI-facing answer needs before it can use a claim
as grounded context.

## Conflict Sets

`explain_conflict(atom_id)` returns all `ConflictSet` objects that contain the
atom. Conflicts are visible state. The kernel may rank or prefer a claim later,
but it does not erase the disagreement.

## Impact Cones

`compute_impact_if_retracted(atom_id)` returns an `ImpactCone`:

- impacted atoms,
- impacted generated answers,
- impacted simulations,
- invalidation trace,
- warning that the output is dependency reasoning, not fact.

The impact cone is review material. It tells maintainers and agents what may no
longer be supported if an upstream atom changes.

## Permission-Aware Dependencies

Dependency semantics participate in permission checks. A public summary that
depends on restricted atoms is treated as unsafe for public context compilation.
The summary can still exist in the graph, but AI-facing query paths must filter
it unless the caller can access the upstream evidence.

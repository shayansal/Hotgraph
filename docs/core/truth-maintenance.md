# Truth Maintenance

The Reality Kernel tracks what depends on each atom.

Dependencies can connect:

- atom to atom
- atom to answer
- answer to simulation
- atom to downstream memory, plan, or derived claim

## Typed Dependency Edges

Truth maintenance edges are typed and weighted:

```rust
pub struct DependencyEdge {
    pub from: AtomId,
    pub to: AtomId,
    pub dependency_type: DependencyType,
    pub strength: f32,
}
```

The edge direction is prerequisite to dependent. If a source supports a claim,
the edge points from the source atom to the claim atom. If a belief is derived
from three atoms, each upstream atom points to the derived belief atom.

Supported dependency types:

- `DerivedFrom`
- `SupportedBy`
- `ContradictedBy`
- `SupersededBy`
- `Assumes`
- `Causes`
- `Enables`
- `Invalidates`

`strength` is constrained to `0.0..=1.0`. It is not truth probability by
itself; it is dependency strength for impact analysis and ranking review work.

## Invalidation

When an atom is retracted or corrected, the kernel computes the transitive
dependents. The output is an invalidation trace, not an automatic destructive
rewrite.

The trace answers:

- Which atoms depended on this atom?
- Which answers used those atoms?
- Which simulations reused those answers?
- Which downstream objects need review?

## Impact Queries

`impact_if_retracted(atom_id)` is dependency reasoning. It is not a factual
claim and not a simulation result. Its output must be labeled with uncertainty.

The native collapse query is:

```text
IfSourceFalseWhatCollapses(source_atom_id)
```

It answers:

- Which beliefs collapse?
- Which memories are unsupported?
- Which plans need review?
- Which generated answers become invalid?
- Which simulations used invalidated context?

The query returns a `TruthCollapseReport` with collapsed atoms, beliefs,
memories, plans, answers, simulations, and the typed dependency trace. It does
not mutate the graph and does not assert that the source is false. It models the
epistemic consequence of the intervention: if this source is false, what depends
on it?

## Contradictions

Contradictions are represented as `ConflictSet` objects. The kernel never
collapses conflicts silently. Query results include conflicts when visible atoms
participate in unresolved or partially resolved conflicts.

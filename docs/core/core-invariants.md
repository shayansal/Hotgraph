# Core Invariants

Reality Graph's kernel is the source of truth for AI-facing reality state. The
kernel stores claims as Reality Atoms and rejects shapes that would make time,
provenance, belief, permissions, or revision history ambiguous.

## Atom Invariants

Every persisted atom must have:

- `valid_time`: when the claim is true in the modeled world.
- `transaction_time`: when the system learned, stored, revised, or rejected the claim.
- `belief_state`: the lifecycle state of the claim.
- `confidence`: a bounded confidence score.
- `SourceRef`: at least one source identifier.
- `EvidenceSpan`: at least one exact evidence span.
- `PermissionLabel`: the permission boundary for the atom.
- `TaintLabel`: whether the atom is trusted, untrusted, risky, or poisoned.

The builder rejects atoms missing valid time, transaction time, provenance, or
confidence. Simulation atoms must be labeled as simulation, never as fact.

## Derived Atom Invariant

A `ClaimType::Derived` atom must name its dependencies. Derived atoms represent
beliefs, summaries, inferences, or other downstream products. Without
dependencies, the kernel cannot answer what breaks when upstream evidence is
retracted.

## Memory Trace Invariant

An `AgentMemory` atom must include an `ExtractionTrace`. A memory write is still
a graph event with provenance: the trace records the deterministic extractor,
review process, or agent workflow that produced the memory. The system must
revise memory by adding new atoms or belief revisions, not by deleting the old
memory.

## Belief Revision Invariant

Belief revision never deletes history. Supersession, dispute, retraction, and
refutation are recorded as revisions at transaction time. Historical queries can
therefore distinguish:

- what was true in the modeled world,
- what the system knew then,
- what the system believes now,
- when the belief changed.

## Contradiction Invariant

Contradictions are represented as `ConflictSet` values. The kernel does not
silently collapse disagreement into one preferred fact. Query results preserve
both sides when the claims are visible.

## Permission Invariant

Permission filtering is transitive through summaries. A public summary that
depends on a restricted atom is not safe to return to a public query unless the
caller is allowed to see the upstream evidence. Summaries must not leak
restricted source content through compressed wording.

## AI-Facing Invariant

An AI-facing answer cannot contain unsupported conclusions. Returned atoms must
carry source IDs, evidence spans, belief state, valid time, transaction time,
and contradiction context where relevant. Simulation output must be labeled as
simulation or impact analysis, never as fact.

# Reality Atom

The Reality Kernel does not treat an edge as the base unit of truth. Its base
unit is the Reality Atom: a bitemporal, evidence-backed claim about reality.

A normal graph edge says:

```text
A --WORKED_AT--> B
```

A Reality Atom says:

```text
Claim: Person A worked at Company B.
World validity: 2021-02-01 through 2024-08-31.
System knowledge: learned later, possibly revised later.
Evidence: source IDs and exact spans.
Belief state: accepted, disputed, superseded, or retracted.
Dependencies: memories, answers, plans, and simulations that rely on it.
AI usage: whether an agent can safely use it for planning.
```

## Required Fields

Every atom must include:

- `id`
- `subject`
- `predicate`
- `object`
- `valid_time`
- `transaction_time`
- `confidence`
- at least one `SourceRef`
- at least one `EvidenceSpan`
- `belief_state`
- `claim_type`
- permission and taint labels
- AI usage policy

The kernel rejects atoms without valid time, transaction time, provenance, or
confidence.

## Universal Primitive

Edges, facts, memories, events, source claims, summaries, hypotheses, and
simulations are specialized forms of Reality Atoms. They differ by `claim_type`,
belief state, dependency links, and AI usage policy. They do not get special
permission to bypass provenance or bitemporal semantics.

## Safety Rule

Simulation atoms must be labeled as simulation-only. They are never accepted as
facts, even when useful for planning.

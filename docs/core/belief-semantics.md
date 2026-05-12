# Belief Semantics

Reality Graph stores claims, not naive facts. A claim's current treatment is
represented by `BeliefState`.

## Belief States

- `Candidate`: proposed but not accepted for AI-facing conclusions.
- `Accepted`: currently supported by evidence.
- `Disputed`: evidence-backed but in active conflict. AI-facing context may
  include it only with the visible conflict set and source evidence.
- `Superseded`: replaced by later evidence while preserving history.
- `Retracted`: invalidated by correction or source withdrawal.
- `Refuted`: rejected by stronger evidence while preserving the prior belief.
- `Simulated`: useful for counterfactual reasoning but not a fact.
- `Unknown`: explicitly unresolved or not yet knowable.

These states are not true/false labels. They are lifecycle states for how the
kernel should treat a claim at a transaction-time horizon.

## Revision Rule

No belief revision deletes history. Supersession and retraction add revision
records and dependency effects; they do not erase the prior atom.

`Refuted` also preserves history. The system must still answer what it believed
before the refutation and when the belief changed.

## Conflict Rule

The kernel does not pick one claim just because multiple sources disagree. It
keeps the competing atoms, their source IDs, their confidence values, and the
visible `ConflictSet`.

For example:

- Claim A: Company X acquired Company Y on March 1.
- Claim B: The acquisition was announced on March 1 but closed on June 30.
- Claim C: Regulators later blocked the deal.

The correct behavior is to model all three claims on the timeline, revise the
March acquisition claim when later evidence refutes it, and preserve the prior
belief for historical queries.

## AI-Facing Rule

AI-facing queries return accepted atoms and disputed evidence-backed atoms with
conflict context. Candidate, retracted, refuted, superseded, simulated, unknown,
poisoned, prompt-injection-risk, and unsupported hypothesis atoms are returned
separately as unsupported conclusions or warnings.

No AI-facing query may silently return an unsupported conclusion as fact.

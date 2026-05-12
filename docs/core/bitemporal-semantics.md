# Bitemporal Semantics

Reality Kernel atoms carry two independent time intervals.

`valid_time` describes when the claim applies in the modeled world.

`transaction_time` describes when Reality Graph knew, recorded, revised, or
invalidated the claim.

This distinction lets the kernel answer both:

- What was true in the world at time `T`?
- What did the system know at transaction time `K`?

## Visibility

An atom is visible for a bitemporal query only when:

```text
valid_time contains valid_at
transaction_time contains known_at
```

Visibility is not the same as belief. A visible atom may still be disputed,
superseded, retracted, tainted, or unsafe for AI planning.

## Native Truth Primitive

The kernel treats bitemporal truth as an execution primitive, not metadata.
Queries carry a `BitemporalTruth`:

```text
BitemporalTruth(valid_at, known_at)
```

This pair answers different questions depending on intent:

- What is true now? Use the caller's current world time and current transaction
  time.
- What was true on March 1, 2024? Use `valid_at = 2024-03-01` and the selected
  transaction horizon.
- What did we believe on March 1, 2024? Use `known_at = 2024-03-01`; optionally
  bind `valid_at` when the world time is also constrained.
- When did our belief change? Read the atom's transaction-time belief revision
  log.

The two axes must remain explicit because an atom can be true in the modeled
world before Reality Graph learns it, and a later revision can change what the
system believes without deleting the earlier belief.

## Historical Knowledge

Belief revisions do not delete history. If an atom was accepted at transaction
time 150 and superseded at transaction time 300, the kernel can still explain
what was believed at transaction time 150.

## Integer Time

The kernel uses integer timestamps internally. Calendar parsing belongs outside
the kernel boundary.

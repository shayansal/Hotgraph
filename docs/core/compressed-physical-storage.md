# Compressed Physical Storage

Reality Kernel keeps the semantic model assertion-first, but the execution layer
must not depend on a naive map-shaped graph forever. The physical store uses
dense ordinals and multiple layouts over the same atoms.

## Layouts

The kernel-level `PhysicalGraphStore` declares these physical layers:

- append-only event log
- columnar atom store
- compressed adjacency lists
- temporal interval indexes
- roaring-style bitmap candidate sets
- trie indexes for joins
- memory-mapped snapshot descriptor
- hot working-set cache
- cold historical segment descriptor
- vector/source sidecar descriptor

The first implementation is in-memory, deterministic, and compact enough to
prove the query shape. It intentionally does not claim to be a durable mmap
format or a real Roaring bitmap implementation yet.

## Atom Table

Atoms are sorted deterministically and assigned dense `AtomOrdinal` values. The
columnar table stores:

```text
atom_id
subject_id
predicate_id
object_id
valid_from
valid_to
tx_from
tx_to
confidence
belief_state
context_id
source_set_id
```

Open-ended intervals are encoded with an internal sentinel, so point-in-time
filters stay integer-only.

## Indexes

The first physical indexes are:

```text
subject -> outgoing atom ordinals
object -> incoming atom ordinals
predicate -> atom ordinals
source -> atom ordinals
valid_start -> atom ordinals
tx_start -> atom ordinals
(subject, predicate, object) -> atom ordinals
contradiction_cluster -> atom ordinals
dependency -> atom ordinals
```

Candidate sets are sorted, deduplicated ordinal vectors with bitmap-like
intersection and union operations. This gives the query VM a stable candidate
interface now and leaves room to replace the backing set with a real Roaring
bitmap later.

## Query Use

Fully-bound native claim verification uses the trie index first, then intersects
temporal candidate sets and applies belief, evidence, contradiction, and
permission operators.

That is the boundary where future worst-case optimal join execution, including a
Leapfrog Triejoin implementation, should plug in.

## Hot And Cold Data

The current descriptors track the intended split:

- hot working-set cache: atom ordinals expected to serve low-latency queries
- cold historical segment store: transaction-time ranges for old data
- memory-mapped snapshot descriptor: future zero-copy snapshot format
- vector/source sidecar: source and embedding storage kept outside graph truth

These descriptors are deliberately small. The invariant is that physical storage
accelerates Reality Kernel semantics; it does not replace provenance,
bitemporal, belief, contradiction, dependency, permission, or simulation rules.

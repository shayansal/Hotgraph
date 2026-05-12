# Reality Graph: Bitemporal Knowledge Substrate for AI Agents

## Abstract

AI agents need durable memory that distinguishes reality, evidence, belief, and
time. Reality Graph proposes a bitemporal knowledge substrate in which the Rust
core stores source-backed assertions rather than naive facts. Each assertion has
valid time, transaction time, confidence, provenance, context, and revision
status. Writes are event-sourced, indexes are materialized from append-only
events, and AI-facing responses return evidence paths instead of ungrounded
claims. The paper evaluates whether this substrate improves temporal question
answering, source faithfulness, contradiction handling, and long-running agent
memory compared with flat vector retrieval and transcript memory.

## Related Work

Bitemporal databases separate the time a claim is true in the modeled world from
the time a system learned the claim. Temporal knowledge graphs and the
[Temporal Graph Benchmark](https://arxiv.org/abs/2307.01026) provide evaluation
tools for dynamic graph learning, but they usually optimize prediction tasks
rather than agent-facing provenance and belief revision. Retrieval-augmented
generation systems retrieve source text for model grounding, while graph RAG
systems such as [GraphRAG](https://arxiv.org/abs/2404.16130) add graph
structure and community summaries. Reality Graph differs by making bitemporal
state, event replay, source IDs, and assertion provenance core database
invariants.

## Method

Reality Graph models `Entity`, `Assertion`, `Source`, `Event`, `CausalLink`,
and `AgentMemory` as explicit Rust domain types. Assertions encode subject,
predicate, object, valid interval, transaction interval, confidence, source IDs,
context, and status. Writes first validate commands, append deterministic
events, then update materialized indexes. Query execution can reconstruct graph
state at a valid time and a transaction time, making two questions distinct:
what was true then, and what did the system know then?

The substrate exposes evidence packs for AI agents. An evidence pack contains
entities, assertions, source excerpts, graph paths, contradiction warnings, and
generation metadata. Embeddings and summaries are auxiliary retrieval structures
and never become source-of-truth facts.

## Datasets

Primary datasets are deterministic fixtures in `evals/fixtures` and synthetic
worlds from `rg-worldgen`: temporal employment, multi-hop company ownership,
geopolitical events, contradictory evidence, supply-chain dependency, and agent
conversation memory. External temporal graph datasets can be added through the
`rg-eval` harness when license and format permit.

## Experiments

The core experiment compares Reality Graph against vector-only RAG, BM25-only
retrieval, hybrid retrieval, graph-only retrieval, and transcript memory. Tasks
include point-in-time state reconstruction, historical knowledge reconstruction,
source-backed claim verification, contradictory assertion detection, and
evidence pack generation. Metrics include temporal correctness, evidence recall,
evidence precision, citation faithfulness, contradiction F1, p95 query latency,
and replay throughput.

## Ablations

- Remove valid time and keep only transaction time.
- Remove transaction time and keep only valid time.
- Remove source IDs from query output.
- Replace assertion status with destructive updates.
- Replace event replay with mutable in-place storage.
- Disable contradiction detection.
- Treat vector retrieval as truth rather than candidate generation.

## Limitations

The first implementation is single-node and uses simple storage and indexes.
Synthetic fixtures are useful for deterministic correctness but do not replace
large messy enterprise corpora. Confidence scoring is initially rule-driven and
does not prove real-world truth. Source-backed assertions still depend on
extraction quality and human or agent review. Bitemporal modeling adds schema
and query complexity that must be hidden by higher-level APIs for broad
adoption.

## Reproducibility Checklist

- Publish crate versions, commit SHA, Rust toolchain, and feature flags.
- Publish fixture datasets, synthetic world seeds, ontology files, and graph
  snapshots.
- Save event logs used to build every reported graph state.
- Save every query, retrieval trace, evidence pack, and answer evaluation.
- Report hardware, memory limits, and storage backend.
- Report all baselines with identical source corpora and time constraints.

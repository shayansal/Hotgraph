# Reality Graph Paper Stack

This directory contains research paper drafts for positioning Reality Graph as a
credible AI memory, retrieval, reasoning, and simulation substrate.

The papers are not marketing collateral. Each draft is organized around a claim
that should be measurable through the repository's crates, deterministic
fixtures, and evaluation harnesses.

## Papers

1. [Reality Graph: Bitemporal Knowledge Substrate for AI Agents](01-bitemporal-knowledge-substrate.md)
2. [Memory Turing Test: Evaluating Persistent Agent Memory](02-memory-turing-test.md)
3. [Temporal GraphRAG: Evidence Retrieval Under Valid and Transaction Time](03-temporal-graphrag.md)
4. [Belief Revision for LLM Agents Using Source-Backed Temporal Graphs](04-belief-revision.md)
5. [Reality Gym: Training Agents in Noisy Dynamic Worlds](05-reality-gym.md)
6. [Cognitive Cache: Low-Latency Memory Retrieval for AI Agents](06-cognitive-cache.md)
7. [Context Compilation: Adaptive Retrieval Plans for Tool-Using Models](07-context-compilation.md)

## Shared Evaluation Principles

- Compare against vector-only, keyword-only, hybrid, graph-only, temporal graph,
  transcript memory, summary memory, and full Reality Graph baselines.
- Report evidence recall, evidence precision, temporal correctness,
  contradiction handling, citation faithfulness, answer quality, latency, and
  cost.
- Preserve seeds, fixture versions, graph snapshots, query traces, retrieval
  plans, and generated context packs for reproducibility.
- Treat graph outputs as evidence selection and state reconstruction, not as a
  substitute for model reasoning.
- Separate measured results from target goals. Ambition belongs in discussion;
  claims belong in experiments.

## Reference Threads

The drafts are designed to cite and compare against work such as
[GraphRAG](https://arxiv.org/abs/2404.16130),
[HippoRAG](https://arxiv.org/abs/2405.14831),
[LightRAG](https://arxiv.org/abs/2410.05779), and
[Temporal Graph Benchmark](https://arxiv.org/abs/2307.01026). Each final paper
should replace these seed references with a full bibliography before submission.

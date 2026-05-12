# Reality Graph Evaluations

This directory contains deterministic fixture datasets for evaluating retrieval,
temporal reasoning, contradiction handling, agent memory, causal/supply-chain
traversal, and adaptive routing strategies.

The `rg-eval` crate parses the files in `fixtures/` and compares vector-only,
keyword-only, graph-only, temporal graph, hybrid, and adaptive routed retrieval.

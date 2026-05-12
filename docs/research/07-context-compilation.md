# Context Compilation: Adaptive Retrieval Plans for Tool-Using Models

## Abstract

Tool-using models should not call one generic search endpoint for every
question. Context Compilation treats retrieval as a planning problem: given a
question, agent state, temporal constraints, ontology, trust policy, latency
budget, cost budget, and available tools, the system emits a compact,
source-backed, temporally-correct evidence pack. The compiler chooses retrieval
operators and records a trace explaining each decision.

## Related Work

RAG systems retrieve text chunks by vector or keyword similarity. GraphRAG adds
graph structure and community summaries, while memory-retrieval systems such as
HippoRAG emphasize activation over graph neighborhoods. LightRAG highlights
efficient graph and vector retrieval with incremental updates. Context
Compilation frames these retrieval styles as operators in a query plan rather
than competing products.

## Method

`rg-retrieval-compiler` defines `QueryIntent`, `RetrievalPlan`,
`RetrievalOperator`, `RetrievalBudget`, `RetrievalTrace`, and
`EvidencePackCompiler`. Operators include vector search, keyword search,
temporal filter, graph expansion, path search, community search, causal
expansion, contradiction check, rerank, compress, and cite. The planner selects
operators based on query intent, available indexes, temporal constraints,
confidence requirements, and budget. Every plan emits a trace for debugging and
offline learning.

## Datasets

Datasets come from multi-hop company ownership, temporal employment,
geopolitical events, agent conversation memory, supply-chain dependency, and
contradictory evidence fixtures. Additional workloads from Reality Gym test
whether compiled context improves downstream actions, not only answer quality.

## Experiments

Compare vector-only retrieval, keyword-only retrieval, hybrid retrieval,
graph-only retrieval, temporal graph retrieval, static GraphRAG-style retrieval,
and adaptive context compilation. Simple factual questions should not regress
against vector-only retrieval, while multi-hop, temporal, causal, and
contradiction-heavy questions should improve. Metrics include answer accuracy,
evidence recall, evidence precision, temporal correctness, citation
faithfulness, p95 latency, token cost, and retrieval-trace correctness.

## Ablations

- Remove query intent classification.
- Remove temporal filter operator.
- Remove graph expansion.
- Remove contradiction check.
- Remove compression.
- Use a fixed retrieval plan for all questions.
- Disable retrieval traces and learning signals.

## Limitations

Intent classification can choose the wrong plan. More operators can increase
latency and complexity. Compression may hide useful nuance if evidence
preservation policies are weak. Adaptive routing should be judged by eval
improvement and cost, not by the elegance of the plan trace.

## Reproducibility Checklist

- Save query inputs, budgets, ontology settings, and trust policies.
- Save retrieval plans, operator outputs, traces, and evidence packs.
- Publish baseline retrieval configurations.
- Report simple, multi-hop, temporal, causal, and contradictory subsets
  separately.
- Report token cost and latency per operator.
- Run offline replay before accepting any learned routing policy.

# Temporal GraphRAG: Evidence Retrieval Under Valid and Transaction Time

## Abstract

GraphRAG improves retrieval by adding graph structure, but agent questions often
depend on time. Temporal GraphRAG extends graph retrieval with valid time,
transaction time, temporal community summaries, contradiction detection, and
evidence pack generation. The central claim is that graph retrieval should answer
not only "what evidence is related?" but "what evidence was true then, known
then, and still supported now?" Reality Graph evaluates this claim on temporal
employment, ownership, geopolitical, and supply-chain tasks.

## Related Work

[GraphRAG](https://arxiv.org/abs/2404.16130) uses graph extraction and
community summaries for broad corpus sensemaking. [LightRAG](https://arxiv.org/abs/2410.05779)
combines graph structures with vector retrieval and incremental update ideas.
Temporal graph benchmarks such as [TGB](https://arxiv.org/abs/2307.01026)
evaluate dynamic graph learning. Temporal GraphRAG uses these ideas but changes
the retrieval contract: every result is filtered and explained under valid-time
and known-at constraints.

## Method

The method builds assertions from reviewed source-backed candidates, indexes
them by subject, predicate, object, valid interval, transaction interval,
context, source, and confidence, then compiles retrieval plans. Operators include
keyword search, vector search, temporal filter, graph expansion, path search,
community search, contradiction check, rerank, compress, and cite. Community
summaries are versioned by valid time and transaction time; assertion changes
invalidate affected summaries rather than forcing full recomputation.

## Datasets

Datasets include temporal employment records, multi-hop company ownership,
geopolitical event timelines, contradictory evidence, and supply-chain
dependency graphs. Each dataset contains sources, assertions, valid intervals,
transaction times, and gold evidence packs. Additional external corpora can be
converted when source licensing allows redistribution.

## Experiments

Tasks include historical QA, known-at QA, multi-hop relationship retrieval,
broad community questions, contradiction-aware answers, and evidence recall
under date constraints. Baselines are vector-only RAG, BM25-only retrieval,
hybrid retrieval, static GraphRAG-style retrieval, graph-only retrieval, and
Temporal GraphRAG. Metrics include answer accuracy, temporal correctness,
evidence recall, evidence precision, citation faithfulness, p95 latency, and
context token cost.

## Ablations

- Remove known-at filtering.
- Remove valid-time filtering.
- Use static community summaries only.
- Disable contradiction retrieval.
- Disable graph expansion.
- Disable vector retrieval.
- Disable adaptive operator routing.

## Limitations

Temporal GraphRAG inherits extraction errors from upstream ingestion. Temporal
community summaries may become stale if invalidation misses a dependency.
Temporal filters can reduce recall when source dates are incomplete. Some
questions are simple enough that graph retrieval should not beat vector-only
RAG; the retrieval compiler must detect those cases rather than forcing graph
work everywhere.

## Reproducibility Checklist

- Release source documents or synthetic equivalents.
- Release assertion extraction outputs and review decisions.
- Release graph snapshots for each transaction-time checkpoint.
- Save retrieval plans, operator traces, and community summary versions.
- Report temporal correctness separately from answer accuracy.
- Report latency with and without cached summaries.

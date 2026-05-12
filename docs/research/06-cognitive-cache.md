# Cognitive Cache: Low-Latency Memory Retrieval for AI Agents

## Abstract

AI agents cannot wait seconds for every memory operation. Cognitive Cache is a
permission-aware, contradiction-aware, temporal cache for hot agent working sets,
entity state, paths, evidence packs, and compressed context. The cache targets
agent cognition latency rather than generic database throughput: fast recall for
recent tasks, recurring entities, and stable evidence packs while preserving
time, permissions, and invalidation semantics.

## Related Work

Database caching, vector index caching, and application-level memoization improve
latency but usually ignore temporal validity, belief revision, source
permissions, and contradiction invalidation. RAG systems often cache embeddings
or final prompts, while graph systems cache adjacency and path expansions.
Reality Graph's cognitive cache combines these ideas with agent-specific working
sets and evidence-pack invalidation.

## Method

`rg-cognitive-cache` defines `AgentWorkingSet`, `EntityHotCache`,
`EvidencePackCache`, `PathQueryCache`, `TemporalInvalidationIndex`,
`PermissionAwareCacheKey`, `ContradictionAwareInvalidation`, and
`SummaryStalenessTracker`. Cache keys include tenant, agent, permission epoch,
temporal constraints, query shape, and source access policy. Invalidation is
triggered by event append, assertion update, redaction, contradiction discovery,
summary staleness, and permission changes.

## Datasets

Latency workloads are derived from agent memory tasks, entity timelines,
multi-hop ownership graphs, supply-chain dependency graphs, and repeated
evidence-pack requests. Stress tests include hot working-set loops, cold-cache
misses, high-cardinality tenants, and permission changes.

## Experiments

Measure p50, p95, and p99 latency for hot memory recall, entity state queries,
path queries, and evidence pack generation. Compare uncached retrieval,
generic memoization, graph-only cache, vector-cache-only, and Cognitive Cache.
Correctness tests verify that redactions, contradiction updates, time changes,
and permission epoch changes prevent stale leakage.

## Ablations

- Remove permission fields from cache keys.
- Remove temporal fields from cache keys.
- Remove contradiction-aware invalidation.
- Remove summary staleness tracking.
- Remove agent working set.
- Cache final prompts without citation maps.

## Limitations

Caching increases operational complexity and memory pressure. Conservative
invalidation may reduce hit rate; aggressive caching risks stale evidence.
Latency targets depend on workload locality and hardware. The cache is not a
substitute for optimizing indexes, replay, and retrieval operators.

## Reproducibility Checklist

- Publish workload generator seeds and cache configuration.
- Report cache hit rate, miss rate, eviction rate, and invalidation causes.
- Report warm-cache and cold-cache latency separately.
- Run redaction and permission-change leakage tests.
- Report memory overhead per cached object type.
- Publish p50, p95, and p99 latency with hardware details.

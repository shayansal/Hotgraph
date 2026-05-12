# Threat Model

Hotgraph stores source-backed beliefs, agent memory, and evidence. The primary
security risk is not only data loss; it is returning the wrong evidence to the
wrong principal or letting poisoned sources silently shape AI-facing context.

## Assets

- Event logs, WAL segments, snapshots, and backups.
- Source records and evidence spans.
- Tenant-scoped entities, assertions, memories, summaries, and context packs.
- API keys, service account identities, KMS key references, and audit records.
- Retrieval traces, contradiction sets, and dependency traces.

## Trust Boundaries

- External clients to API server.
- API server to durable storage.
- API server to vector/search sidecars.
- API server to object storage and backup targets.
- MCP/tool callers to write-capable graph operations.
- Tenant data boundaries inside indexes, summaries, and evidence packs.

## Required Controls

- Auth enabled by default outside explicit development mode.
- Scoped API keys and service accounts for read/write/admin operations.
- Tenant IDs as first-class storage and query fields.
- Source-level and memory-level ACL checks before evidence leaves the system.
- Redaction-aware query and summary invalidation.
- Secret redaction in logs, traces, errors, and metrics labels.
- Signed source records for high-trust ingestion paths.
- Tainted-source propagation through summaries and evidence packs.
- KMS-backed envelope encryption for event logs, snapshots, source stores, and
  backups.
- Deny-by-default MCP/tool write operations with policy gates.

## Open Risks

- Current confidential-mode test crypto must not be used for production
  encryption.
- Prompt-injection detection is defense-in-depth only and must not replace ACLs.
- Multi-replica mode needs split-brain prevention and single-writer guarantees.
- Production readiness requires an external penetration test and adversarial
  memory-poisoning evaluation.


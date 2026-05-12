# Hotgraph Production-Grade Release Gates

Hotgraph can only claim production-grade status for a release when every gate in
this document has dated evidence tied to a commit SHA, image digest, dataset
seed, and operator sign-off. A partial pass is a fail.

## 1. Durability

Pass criteria:

- Every acknowledged write survives process crash, machine restart, snapshot
  restore, and WAL-tail replay.
- WAL records have monotonic LSNs, event checksums, schema version, transaction
  timestamp, and idempotency metadata.
- Durable serving state can be rebuilt from WAL plus snapshots and matches the
  original deterministic state hash.
- Single-writer fencing prevents two writers from acknowledging writes under the
  same lease epoch.

Required evidence:

- Crash matrix artifact for append, fsync, materialization, snapshot publish,
  compaction, and backup upload kill points.
- State hash parity report after restore.
- Fencing-token test report.

Current status: failing until process-level crash and power-failure evidence is
produced.

## 2. Recoverability

Pass criteria:

- RPO is less than or equal to 60 seconds for the validated deployment profile.
- RTO is less than or equal to 15 minutes for the validated dataset size.
- Restore refuses to target a live writer or non-empty data directory.
- Restored indexes and historical queries match the source environment.

Required evidence:

- Monthly restore drill report with timing, logs, source backup manifest,
  restored state hash, and query parity checks.
- Backup integrity verification artifact.

Current status: failing until restore drills exist for production-like data.

## 3. Bitemporal Correctness

Pass criteria:

- Valid-time and transaction-time invariants survive append, replay, migration,
  snapshot restore, and follower catch-up.
- Historical belief, contradiction preservation, dependency invalidation, and
  provenance are stable across restart.
- No AI-facing result includes an unsupported conclusion.

Required evidence:

- Property-based storage/replay/query test report.
- Golden query parity report before and after restore.

Current status: failing until the property suite covers durable replay and
restore paths.

## 4. Isolation And Security

Pass criteria:

- Tenant IDs are first-class in storage and index keys.
- Governance checks run on every query, path, source, evidence-pack, AI context,
  reality API, and MCP/tool response path.
- Redacted sources cannot leak through summaries, caches, contradictions, or
  evidence packs.
- Production encryption uses envelope encryption through `KmsProvider` and
  audited AEAD, not custom test crypto.

Required evidence:

- Cross-tenant adversarial test artifact.
- Redaction propagation test artifact.
- Key rotation and wrong-key/tamper test artifact.
- Threat model and penetration test report with no open critical/high findings.

Current status: failing until governance is wired through every read path and
AEAD/KMS replaces test crypto.

## 5. Operability

Pass criteria:

- OpenTelemetry spans and Prometheus metrics cover write, fsync, materialization,
  query planning, governance, evidence-pack generation, backup, restore, and
  follower lag.
- Dashboards show SLO health, WAL lag, snapshot age, backup freshness, restore
  verification, query latency, rate limits, per-tenant usage, and governance
  denies.
- Alerts link directly to runbooks.
- A non-core engineer can resolve common incidents from runbooks during drills.

Required evidence:

- Dashboard screenshots or exported JSON.
- Alert rule set.
- On-call drill report.

Current status: failing until operational drill evidence exists.

## 6. Scale Envelope

Pass criteria:

- The release has reproducible benchmark artifacts for 10M, 50M, and 100M
  assertions.
- Artifacts include hardware profile, dataset seed, build profile, commit SHA,
  image digest, p50/p95/p99 write latency, p50/p95/p99 query latency,
  evidence-pack latency, replay time, restore time, RSS, disk amplification, and
  compaction pause.
- Accepted p95 latency regressions greater than 7 percent block the release.

Required evidence:

- Benchmark JSONL artifacts.
- Markdown release report.
- Regression comparison against the previous accepted release.

Current status: failing until large-scale benchmark evidence exists.

## 7. Dirty And Adversarial Data Pilot

Pass criteria:

- A production-like pilot includes malformed documents, duplicate floods,
  timestamp abuse, replay attacks, schema edge cases, contradictory evidence,
  prompt-injection sources, and cross-tenant probing.
- Critical and high findings are closed before release.
- Cost profile, operator burden, and user-visible correctness are documented.

Required evidence:

- Pilot postmortem.
- Closed critical/high issue list.
- Cost and operator burden report.

Current status: failing until a real pilot is run and reviewed.

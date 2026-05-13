# Production Readiness Gates

Hotgraph is not production-ready until every gate in this document has a dated
evidence artifact. A release manager must mark each gate `pass`, `fail`, or
`waived with owner and expiry`. A missing artifact is a fail.

## Gate 0: Release Decision Record

For every production candidate, create `docs/ops/releases/<version>.md` with:

- git commit SHA, container digest, schema version, and migration version
- dataset scale used for validation
- backup and restore drill IDs
- security review ID
- benchmark report location
- incident drill report location
- known limitations and accepted risk owners

Use `docs/ops/releases/TEMPLATE.md` as the starting point. The default decision
for every gate is `fail`; release managers must replace each row with dated
evidence before claiming production readiness.

Pass: all linked artifacts exist and match the candidate commit.

Fail: any artifact is missing, from another commit, or not reproducible.

## Gate 1: Durability

Requirement: no acknowledged write is lost across process crash, host reboot, or
power-failure-equivalent WAL truncation.

Checks:

- WAL record includes sequence number, event ID, transaction time, idempotency
  key when supplied, payload checksum, and schema version.
- Acknowledged writes are persisted according to the configured fsync policy.
- Recovery truncates only corrupt tail bytes and never rewrites good records.
- Replay from WAL rebuilds graph state and indexes deterministically.

Pass: crash-recovery matrix passes and the restore hash matches the pre-crash
state for every acknowledged record.

Fail: any acknowledged record disappears, reorders silently, duplicates without
idempotency handling, or replays to a different deterministic state hash.

## Gate 2: Recoverability

Targets:

- RPO: less than or equal to 60 seconds for scheduled backups.
- RTO: less than or equal to 15 minutes for the documented single-node restore
  workflow at the validated scale envelope.

Checks:

- Snapshot manifest records schema version, WAL LSN boundary, event checksum,
  last event ID, and deterministic graph-state hash.
- Restore uses `snapshot + WAL tail` or backup artifact and verifies event
  checksum plus graph-state hash.
- Monthly timed restore drill produces logs, timing, checksum report, and
  post-restore query parity report.

Pass: latest drill satisfies RPO/RTO and restored query parity is true.

Fail: restore requires undocumented manual steps, misses RPO/RTO, or lacks
checksum/query parity evidence.

## Gate 3: Bitemporal Correctness

Requirement: valid-time and transaction-time invariants survive restart, WAL
replay, snapshot restore, backup restore, and index rebuild.

Checks:

- Every assertion has valid time and transaction time.
- Point-in-time queries distinguish `valid_at` from `known_at`.
- Historical belief tests pass after replay and restore.
- Restored state hash matches original state hash.

Pass: kernel, event, storage, index, and query tests pass in debug and release.

Fail: any restored query returns a fact under a different valid/known-time
interpretation.

## Gate 4: Isolation And Security

Requirements:

- Tenant boundaries are enforced before storage/index/query/evidence responses
  leave the process.
- KMS-backed key handling is used for production secrets and data keys.
- API keys are never logged or stored in plaintext.
- Redaction is irreversible in returned sources, summaries, and evidence packs.

Pass: automated cross-tenant leakage tests, redaction tests, and key-rotation
tests pass; the security review and penetration test have no open critical or
high findings.

Fail: plaintext secret exposure, cross-tenant evidence leakage, missing KMS
configuration, or unresolved critical/high finding.

## Gate 5: Operability

Requirements:

- SLOs exist for availability, write durability lag, write latency, query
  latency, evidence-pack latency, replay time, restore time, and error rate.
- Dashboards show golden signals plus WAL recovery status and backup freshness.
- Alerts are actionable and linked to runbooks.
- On-call drill results show a non-core developer can follow the runbook.

Pass: dashboards and alerts are deployed, runbooks exist for the top 10
incidents, and the latest on-call drill meets the response objective.

Fail: vanity-only alerts, missing runbooks, no drill, or unresolved alert noise.

## Gate 6: Scale Envelope

Validation levels:

- 10M assertions: required before private beta.
- 50M assertions: required before paid production pilot.
- 100M assertions: required before general production claim.

Metrics:

- ingest throughput
- p50, p95, and p99 query latency
- p50, p95, and p99 evidence-pack latency
- replay time
- compaction impact
- memory/RSS
- disk amplification
- backup time and restore time

Pass: benchmark artifact is version-tagged, reproducible, and within the SLO
envelope for the release tier.

Fail: no artifact, synthetic-only artifact for a real-data claim, or more than
7 percent p95 regression against the previous accepted baseline.

## Gate 7: Dirty And Adversarial Pilot

Requirement: at least one meaningful pilot runs malformed, conflicting,
adversarial, and high-volume data through the ingestion, query, evidence,
backup, restore, and operations paths.

Pass: pilot postmortem exists, all critical/high findings are closed, and
operator burden and cost profile are documented.

Fail: no pilot, unclosed critical/high issue, or user-visible correctness issue
without an accepted mitigation.

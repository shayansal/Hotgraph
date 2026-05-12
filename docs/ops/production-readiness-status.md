# Production Readiness Status

Hotgraph is still pre-production. This file tracks the P0 blockers as pass/fail
gates so the project does not drift from evidence-backed readiness.

## Implemented Gates

- Durable single-node WAL has record checksums, monotonic sequence numbers,
  transaction timestamps, and idempotency metadata.
- Segmented WAL supports segment manifests, segment-level checksums, atomic
  manifest publish, segment rotation, compaction archival, corrupt segment
  quarantine, and snapshot plus WAL-tail restore.
- Snapshots include schema version, WAL LSN boundary, event checksum, and
  deterministic graph state hash over materialized content.
- API auth is enabled by default outside explicit development mode.
- API idempotency records can be persisted beside durable event logs.
- API errors include machine-stable error codes.
- Kubernetes starter deployment is pinned to one API replica for file-backed
  single-node mode and includes PVCs, health checks, backup CronJob, network
  policies, and PodDisruptionBudgets.

## Blocking Evidence Still Required

- Crash/power-failure fault injection has targeted unit coverage, but not a
  process-level kill matrix for append, fsync, index update, compaction, and
  snapshot finalization.
- Disk-full and torn-write simulation are not wired to CI. They require a
  controlled filesystem or fault-injection layer.
- KMS-backed envelope encryption is documented, but not connected to a real KMS
  provider in this repository.
- Production redb/RocksDB/sled backend evaluation is not complete.
- Multi-replica API mode is intentionally blocked until a shared durable log or
  leader/follower single-writer design exists.
- 10M, 50M, and 100M benchmark artifacts have not been produced.
- Penetration test, monthly restore drill, dirty-data pilot, and adversarial
  real-data pilot require external operational evidence.

## Current Production Claim

The only honest production claim today is:

Hotgraph has a production-shaped single-node storage and API foundation, but it
is not production-grade until the blocking evidence above is complete and
reviewed.


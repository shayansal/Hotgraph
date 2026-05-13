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
- A first redb-backed durable graph store exists with LSN-addressed events,
  persisted idempotency keys, materialized entity/assertion/source rows,
  durable subject/predicate/object/source/time/context index entries, schema
  migration metadata, and writer lease fencing.
- The API can be started from `RG_REDB_PATH`; redb-backed idempotency survives
  API restart and prevents duplicate writes after process-local state is lost.
- Redb-backed graph queries can read materialized assertion rows through durable
  subject, predicate, object, source, valid-time, transaction-time, and context
  indexes instead of replaying an in-memory graph for every query.
- A `hotgraph` operational CLI can create and verify backup artifacts from a
  redb store, restore into a clean redb target directory, and verify restored
  state hash/query parity.
- API node-role configuration exists for writer and reader modes. Reader mode
  requires `HOTGRAPH_WRITER_URL`, can proxy writes to the writer over HTTP, can
  call the writer replication endpoint, and can apply serialized replication
  batches to its local redb follower store.
- Redb follower catch-up can replay committed leader LSNs into a follower store,
  exposes network-serializable replication batches, enforces in-order LSN
  application, and rejects divergent existing LSNs.
- Confidential mode uses `XChaCha20Poly1305` AEAD for record/envelope
  encryption, includes authenticated associated data, and has tests for
  tamper, wrong-key, wrong-purpose, source-store, event-log, snapshot, and key
  rotation behavior.
- Production envelope construction rejects the deterministic local development
  KMS provider.
- `AwsKmsProvider` is wired to the AWS Rust SDK behind the `aws-kms` feature,
  with a mockable `AwsKmsClient` boundary and tests for data-key generation,
  unwrap, health check, and production envelope use.
- Governance enforcement now filters source fetches, assertion fetches, graph
  queries, path results, entity state assertions, evidence packs, and AI context
  packs for source ACL and redaction checks in the main API crate.
- Prometheus metrics include durable last/applied LSN, follower lag, writer
  lease status, and API latency histograms. Kubernetes Prometheus rules and
  Grafana panels exist for the first production SLO signals.
- Release-gate data structures now exist for crash-recovery matrix artifacts
  and 10M/50M/100M benchmark artifacts so CI/release checks can reject fake or
  incomplete evidence.

## Blocking Evidence Still Required

- Crash/power-failure fault injection now has an explicit required matrix and
  machine-checkable report contract, but real process-kill/power-loss artifacts
  still must be produced by nightly/stress infrastructure.
- Disk-full and torn-write simulation are not wired to CI. They require a
  controlled filesystem or fault-injection layer.
- The AWS KMS SDK adapter compiles under the `aws-kms` feature, but hosted
  workload identity/IAM behavior still requires deployment evidence.
- The redb backend is implemented as v0.1, but it still needs process-level
  crash testing, full API query-path coverage, and scale evidence before it can
  be called production-ready.
- Multi-replica API mode has a node-role boundary, network write proxying,
  serialized follower batches, and catch-up endpoints. Stale-reader production
  routing, lease-expiry failover drills, and split-brain prevention evidence
  remain blocking evidence items.
- 10M, 50M, and 100M benchmark artifacts have not been produced.
- Penetration test, monthly restore drill, dirty-data pilot, and adversarial
  real-data pilot require external operational evidence.

## Current Production Claim

The only honest production claim today is:

Hotgraph has a production-shaped single-node storage and API foundation, but it
is not production-grade until the blocking evidence above is complete and
reviewed.

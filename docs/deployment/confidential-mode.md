# Confidential Mode Deployment

Reality Graph confidential mode is for environments where graph memory, source text,
and agent context cannot leave a controlled security boundary. The first
implementation lives in `crates/rg-confidential` and provides encrypted-at-rest
record envelopes, key rotation mechanics, redaction-aware evidence packs,
no-raw-source mode, and privacy-preserving analytics interfaces.

## Security Posture

The MVP crate is an integration boundary, not a complete compliance program.
Production deployments should replace local test key material with KMS, HSM, or
confidential-compute key release, and should run regular restore, redaction, and
access-control drills.

Core rules:

- Keep Rust core graph state inside the deployment boundary.
- Encrypt event logs, snapshots, source stores, and backups at rest.
- Keep source text outside hot graph indexes.
- Return source IDs and content hashes even when raw evidence is redacted.
- Do not send raw source snippets to AI agents in no-raw-source mode.
- Treat privacy analytics as aggregate output only; exact small-group counts are
  suppressed.
- Log key rotation, snapshot restore, and evidence redaction decisions.

## Air-Gapped Deployment

Air-gapped deployments should assume no external dependency at runtime.

Recommended setup:

- Build images and Rust artifacts in a controlled build environment.
- Export signed OCI images and package checksums to removable media.
- Mirror required package indexes internally before the build window.
- Run Qdrant or any vector sidecar inside the same network boundary.
- Disable outbound network access for API, worker, and ingestion services.
- Store key material in an offline HSM, sealed file, or local KMS appliance.
- Use no-raw-source mode for agent-facing evidence packs by default.
- Export only redacted reports, aggregate analytics, or approved source excerpts.

Operational checks:

- Verify that event log and snapshot files do not contain plaintext source text.
- Restore a graph from encrypted snapshots and append-only logs before launch.
- Confirm that analytics suppress small groups.
- Confirm that ingestion review queues cannot call external LLM providers.

## On-Prem Deployment

On-prem deployments should use tenant-level key separation and strict source
storage controls.

Recommended setup:

- Deploy the API and workers on dedicated nodes with encrypted local disks.
- Back event logs and snapshot stores with storage that supports immutability or
  append-only policies.
- Store raw source documents in a separate encrypted source store.
- Use tenant-specific keys for logs, snapshots, backups, and source blobs.
- Rotate tenant keys on a fixed schedule and after operator changes.
- Keep Prometheus and Grafana inside the private network.
- Scrub logs for source text, snippets, secrets, and prompt-injection payloads.

For AI workflows:

- Enable no-raw-source mode for default agent retrieval.
- Allow raw excerpts only through explicit source access policy.
- Return redaction reasons inside evidence metadata.
- Prefer local embedding providers for sensitive corpora.

## Private Cloud Deployment

Private cloud deployments should bind confidential mode to managed security
services without exposing source text to public endpoints.

Recommended setup:

- Use cloud KMS keys scoped per tenant or regulated data boundary.
- Use private networking for API, workers, vector sidecars, object storage, and
  metrics.
- Require service identities for key unwrap and snapshot restore.
- Store encrypted snapshots and event logs in object storage with versioning and
  retention lock.
- Use workload identity instead of static cloud credentials.
- Deny public ingress except through approved API gateways.
- Run backup restore tests in an isolated account or project.

Policy controls:

- Separate read permissions for graph assertions and raw source excerpts.
- Enforce no cross-tenant evidence pack leakage.
- Keep redacted source IDs visible so answers remain auditable.
- Keep differential privacy parameters documented per analytics product.

## Confidential Compute Roadmap

Confidential compute support should be added after single-node correctness and
encrypted restore are stable.

Roadmap:

1. Add a KMS-backed `KeyProvider` trait with envelope encryption.
2. Add attested key release for confidential VMs or enclaves.
3. Bind event-log and snapshot decryption to measured boot claims.
4. Add remote attestation evidence to health checks and deployment status.
5. Add sealed local cache keys for hot evidence-pack and source-slice caches.
6. Add encrypted vector sidecar payloads or inside-boundary vector search.
7. Add tenant-scoped backup escrow and break-glass recovery workflows.

Confidential-compute constraints:

- Never present simulation, retrieval, or analytics output as proof of hardware
  attestation.
- Do not release source-decryption keys until attestation policy passes.
- Keep attestation statements in audit logs.
- Rotate keys when attestation policy, image digest, or runtime configuration
  changes.

## Verification Checklist

Before using confidential mode with sensitive data:

- Run `cargo test -p rg-confidential --test confidential_mode`.
- Write and restore an encrypted event log.
- Write and restore an encrypted snapshot.
- Rotate keys and verify old records remain readable through retained keys.
- Verify no plaintext source snippets appear on disk.
- Verify no-raw-source evidence packs preserve IDs while removing snippets.
- Verify small-group analytics are suppressed.
- Verify deployment logs contain no source text or secrets.

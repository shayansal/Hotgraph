# WAL, Snapshot, And Recovery Semantics

This document defines the single-node durability contract for the current
storage engine.

## WAL Record Format

Each durable event record is line-oriented and encoded as escaped fields:

```text
RGEVENT | sequence | checksum | version | event_id | tx_time | idempotency_key | payload
```

Fields:

- `sequence`: monotonic 1-based WAL sequence number.
- `checksum`: checksum over sequence, version, event ID, transaction time,
  idempotency key, and payload.
- `version`: record codec version.
- `event_id`: deterministic event ID from the event-sourced command path.
- `tx_time`: monotonic transaction timestamp.
- `idempotency_key`: optional API idempotency key, empty when absent.
- `payload`: encoded graph event.

Legacy v1 records without sequence metadata remain readable for local upgrade
compatibility, but production writes must use the v2 format.

## Segmented WAL

`SegmentedWal` is the production-shaped WAL path. It stores records in files
named:

```text
segment-00000000000000000001.wal
segment-00000000000000000001.manifest
```

Each segment manifest includes:

- schema version
- segment ID
- first sequence
- last sequence
- event count
- checksum over records in the segment

Segment manifests are published atomically after append. Readers verify every
manifest before replay and reject reordered, missing, duplicated, or corrupted
segments. Compaction may archive complete segments whose sequences are covered
by a verified snapshot. Recovery can quarantine a corrupt segment into a
dedicated directory without silently replaying unverified data.

## Fsync Policy

`WalOptions` supports:

- `EveryWrite`: strongest durability, fsyncs every acknowledged write.
- `EveryNWrites(n)`: group commit, fsyncs every `n` records.
- `Never`: tests and throwaway local fixtures only.

Production default is `EveryWrite` until group commit has crash-test evidence
and an explicit RPO owner.

## Corruption Handling

Recovery reads records in sequence order. If it finds a partial, torn,
checksummed, unsupported, or out-of-order record, it truncates the WAL to the
last good byte boundary and reports:

- recovered record count
- last good sequence
- quarantined byte count
- corruption reason

Recovery must not modify any valid record before the corrupt tail.

## Snapshot Semantics

Snapshots are point-in-time event snapshots with a manifest containing:

- schema version
- event count
- last event ID
- event checksum
- WAL LSN boundary
- deterministic graph-state hash

`SnapshotWriter::write_atomic` writes a temporary file, fsyncs it, renames it
into place, and best-effort fsyncs the parent directory. Readers reject missing
or mismatched manifests.

## Restore Contract

Restoring from a snapshot or backup must satisfy:

- event checksum equality
- deterministic graph-state hash equality
- query parity over entities, assertions, and sources
- bitemporal query semantics unchanged after replay
- snapshot WAL LSN boundary plus segmented WAL tail reaches the same state as
  full replay

Any mismatch is a failed restore, not a warning.

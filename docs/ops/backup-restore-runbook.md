# Backup And Restore Runbook

## Backup Schedule

Initial single-node policy:

- hourly WAL-preserving backup for active deployments
- daily full backup retained for 30 days
- weekly full backup retained for 12 weeks
- monthly full backup retained for 12 months

The production scheduler must write backup metadata with commit SHA, schema
version, event count, last event ID, event checksum, graph-state hash, start
time, finish time, and storage location.

## Backup Integrity Check

For every backup artifact:

1. Read the manifest.
2. Decode every WAL record and verify checksum.
3. Replay into isolated storage.
4. Compare event checksum and deterministic graph-state hash.
5. Run post-restore query parity checks.
6. Emit a restore report with timings and pass/fail result.

## One-Command Restore

The restore command must take:

```text
backup artifact path
target data directory
expected commit/schema version
```

It must refuse to overwrite an existing target unless an explicit `--replace`
flag is provided and the target was backed up first.

## Monthly Drill

Every month, run a timed restore drill against the largest production-like
dataset available. Store evidence in `docs/ops/restore-drills/<date>.md`:

- backup artifact ID
- operator
- start and finish time
- RPO/RTO result
- checksum report
- query parity report
- incident notes

Critical or high findings block production promotion until closed.


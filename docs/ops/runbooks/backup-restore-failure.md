# Backup Or Restore Failure Runbook

Use when backup freshness, backup verification, or restore verification fails.

1. Freeze destructive maintenance jobs.
2. Preserve the failing artifact and logs.
3. Run `hotgraph backup verify --input <artifact>`.
4. Restore into a clean PVC or clean directory only.
5. Compare manifest schema version, event checksum, graph state hash, and query parity.
6. If state hash differs, mark the artifact unsafe and restore from the previous verified backup.
7. Store the report in `docs/ops/restore-drills/`.

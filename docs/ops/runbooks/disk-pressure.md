# Disk Pressure Runbook

Use when disk usage exceeds alert thresholds.

1. Stop compaction and nonessential ingestion.
2. Check PVC usage, WAL growth, snapshot age, backup volume, and redb file size.
3. Verify recent backups before deleting archived artifacts.
4. Expand the PVC or move archived WAL segments to object storage.
5. Confirm no active WAL segment or snapshot manifest was removed.
6. Re-run health and restore verification after remediation.

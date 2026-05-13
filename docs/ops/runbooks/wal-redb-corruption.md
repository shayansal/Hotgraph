# WAL Or redb Corruption Runbook

Use when WAL, backup, snapshot, or redb corruption is detected.

1. Stop the writer and preserve all WAL segments, manifests, snapshots, and redb files.
2. Work only on a copy.
3. Quarantine corrupt segments and identify the last good LSN.
4. Restore from the newest verified snapshot plus WAL tail.
5. Compare deterministic state hash and historical query parity.
6. Rebuild followers from the verified restored state.
7. File a root-cause report before accepting new writes.

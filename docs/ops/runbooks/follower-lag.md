# Follower Lag Runbook

Use when `HotgraphFollowerLag` fires.

1. Check `rg_replication_lag_lsn` and reader pod logs.
2. Confirm the reader has `HOTGRAPH_WRITER_URL` and `HOTGRAPH_READER_MAX_LAG_LSN`.
3. Trigger `/v1/admin/replication/catch-up` with an admin key.
4. If divergence is reported, stop the reader and rebuild it from a clean snapshot plus WAL tail.
5. Keep writes on the fenced writer; do not promote a stale reader.
6. Attach the catch-up report and state hash parity to the incident record.

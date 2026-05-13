# API Readiness Runbook

Use when `HotgraphApiDown` fires.

1. Confirm the exact image digest and commit from the deployment.
2. Check `/v1/health` and `/v1/metrics` from inside the namespace.
3. Inspect API pod logs for auth, storage, redb, WAL, and migration errors.
4. Verify the redb PVC is mounted read/write on the writer and read-only or local on readers.
5. If the writer is unhealthy, freeze writes and follow `writer-failover.md`.
6. Record the incident timeline in the release evidence folder.

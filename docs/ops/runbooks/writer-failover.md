# Writer Failover Runbook

Use when the writer lease is missing, expired, or the writer is unhealthy.

1. Confirm no two writers are acknowledging writes.
2. Stop the unhealthy writer before promoting another node.
3. Verify the latest backup, snapshot manifest, WAL tail, and deterministic state hash.
4. Acquire a new fenced writer lease with a higher fencing token.
5. Start one writer replica only.
6. Run follower catch-up and query parity checks.
7. Record fencing token, commit SHA, image digest, and state hash in the release evidence folder.

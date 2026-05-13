# Auth Failure Spike Runbook

Use when auth failures or rate limits spike.

1. Check if the spike is isolated to one tenant, principal, IP range, or route.
2. Verify recent API key rotation and service-account changes.
3. Rotate suspected keys and invalidate stale key versions.
4. Confirm no secrets appeared in logs, traces, metrics, or error responses.
5. Export audit events for security review.
6. Keep write-capable keys disabled until abuse is ruled out.

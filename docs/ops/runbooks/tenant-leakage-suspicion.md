# Tenant Leakage Suspicion Runbook

Use when cross-tenant denials spike or a leak is suspected.

1. Freeze affected tenant reads and AI context-pack generation.
2. Export audit events for the suspected tenant, principal, source, and time window.
3. Run adversarial cross-tenant query tests against a restored copy.
4. Verify source ACLs, memory ACLs, redaction status, and summary invalidation.
5. Preserve evidence packs and logs for security review.
6. Do not reopen access until no critical/high findings remain.

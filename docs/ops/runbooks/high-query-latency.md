# High Query Latency Runbook

Use when `HotgraphHighQueryLatency` fires.

1. Identify slow routes from `rg_api_request_duration_seconds`.
2. Check result limits, path depth, candidate counts, and tenant distribution.
3. Inspect reader lag and disk pressure before blaming query logic.
4. For path queries, reduce max depth or route broad graph expansion to a background job.
5. Capture the query shape, tenant ID, request ID, p95/p99 windows, and relevant logs.
6. File a regression if the latency exceeds the accepted benchmark envelope.

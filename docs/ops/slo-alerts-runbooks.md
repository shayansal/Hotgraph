# SLOs, Alerts, And Incident Runbooks

## Initial SLOs

- API availability: 99.9 percent monthly for production pilots.
- Write durability lag: p99 less than 5 seconds under the selected fsync policy.
- Write latency: p95 less than 250 ms for acknowledged single writes at the
  validated scale envelope.
- Point-in-time query latency: p95 less than the release-specific benchmark
  envelope.
- Evidence-pack latency: p95 less than the release-specific benchmark envelope.
- Recovery time: latest drill meets RTO less than or equal to 15 minutes.
- Backup freshness: latest successful backup less than or equal to 60 minutes
  old.

## Required Metrics

- request count by route, status, tenant, and role
- request duration histograms by operation
- write append latency
- WAL fsync latency and failures
- WAL recovery status and quarantined bytes
- snapshot age and restore validation status
- backup age, duration, size, checksum status, and restore status
- replay duration
- query result count and timeout count
- memory/RSS and disk usage

## Required Alerts

- API down or readiness failing for more than 2 minutes
- p95 write latency above SLO for 10 minutes
- p95 query latency above SLO for 10 minutes
- WAL append or fsync failure
- WAL recovery quarantines any bytes
- backup older than RPO
- restore verification failure
- disk usage above 80 percent, critical above 90 percent
- cross-tenant access denial spike
- auth failure spike

## Top 10 Runbooks

1. API readiness failing: check pod logs, health endpoint, storage path, and
   latest deployment digest.
2. WAL append failure: stop writes, preserve WAL, inspect disk space and file
   permissions, run recovery in a copy.
3. WAL corruption: copy the segment, run truncate-to-last-good, compare replay
   hash, file incident.
4. Backup stale: inspect scheduler, storage credentials, disk pressure, and
   last restore report.
5. Restore failure: preserve artifact, compare manifest, event checksum, and
   replay error, then escalate.
6. Query latency spike: inspect hot tenants, result limits, path depth, and
   slow query logs.
7. Evidence-pack latency spike: inspect source retrieval, contradiction checks,
   and compression stage timing.
8. Disk pressure: stop compaction, expand volume or rotate old artifacts,
   confirm no WAL truncation.
9. Auth failure spike: inspect service-account changes, key rotation timing,
   and possible credential stuffing.
10. Cross-tenant leakage suspicion: freeze affected tenant queries, export audit
    log, run isolation tests, and escalate to security owner.


# Hotgraph Production Candidate Release Record

## Candidate

- Version:
- Commit SHA:
- Container image digest:
- Schema version:
- Migration version:
- Release manager:
- Decision date:

## Evidence Artifacts

- Crash matrix report:
- Backup artifact:
- Restore drill report:
- State hash parity report:
- Query parity report:
- Security review:
- Penetration test:
- Benchmark report:
- On-call drill report:
- Dirty/adversarial pilot postmortem:

## Scale Envelope

- Dataset seed:
- Assertion count:
- Event count:
- Hardware profile:
- p50 write latency:
- p95 write latency:
- p99 write latency:
- p50 query latency:
- p95 query latency:
- p99 query latency:
- p95 evidence-pack latency:
- Replay time:
- Restore time:
- RSS:
- Disk amplification:
- Compaction pause:

## Gate Decisions

| Gate | Decision | Evidence | Risk Owner | Expiry |
| --- | --- | --- | --- | --- |
| Durability | fail |  |  |  |
| Recoverability | fail |  |  |  |
| Bitemporal correctness | fail |  |  |  |
| Isolation and security | fail |  |  |  |
| Operability | fail |  |  |  |
| Scale envelope | fail |  |  |  |
| Dirty/adversarial pilot | fail |  |  |  |

## Known Limitations

- Production-grade status must remain blocked while any gate is `fail`.
- Waivers require a named owner, mitigation, and expiry date.

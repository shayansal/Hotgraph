# Load Test And Scale Envelope Program

Hotgraph scale claims require reproducible artifacts, not README targets.

## Dataset Families

Each scale run must include:

- clean ingestion: well-formed source-backed assertions
- noisy ingestion: duplicates, near-duplicates, missing optional metadata, and
  backfilled valid-time intervals
- adversarial ingestion: timestamp abuse, replay attempts, duplicate floods,
  malformed source records, and conflicting provenance
- temporal skew: hot recent writes mixed with deep historical backfills

## Required Scale Points

- 10M assertions: private beta gate
- 50M assertions: paid pilot gate
- 100M assertions: production claim gate

## Metrics

Record:

- ingest throughput
- p50, p95, and p99 write latency
- p50, p95, and p99 point-in-time query latency
- p50, p95, and p99 path query latency
- p50, p95, and p99 evidence-pack latency
- replay time
- compaction pause and throughput impact
- memory/RSS
- disk amplification
- backup time
- restore time
- cost per million assertions and per thousand queries

## Regression Policy

Any p95 latency regression greater than 7 percent against the latest accepted
baseline blocks merge unless the release owner records an explicit waiver with
expiry and remediation issue.

## Artifact Format

Store each run under `benchmarks/reports/<date>-<scale>/` with:

- `config.json`
- `dataset-manifest.json`
- `results.jsonl`
- `summary.md`
- `environment.txt`
- `flamegraph` or profiler output when regressions occur


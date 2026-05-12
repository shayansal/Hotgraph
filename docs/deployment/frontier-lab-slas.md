# Frontier-Lab SLAs and Deployment Mode

Reality Graph lab deployment mode is for research groups that need to freeze a
system version, reproduce a paper run months later, and audit every
memory/evidence decision that influenced an answer or agent action.

## Lab Freeze Contract

A lab freeze must include:

- Exact version pins for Reality Graph, Rust, schemas, fixture datasets, model
  providers or provider-independent deterministic adapters, container images,
  and evaluation harnesses.
- A reproducibility manifest with seeds, graph snapshots, event logs, eval
  reports, and audit decision records.
- Offline artifact bundles for air-gapped or provider-independent replay.
- Versioned graph schemas and migration simulation reports.
- Rollback plans that restore schema versions, snapshots, event cursors, and
  audit records.
- Cluster health reports showing deterministic replay status, restore-test
  freshness, version skew, tenant scope, and offline artifact availability.

## SLA Targets

The initial SLA vocabulary is qualitative because benchmark numbers must come
from the harness:

- Offline mode: all critical replay, evaluation, and audit workflows can run
  without network access.
- Deterministic replay: event logs and snapshots reproduce the same graph state
  under the pinned schema and code version.
- Provider independence: paper baselines can use deterministic fake providers or
  explicitly pinned model-provider adapters.
- Auditability: every memory write, evidence pack, answer, and simulation output
  can be tied to assertion IDs, source IDs, event IDs, transaction time, and
  retrieval trace.
- Benchmark reproducibility: eval results include seeds, fixture digests,
  retrieval plans, context packs, and latency/cost settings.
- Rollback: every schema migration has a rollback assessment before it reaches a
  long-term support branch.

## Long-Term Support Branches

Long-term support branches should accept only:

- Backward-compatible schema migrations.
- Security patches.
- Determinism fixes.
- Reproducibility manifest improvements.
- Export and audit format additions that do not change prior semantics.

Breaking migrations should be rejected for LTS branches unless they include a
compatibility shim, migration simulator report, rollback plan, and explicit lab
approval.

## Operational Checklist

Before publishing a lab result:

- Generate an `rg-lab-deploy` reproducibility manifest.
- Validate the offline artifact bundle.
- Run migration simulation for every schema change since the previous freeze.
- Run rollback validation against the previous frozen version.
- Export graph snapshots, event logs, eval reports, and redacted datasets.
- Confirm cluster health reports no version skew and deterministic replay OK.
- Store the bundle digest with the paper, benchmark report, or internal lab
  record.

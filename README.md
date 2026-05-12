<p align="center">
  <img src="assets/hotgraph-logo.png" alt="Hotgraph logo" width="760">
</p>

# Hotgraph

**Reality Graph: 4D Knowledge Graph - Rust**

Models predict tokens. Reality Graph maintains reality.

Hotgraph is the repository for Reality Graph, a Rust-first, AI-native reality substrate. It is not trying to be a Neo4j clone, a vector database, or a thin GraphRAG wrapper. The core idea is a bitemporal, evidence-backed, belief-aware graph that can tell an AI agent what is known, how it is known, when it was true, what contradicts it, and what depends on it.

## Project Stage

This repository is in **pre-alpha research and kernel prototyping**.

What exists now:

- A Rust workspace with focused crates for the core graph, event sourcing, storage, temporal indexing, querying, AI context packs, ingestion, API, evaluation, governance, simulation, and deployment scaffolding.
- A new `rg-kernel` Reality Kernel with `RealityAtom`, bitemporal visibility, belief states, evidence spans, source references, conflict sets, dependency graphs, truth-maintenance primitives, causal primitives, a minimal Reality Query VM, model-native context compilation, and self-revising graph suggestions.
- Deterministic tests and fixtures across the workspace so behavior is auditable and reproducible.
- Early Python SDK, Next.js admin consoles, OpenAPI/protobuf schemas, Docker Compose, Kubernetes manifests, and research/design docs.
- A reusable Codex skill at `reality-graph-architect/` for project-specific checks and architecture guidance.

What this is not yet:

- A production database.
- A stable public API.
- A distributed graph engine.
- A fully optimized storage engine.
- A general-purpose vector database.
- A system that lets model output silently become truth.

The current goal is to prove the Reality Kernel and single-node correctness before scaling or hardening the system for production workloads.

## Core Thesis

Reality Graph stores assertions about reality, not naive facts.

```text
Entity:      A thing that may exist.
Assertion:  A source-backed claim about reality.
Event:      Something that happened in the system or world.
Source:     Evidence supporting an assertion.
Edge:       A relationship derived from assertions.
State:      The resolved graph at a valid-time and transaction-time point.
Atom:       The Reality Kernel primitive that unifies claims, memories, events,
            summaries, simulations, and derived beliefs.
```

Every meaningful claim must carry:

- valid time: when it is true in the modeled world
- transaction time: when the system learned or revised it
- provenance: sources and evidence spans
- confidence
- belief state
- context and permissions
- contradiction and dependency links when applicable

## Non-Negotiable Invariants

- The Rust core is the source of truth.
- Every assertion has provenance.
- Every assertion supports valid time and transaction time.
- No edge exists without confidence, source, and temporal metadata.
- Writes append events before updating indexes.
- The graph is queryable at historical valid-time and transaction-time points.
- Embeddings are retrieval indexes, not source-of-truth facts.
- AI-facing answers must return evidence paths and source IDs.
- Belief revision never deletes history.
- Contradictions are preserved, not silently collapsed.
- Simulation output is never labeled as fact.
- Unsafe Rust is forbidden unless an ADR justifies it and benchmarks prove necessity.

See [AGENTS.md](AGENTS.md) for the working rules Codex and contributors should follow.

## Repository Layout

```text
reality-graph/
  AGENTS.md
  README.md
  Cargo.toml
  crates/                         Rust workspace
  python/reality_graph/            Thin Python HTTP SDK
  frontend/console/                Minimal admin console
  frontend/lab-console/            Lab/eval command console
  schemas/openapi/                 REST schemas
  schemas/protobuf/                Protobuf schemas
  specs/rmp/                       Reality Memory Protocol draft
  infra/docker/                    Dockerfile and Compose stack
  infra/k8s/                       Kubernetes manifests
  infra/terraform/                 Terraform notes placeholder
  docs/architecture/               Architecture docs and roadmaps
  docs/adr/                        Architecture decision records
  docs/core/                       Reality Kernel semantics
  docs/product/                    Product and positioning docs
  docs/research/                   Paper stack drafts
  evals/                           Evaluation fixtures and scenarios
  tests/                           Fixtures, integration, and golden outputs
  reality-graph-architect/         Reusable Codex skill
```

## Architecture At A Glance

```text
Sources and documents
  -> ingestion candidates
  -> reviewed graph commands
  -> append-only events
  -> Reality Kernel atoms/assertions
  -> temporal indexes and materialized views
  -> query VM / retrieval compiler / context compiler
  -> evidence packs, API responses, agent memory, eval traces
```

The system separates truth from retrieval:

- The graph decides what evidence exists.
- Indexes make evidence discoverable.
- Vector search proposes candidates.
- LLMs summarize evidence.
- Belief, contradiction, permission, and temporal semantics stay in the Rust core.

## Rust Workspace Components

### Kernel And Core

| Crate | Purpose |
| --- | --- |
| `rg-core` | Assertion-first domain primitives: IDs, time intervals, confidence, entities, assertions, sources, ontology validation. |
| `rg-kernel` | Core Graph 2.0 Reality Kernel: atoms, bitemporal visibility, belief state, provenance, conflicts, dependencies, truth maintenance, causal primitives, native query VM, model-native context compilation, and self-revision suggestions. |
| `rg-events` | Event-sourced write path: graph commands, deterministic events, monotonic transaction timestamps, replayable graph state. |
| `rg-storage` | Single-node storage primitives: in-memory storage, file event log, snapshots, crash recovery. |
| `rg-index` | Temporal and adjacency indexes, contradiction checks, point-in-time query helpers. |
| `rg-query` | Internal graph query and path query execution over storage/index layers. |
| `rgql` | Reality Graph Query Language parser, AST, planner, executor, explanations, and fuzz tests. |

### AI-Native Context, Memory, And Retrieval

| Crate | Purpose |
| --- | --- |
| `rg-ai` | EvidencePack generation, vector index trait, deterministic AI test providers, graph-to-evidence linkage. |
| `rg-retrieval-compiler` | Adaptive retrieval compiler that routes between keyword, vector, graph, temporal, causal, contradiction, and compression operators. |
| `rg-memory-activation` | HippoRAG-style spreading activation over entity, assertion, source, and memory graphs. |
| `rg-agent-memory` | Typed agent memory lifecycle: episodic, semantic, procedural, preference, goal, plan, reflection, correction, and relationship memories. |
| `rg-cognitive-cache` | Permission-aware hot caches for low-latency agent recall, entity state, path queries, and evidence packs. |
| `rg-context-compression` | Token-budget-aware compression that preserves citations, uncertainty, temporal metadata, and contradictions. |
| `rg-context-serving` | Streaming and low-copy context serving primitives, protobuf schema, batch context assembly, and tracing stages. |
| `rg-runtime` | Experimental model-runtime hooks for prefill context, verify-before-answer, and write-memory-after-action patterns. |

### Belief, Time, Truth, And Causality

| Crate | Purpose |
| --- | --- |
| `rg-belief` | Contradiction-aware belief state, belief revisions, conflict sets, source trust policy hooks. |
| `rg-truth-maintenance` | Assumptions, derived assertions, dependency graph, retraction propagation, and invalidation traces. |
| `rg-temporal-reasoning` | Allen interval algebra and temporal query operators. |
| `rg-causal` | Causal events, causal links, mechanisms, interventions, dependency cones, and counterfactual impact traces. |
| `rg-sim` | Simulation helpers and synthetic graph events. |
| `rg-agent-sim` | Agent simulation lab primitives for proposed actions, risks, missing information, and policy violations. |

### Ingestion, Ontology, Maintenance, And Trust

| Crate | Purpose |
| --- | --- |
| `rg-ingest` | Candidate assertion extraction interfaces and review/commit planning. |
| `rg-ingest-multimodal` | Deterministic source adapters for text, PDFs, CSV, JSON, HTML, image metadata, transcripts, repositories, and database snapshots. |
| `rg-maintenance` | Self-healing maintenance jobs for duplicate entities, stale assertions, contradictions, summaries, source trust, compaction, and index rebuilds. |
| `rg-ontology-learning` | Review-gated ontology drift detection, predicate mining, constraint learning, and human review workflow. |
| `rg-source-trust` | Source identity, authority, reputation, corroboration, independence, and trust update models. |
| `rg-active-knowledge` | Missing information, staleness, uncertainty, clarifying questions, and tool recommendation primitives. |

### APIs, Governance, Security, And Integrations

| Crate | Purpose |
| --- | --- |
| `rg-api` | Axum HTTP API boundary with health, metrics, graph, query, evidence, ingestion, and AI endpoints. |
| `rg-reality-api` | High-level AI-native product API: remember, recall, verify, explain, timeline, simulate, context, contradictions, state. |
| `rg-mcp-server` | MCP resources and tools for agent access to graph context. |
| `rg-integrations` | Adapter layer for MCP, OpenAI-style tools, Anthropic-style tools, LangGraph, LlamaIndex, DSPy, and local agent daemon patterns. |
| `rg-agent-security` | Capability tokens, tool permission policies, taint tracking, prompt-injection risk, sandboxed MCP invocation, audit logs, and exfiltration detection. |
| `rg-governance` | Tenant isolation, permissions, retention, audit, redaction, legal hold, source signing, and evidence access control. |
| `rg-confidential` | Encrypted event logs, encrypted snapshots, redacted query mode, no-raw-source mode, key rotation, and privacy-preserving analytics. |
| `rg-federation` | Federated graph nodes, trust boundaries, remote plans, cross-graph entity resolution, and permissioned joins. |
| `rg-lab-deploy` | Frontier-lab deployment reproducibility: deterministic profiles, schema versions, migration simulation, rollback tests, offline bundles. |

### Evaluation, Training, And Research Infrastructure

| Crate | Purpose |
| --- | --- |
| `rg-eval` | Retrieval benchmark harness comparing vector-only, keyword-only, graph-only, temporal, hybrid, and adaptive retrieval. |
| `rg-frontier-eval` | Frontier-lab benchmark families: TemporalQA, AgentMemoryQA, MultiHopEvidenceQA, CausalTraceQA, CounterfactualPlanningQA, and more. |
| `rg-memory-turing-test` | Salehi Memory Turing Test benchmark for persistent, evolving agent memory. |
| `rg-adversarial-memory-eval` | Adversarial memory scenarios for poisoning, prompt injection, temporal spoofing, fake authority, and leakage attempts. |
| `rg-agent-judge` | Agent trace evaluation oracle for correctness, evidence faithfulness, temporal correctness, hallucination, and unsafe memory use. |
| `rg-learning` | Feedback events, retrieval outcomes, ranking features, offline evaluation, and bandit-router placeholders. |
| `rg-feedback-loop` | Outcome observations, agent success signals, evidence usefulness, memory quality, policy candidates, and training export jobs. |
| `rg-training-data` | Exporters for graph-aware training examples, temporal reasoning examples, evidence-pack SFT, belief-revision DPO pairs, and tool-trace preferences. |
| `rg-distillation` | Training-data generation and baseline small models for routing, temporal classification, contradiction classification, source trust, and ranking. |
| `rg-worldgen` | Synthetic world generation with hidden truth, noisy evidence, documents, contradictions, causal chains, and benchmark tasks. |
| `reality-gym` | Agent training environment loop: observe, retrieve, reason, act, write memory, update world, evaluate outcome. |
| `rg-bench` | Criterion benchmark helpers and synthetic graph generators for throughput, replay, temporal queries, traversal, and evidence packs. |
| `rg-accelerated` | CPU-first optimized graph kernels and feature-gated acceleration research tracks. |

### Multi-Agent And Shared Reality

| Crate | Purpose |
| --- | --- |
| `rg-multi-agent` | Private memory, shared memory spaces, belief namespaces, memory sharing policy, inter-agent evidence exchange, and conflict resolution. |
| `rg-graphrag` | Temporal community summaries and source-backed GraphRAG-style hierarchy with valid-time and transaction-time semantics. |

## Non-Rust Components

| Path | Purpose |
| --- | --- |
| `python/reality_graph/` | Thin Python SDK for the REST API. It deliberately avoids duplicating engine logic. |
| `frontend/console/` | Minimal admin console for entity browsing, assertions, source viewing, and query workbench flows. |
| `frontend/lab-console/` | Lab command console for eval leaderboard, evidence traces, contradiction maps, source trust, latency/cost, and security incidents. |
| `schemas/openapi/` | OpenAPI descriptions for the REST surface. |
| `schemas/protobuf/` | Protobuf schemas for graph and evidence-pack serving. |
| `specs/rmp/` | Draft Reality Memory Protocol with JSON schema, protobuf, HTTP mapping, MCP mapping, OpenAPI, security model, versioning, and reference client notes. |
| `infra/docker/` | Dockerfile, Docker Compose stack, Prometheus, Grafana provisioning, and local deployment instructions. |
| `infra/k8s/` | Kubernetes manifests for API, worker, Qdrant, Prometheus, Grafana, ingress, and config. |
| `docs/research/` | Draft paper stack for bitemporal knowledge substrates, memory tests, temporal GraphRAG, belief revision, Reality Gym, cognitive cache, and context compilation. |
| `evals/` | Fixture datasets and scenario files for retrieval, memory, and adversarial evaluation. |

## Development Commands

Install Rust using `rustup`, then run:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo test --all --release
```

The project skill bundles the same checks:

```bash
bash reality-graph-architect/scripts/run_all_checks.sh
```

Run focused kernel tests:

```bash
cargo test -p rg-kernel --test reality_kernel
```

Run the API locally:

```bash
cargo run -p rg-api
```

Health and metrics:

```text
GET http://127.0.0.1:8080/v1/health
GET http://127.0.0.1:8080/v1/metrics
```

## Local Deployment

Start the local stack:

```bash
docker compose -f infra/docker/docker-compose.yml up --build
```

Services:

- Reality Graph API: `http://localhost:8080`
- Qdrant: `http://localhost:6333`
- Prometheus: `http://localhost:9090`
- Grafana: `http://localhost:3000`

Postgres is optional:

```bash
docker compose -f infra/docker/docker-compose.yml --profile postgres up --build
```

Kubernetes manifests live under `infra/k8s/`:

```bash
kubectl apply -k infra/k8s/
```

See [infra/docker/README.md](infra/docker/README.md), [infra/k8s/README.md](infra/k8s/README.md), [docs/deployment/confidential-mode.md](docs/deployment/confidential-mode.md), and [docs/deployment/frontier-lab-slas.md](docs/deployment/frontier-lab-slas.md).

## Documentation Map

Start here:

- [Product PRD](docs/product/prd.md)
- [System overview](docs/architecture/system-overview.md)
- [Reality Atom](docs/core/reality-atom.md)
- [Bitemporal semantics](docs/core/bitemporal-semantics.md)
- [Belief semantics](docs/core/belief-semantics.md)
- [Truth maintenance](docs/core/truth-maintenance.md)
- [Query VM](docs/core/query-vm.md)
- [Model-native context compilation](docs/core/model-context-compilation.md)
- [Self-revising graph mechanics](docs/core/self-revising-graph.md)
- [Trillion-edge roadmap](docs/architecture/trillion-edge-roadmap.md)
- [Reality Memory Protocol](specs/rmp/README.md)

Architecture decisions:

- [ADR 0001: Rust core](docs/adr/0001-rust-core.md)
- [ADR 0002: Event sourcing](docs/adr/0002-event-sourcing.md)
- [ADR 0003: Bitemporal model](docs/adr/0003-bitemporal-model.md)

## Working Principles

- Prefer correctness before distributed scale.
- Keep the Rust core authoritative.
- Keep model output separate from durable truth.
- Make every revision replayable.
- Make every contradiction visible.
- Make every AI-facing response evidence-backed.
- Treat benchmarks and evals as product requirements, not afterthoughts.

## License

This workspace is configured as `MIT OR Apache-2.0` in `Cargo.toml`.

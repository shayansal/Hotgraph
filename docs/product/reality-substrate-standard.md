# Reality Substrate Standard

## Positioning

Models predict tokens.
Reality Graph maintains reality.

Reality Graph is not positioned as the best knowledge graph, a faster vector
database, or a prettier GraphRAG stack. The Phase 70 target is to become the
standard memory and reality substrate for AI agents: a source-backed,
bitemporal, permission-aware system that lets agents remember, verify, revise,
simulate, and explain what they know.

The product promise is simple:

- AI agents can act with persistent memory.
- Every non-trivial answer can point back to evidence.
- Beliefs can change without deleting history.
- Contradictions remain visible until resolved.
- Model outputs can improve the graph, but they do not become truth by default.
- Retrieval and memory policies improve only when eval gates prove improvement.

## Standard Package

Reality Graph becomes a credible standard only when the project ships the full
substrate around the engine, not only the graph core.

| Requirement | Repository Artifact | Standard Role |
| --- | --- | --- |
| Open-source community edition | Rust workspace, Python SDK, frontend consoles, Docker Compose | Gives researchers and builders a runnable baseline. |
| Frontier-lab benchmark suite | `crates/rg-frontier-eval`, `crates/rg-eval`, `evals/` | Proves quality, latency, cost, temporal correctness, and evidence recall. |
| Published research papers | `docs/research/` | Gives labs a technical frame for reproducibility and peer review. |
| MCP-compatible adapter | `crates/rg-mcp-server`, `crates/rg-integrations` | Makes the graph callable by agent clients and tool ecosystems. |
| Reality Memory Protocol spec | `specs/rmp/` | Defines a model-native memory contract beyond tool calls. |
| Agent training environments | `crates/reality-gym`, `crates/rg-worldgen` | Lets labs train and evaluate agents in noisy dynamic worlds. |
| Security-hardened memory layer | `crates/rg-agent-security`, `crates/rg-governance`, `crates/rg-confidential` | Prevents memory poisoning, exfiltration, and cross-tenant leakage. |
| Enterprise deployment mode | `infra/`, `docs/deployment/`, `crates/rg-lab-deploy` | Supports reproducible, audited, on-prem, private-cloud, and frozen-paper deployments. |
| Trillion-edge architecture roadmap | `docs/architecture/trillion-edge-roadmap.md` | Shows the path from single-node correctness to lab-scale reality graphs. |
| Evidence-backed eval oracle | `crates/rg-agent-judge`, `crates/rg-truth-maintenance` | Grades whether agents retrieved, cited, remembered, and reasoned correctly. |

## Standard Contract

A Reality Substrate implementation must satisfy these invariants:

- Assertions are about reality; they are not naive facts.
- Every assertion carries provenance, confidence, valid time, and transaction time.
- Writes append events before indexes are updated.
- Embeddings, summaries, and model outputs are auxiliary artifacts.
- AI-facing responses include source IDs, evidence paths, confidence, temporal metadata, and contradiction warnings when relevant.
- Agent memory is structured as episodic, semantic, procedural, preference, goal, plan, reflection, correction, relationship, or world-state memory.
- Memory revision preserves history through supersession, contradiction, and belief revision.
- Permissions are enforced before evidence, summaries, memories, or context packs are returned.
- Dangerous writes and memory promotion are policy-gated.
- Model or retrieval policy updates require evaluation improvement before deployment.

## Adoption Surfaces

Reality Graph should meet AI labs where they already work:

- MCP tools for agent clients.
- HTTP and OpenAPI endpoints for platform teams.
- Protobuf and streaming context serving for model runtimes.
- Python, Rust, and TypeScript client layers.
- RMP for model-native memory semantics.
- Docker Compose for local proof-of-concept work.
- Kubernetes and lab deployment profiles for reproducible experiments.
- JSONL, Parquet, Arrow, and OpenAI-style training exports.
- Evaluation reports that compare against vector-only, keyword-only, hybrid, GraphRAG-style, temporal graph, transcript memory, and Reality Graph full-stack baselines.

## Feedback Loop

The compounding engine is:

1. A model uses the graph.
2. The model produces an answer or action.
3. User, tool, or environment outcome is observed.
4. Reality Graph records success, failure, evidence usefulness, and memory-write quality.
5. The eval oracle scores correctness, evidence faithfulness, temporal correctness, hallucination, missing context, unsafe memory use, and contradiction handling.
6. Training examples are generated for retrieval, ranking, memory policy, ontology learning, and model fine-tuning.
7. Candidate retrieval, ranking, ontology, or memory-policy updates are proposed.
8. No automatic model-policy update is deployed unless an eval gate shows improvement.

This is how Reality Graph improves models while models improve Reality Graph.

## Community Edition

The community edition should stay useful without enterprise-only assumptions:

- Single-node Rust core.
- Local file/in-memory storage modes.
- API server and Python SDK.
- Admin console and lab console.
- Deterministic fixture graphs.
- Benchmark and eval harnesses.
- MCP adapter.
- RMP reference schemas.
- Docker Compose stack.

Enterprise capabilities can extend that base with tenant isolation, private cloud
deployment, confidential mode, audit retention, source signing, and governance
policies. The open core must still prove the main idea: evidence-backed temporal
memory for agents.

## Proof Standard

Reality Graph should not claim superiority by architecture diagram. It should
win by reproducible evidence:

- Better evidence recall on multi-hop and temporal questions than vector-only retrieval.
- Better temporal correctness than flat RAG and transcript memory.
- Explicit contradiction handling instead of silent claim collapse.
- Lower stale-memory error rate on long-horizon agent memory tests.
- Faithful citations after context compression.
- Permission-correct evidence packs.
- Reproducible benchmark reports with seed configs.
- Deterministic replay of graph state and memory decisions.

No benchmark dominance, no lab adoption.

## Product Line

At Phase 70, Reality Graph is presented as a family:

- Reality Graph Core: source-backed bitemporal graph engine.
- Reality API: high-level remember, recall, verify, explain, simulate, context, and state operations.
- Reality Memory Protocol: model-native memory semantics.
- Reality Gym: agent training and evaluation environments.
- Reality Eval Oracle: evidence-backed grading of agent traces.
- Reality Lab Console: executive visibility into quality, memory health, trust, security, latency, cost, and growth.
- Reality Deployment: reproducible lab, enterprise, and confidential deployment mode.

## North Star

The standard is reached when a frontier lab can connect Reality Graph to an
existing agent experiment in under 30 minutes, run a reproducible evaluation,
inspect every memory and evidence decision, export training examples, and freeze
the exact version for a paper or deployment audit.

That is the product:

Models predict tokens.
Reality Graph maintains reality.

# Reality Memory Protocol

Reality Memory Protocol, or RMP, is a model-native protocol for persistent,
source-backed, temporally-correct AI memory. MCP exposes tools and resources.
RMP defines the deeper memory contract those tools can carry.

RMP is designed for agents and model runtimes that need more than retrieval:
they need to remember, recall, verify, revise, forget, explain, simulate,
ground, compress, share, and audit memory with provenance and permissions.

## Goals

- Give model runtimes a stable memory protocol independent of any single model
  provider.
- Make every non-trivial memory response evidence-backed.
- Preserve valid time and transaction time in memory, claims, beliefs, and
  context packs.
- Keep contradictions visible instead of flattening them into one answer.
- Support secure sharing, redaction, audit, and tenant boundaries.
- Provide HTTP, MCP, OpenAPI, protobuf, and JSON Schema projections.

## Core Operations

| Operation | Purpose |
| --- | --- |
| `REMEMBER` | Store source-backed memory or candidate memory. |
| `RECALL` | Retrieve task-relevant memory, claims, timelines, or context packs. |
| `VERIFY` | Check a claim against source-backed graph state. |
| `REVISE` | Supersede or correct prior memory without deleting history. |
| `FORGET` | Apply retention, deletion, redaction, or archive policy. |
| `EXPLAIN` | Explain why a memory, claim, belief, or context pack was returned. |
| `SIMULATE` | Run a counterfactual or agent action simulation. |
| `GROUND` | Attach evidence and temporal constraints to model context. |
| `COMPRESS` | Reduce context under a token budget while preserving citations. |
| `SHARE` | Move memory across private, team, organization, or federated spaces. |
| `AUDIT` | Return access, write, retrieval, and belief-revision audit trails. |

## Core Objects

RMP standardizes these object families:

- `Memory`
- `Claim`
- `Evidence`
- `Belief`
- `Timeline`
- `ContextPack`
- `Contradiction`
- `CausalTrace`
- `SimulationTrace`
- `Permission`

Each object includes IDs, tenant or namespace scope, source IDs when applicable,
temporal metadata, confidence, and policy metadata.

## Files

- [rmp.schema.json](rmp.schema.json): canonical JSON envelope and object schema.
- [rmp.proto](rmp.proto): protobuf service and message projection.
- [openapi.yaml](openapi.yaml): HTTP OpenAPI mapping.
- [http-mapping.md](http-mapping.md): HTTP behavior and endpoint mapping.
- [mcp-mapping.md](mcp-mapping.md): MCP tools, resources, prompts, and JSON-RPC
  mapping.
- [security-model.md](security-model.md): auth, permissions, redaction,
  provenance, and audit requirements.
- [version-negotiation.md](version-negotiation.md): compatibility and upgrade
  rules.
- [reference-client/python/rmp_client.py](reference-client/python/rmp_client.py):
  minimal dependency-free Python reference client.

## Design Rule

RMP does not let model output become truth by default. Models may propose memory,
summaries, candidates, and explanations. Reality Graph decides what evidence
exists, what policy permits, and what belief state is current.

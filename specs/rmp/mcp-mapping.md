# RMP MCP Mapping

MCP is a tool and context protocol. RMP is a memory protocol. The MCP mapping
allows existing MCP clients to call RMP operations without losing RMP semantics.

This mapping follows the MCP pattern of JSON-RPC messages, resources, tools, and
capability negotiation. In current MCP documentation, base protocol messages use
JSON-RPC 2.0, lifecycle handles capability negotiation, and server features
include resources, prompts, and tools.

## MCP Tools

Each RMP operation is exposed as a tool with a strict input schema matching the
RMP envelope:

| RMP operation | MCP tool |
| --- | --- |
| `REMEMBER` | `rmp_remember` |
| `RECALL` | `rmp_recall` |
| `VERIFY` | `rmp_verify` |
| `REVISE` | `rmp_revise` |
| `FORGET` | `rmp_forget` |
| `EXPLAIN` | `rmp_explain` |
| `SIMULATE` | `rmp_simulate` |
| `GROUND` | `rmp_ground` |
| `COMPRESS` | `rmp_compress` |
| `SHARE` | `rmp_share` |
| `AUDIT` | `rmp_audit` |

Write tools (`rmp_remember`, `rmp_revise`, `rmp_forget`, `rmp_share`) are
dangerous by default and must require explicit policy gates.

## MCP Resources

RMP objects map to MCP resources:

| Resource template | Object |
| --- | --- |
| `rmp://memories/{id}` | `Memory` |
| `rmp://claims/{id}` | `Claim` |
| `rmp://evidence/{source_id}` | `Evidence` |
| `rmp://beliefs/{id}` | `Belief` |
| `rmp://timelines/{id}` | `Timeline` |
| `rmp://context-packs/{id}` | `ContextPack` |
| `rmp://contradictions/{id}` | `Contradiction` |
| `rmp://causal-traces/{id}` | `CausalTrace` |
| `rmp://simulation-traces/{id}` | `SimulationTrace` |
| `rmp://audit/{id}` | `AuditEvent` |

Resources may return compact summaries by default. Raw source excerpts require a
source read capability and must honor redaction policy.

## MCP JSON-RPC Tool Call Example

```json
{
  "jsonrpc": "2.0",
  "id": "mcp_req_001",
  "method": "tools/call",
  "params": {
    "name": "rmp_recall",
    "arguments": {
      "protocol": "rmp",
      "version": "1.0.0",
      "operation": "RECALL",
      "request_id": "req_001",
      "actor": {
        "principal_id": "user_123",
        "agent_id": "agent_research",
        "tenant_id": "tenant_lab"
      },
      "request": {
        "task": "Prepare context for the Oracle employment question."
      }
    }
  }
}
```

## MCP Tool Result Shape

The MCP tool result should include:

- `content`: concise human/model-readable text.
- `structuredContent`: the full RMP response object.
- `security`: permission scope, data provenance, taint status, source trust, and
  audit event ID.
- `isError`: true only for tool execution errors, not ordinary RMP states such
  as `needs_review`.

## MCP Prompts

RMP servers may expose prompts:

- `rmp_answer_with_evidence`
- `rmp_explain_belief`
- `rmp_review_memory_candidate`
- `rmp_compare_historical_state`
- `rmp_run_counterfactual_review`

Prompts must reference RMP resources or context packs. They must not inline raw
source text unless the caller has source-read permission.

## MCP Security Requirements

- Hosts should show the user when a write-capable RMP tool is requested.
- RMP servers must include audit metadata in every tool response.
- Prompt-injection-tainted evidence must be labeled in `structuredContent`.
- MCP clients that do not support required capabilities should receive a
  version or capability error, not degraded unsafe output.

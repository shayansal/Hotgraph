# RMP HTTP Mapping

RMP over HTTP uses JSON envelopes from `rmp.schema.json`. The canonical endpoint
is `POST /rmp/v1/exchange`; convenience endpoints map one-to-one to operations.

## Common Headers

| Header | Required | Meaning |
| --- | --- | --- |
| `Authorization: Bearer <token>` | yes | Principal, agent, tenant, and capability token. |
| `Content-Type: application/rmp+json` | yes | RMP JSON envelope. |
| `Accept: application/rmp+json` | yes | RMP JSON response. |
| `RMP-Version` | recommended | Requested protocol version, for example `1.0.0`. |
| `RMP-Min-Version` | optional | Oldest compatible version accepted by the client. |
| `Idempotency-Key` | required for writes | Required for `REMEMBER`, `REVISE`, `FORGET`, and `SHARE`. |
| `X-RG-Tenant` | recommended | Tenant boundary for request routing and audit. |

## Endpoints

| Operation | Method | Path |
| --- | --- | --- |
| Any operation | `POST` | `/rmp/v1/exchange` |
| `REMEMBER` | `POST` | `/rmp/v1/remember` |
| `RECALL` | `POST` | `/rmp/v1/recall` |
| `VERIFY` | `POST` | `/rmp/v1/verify` |
| `REVISE` | `POST` | `/rmp/v1/revise` |
| `FORGET` | `POST` | `/rmp/v1/forget` |
| `EXPLAIN` | `POST` | `/rmp/v1/explain` |
| `SIMULATE` | `POST` | `/rmp/v1/simulate` |
| `GROUND` | `POST` | `/rmp/v1/ground` |
| `COMPRESS` | `POST` | `/rmp/v1/compress` |
| `SHARE` | `POST` | `/rmp/v1/share` |
| `AUDIT` | `POST` | `/rmp/v1/audit` |
| Version negotiation | `GET` | `/rmp/v1/capabilities` |

## Status Codes

| Status | Meaning |
| --- | --- |
| `200` | Operation completed. |
| `202` | Operation accepted for review or asynchronous processing. |
| `400` | Invalid envelope, unsupported object, or malformed temporal context. |
| `401` | Missing or invalid authentication. |
| `403` | Permission denied by tenant, source, memory, or share policy. |
| `409` | Contradiction, revision conflict, or idempotency conflict. |
| `410` | Requested memory or evidence was redacted or deleted. |
| `422` | Valid schema but semantically invalid operation. |
| `426` | Version negotiation failed. |
| `429` | Rate or budget exceeded. |
| `500` | Internal server error. |

## Request Example

```http
POST /rmp/v1/verify HTTP/1.1
Authorization: Bearer rg_capability_token
Content-Type: application/rmp+json
Accept: application/rmp+json
RMP-Version: 1.0.0

{
  "protocol": "rmp",
  "version": "1.0.0",
  "operation": "VERIFY",
  "request_id": "req_001",
  "actor": {
    "principal_id": "user_123",
    "agent_id": "agent_research",
    "tenant_id": "tenant_lab"
  },
  "temporal_context": {
    "valid_at": "2024-01-01T00:00:00Z",
    "known_at": "2026-05-12T00:00:00Z"
  },
  "request": {
    "claim": {
      "id": "claim_candidate",
      "subject": "person:123",
      "predicate": "WORKED_AT",
      "object": "company:456",
      "confidence": 0.8,
      "source_ids": ["source:abc"],
      "status": "candidate"
    }
  }
}
```

## Response Rules

- Every successful response must include `status`.
- Any claim, memory, belief, context pack, causal trace, or simulation trace that
  influenced a model-facing answer must include source IDs or explicit redaction
  metadata.
- A response may omit raw excerpts in no-raw-source mode, but must preserve
  source IDs and content hashes.
- `SIMULATE` responses must label outputs as simulation, not fact.
- `FORGET` responses should return audit events rather than deleted payloads.

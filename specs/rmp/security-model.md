# RMP Security Model

RMP assumes memory is sensitive. It can contain private user preferences,
enterprise records, agent plans, source excerpts, beliefs, and simulations.

## Principals

Every request must identify:

- `principal_id`: human, service account, or workload identity.
- `tenant_id`: tenant or isolation boundary.
- `agent_id`: optional model or agent identity.
- `session_id`: optional run/session identifier.

Anonymous RMP requests are not allowed.

## Capability Model

Capabilities are scoped by operation:

- Read capabilities: `RECALL`, `VERIFY`, `EXPLAIN`, `GROUND`, `COMPRESS`,
  `AUDIT`.
- Write capabilities: `REMEMBER`, `REVISE`, `FORGET`, `SHARE`.
- Simulation capability: `SIMULATE`.

Capabilities must also be scoped to tenants, memory spaces, source policies, and
time windows. A capability that grants `RECALL` does not automatically grant raw
source access.

## Permission Objects

The `Permission` object carries:

- scope: private, agent, team, organization, tenant, public, or federated.
- capabilities: allowed operations.
- source policy: which source IDs or classes are readable.
- memory policy: which memories can be read, revised, forgotten, or shared.
- redaction policy: which fields must be removed or transformed.
- share policy: where memory can move.
- human review requirement.

## Provenance and Evidence

Every model-facing claim must be backed by at least one of:

- source ID and content hash,
- assertion ID whose source IDs are accessible,
- memory ID with source IDs,
- explicit redaction marker and audit event.

No-raw-source mode may remove excerpts and URIs. It must not remove source IDs,
content hashes, claim IDs, or audit metadata.

## Write Safety

`REMEMBER`, `REVISE`, `FORGET`, and `SHARE` require:

- idempotency key,
- write capability,
- tenant match,
- source-backed evidence or candidate-review status,
- audit event,
- optional human review depending on policy.

`FORGET` is policy application, not silent deletion. Implementations must record
whether memory was archived, redacted, legally held, or deleted.

## Prompt Injection and Taint

Evidence can carry taint labels such as:

- `external_unverified`,
- `prompt_injection_suspected`,
- `tool_output`,
- `secret_adjacent`,
- `quarantined`.

Tainted evidence may still be returned, but it must be labeled and may be
excluded from model context depending on policy.

## Federation

Federated RMP calls must preserve:

- origin graph ID,
- trust boundary,
- source boundary label,
- remote attestation status when available,
- permissioned join decision,
- partial result warnings.

Cross-graph sharing must never erase the source graph or trust boundary.

## Audit

Every operation must emit an audit event containing actor, operation, resource
IDs, decision, reason, transaction time, and request ID. Audit logs must be
append-only and queryable through `AUDIT`.

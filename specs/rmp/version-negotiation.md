# RMP Version Negotiation

RMP uses semantic protocol versions. Version `1.0.0` is the initial draft in
this repository.

## HTTP Negotiation

Clients should send:

```http
RMP-Version: 1.0.0
RMP-Min-Version: 1.0.0
Accept: application/rmp+json
```

Servers should respond with:

```http
RMP-Version: 1.0.0
RMP-Supported-Versions: 1.0.0
```

If no compatible version exists, the server returns `426 Upgrade Required` with
an RMP error payload:

```json
{
  "status": "error",
  "error": {
    "code": "unsupported_version",
    "message": "No compatible RMP version found."
  }
}
```

## Capability Discovery

`GET /rmp/v1/capabilities` returns:

- supported protocol versions,
- supported operations,
- supported object types,
- maximum request size,
- streaming support,
- no-raw-source support,
- required security features,
- MCP mapping version,
- protobuf package version.

## Compatibility Rules

Patch versions may add optional fields and new enum values if unknown values are
ignored safely. Minor versions may add operations or object types. Major versions
may change required fields or semantics.

Clients must ignore unknown optional fields. Servers must reject unknown required
operation semantics with `unsupported_operation` or `unsupported_object`.

## Schema Evolution

RMP objects should follow these rules:

- Add optional fields before required fields.
- Do not remove fields in a minor version.
- Do not change the meaning of existing fields.
- Keep redaction, provenance, and audit metadata backward compatible.
- Preserve source IDs and content hashes across all compatible versions.

## Model Provider Independence

RMP never requires a model name. Clients may send model preferences in transport
metadata, but RMP semantics must be independent of the model provider. A
provider-specific adapter may be pinned for reproducibility, but the protocol
must not depend on it.

# API Compatibility Policy

The `/v1` API is additive-only within the major version.

## Stable Contract Rules

- Existing request fields cannot change type or meaning.
- Existing response fields cannot be removed or renamed.
- New response fields are allowed.
- New optional request fields are allowed.
- New required request fields require a new major API version.
- Error responses must include a machine-stable `code` and a human-readable
  `error`.
- Deprecated endpoints must return deprecation and sunset headers before
  removal in a later major version.

## Contract Tests

Production readiness requires golden fixtures for:

- entity create and read
- source create and read
- assertion create and read
- graph query
- path query
- evidence pack generation
- AI context pack generation
- auth failures
- idempotency replay

Any breaking fixture change must fail CI unless a major version and migration
note are added in the same change.


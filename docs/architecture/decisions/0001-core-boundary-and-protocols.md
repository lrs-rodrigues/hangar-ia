# ADR 0001: Protocol-neutral core with edge adapters

**Status:** Accepted

## Decision

Hangar will define a versioned native API (gRPC + HTTP/JSON gateway) and canonical event contracts. MCP, A2A, UTCP, and ACP integrations are isolated adapters, each with contract tests against the native API.

## Rationale

MCP is designed for host/client/server context and tool integration, while A2A is for independent agent discovery and communication. UTCP describes direct native tool calls across transports. They are complementary, not interchangeable. Treating one as the database API would couple tenancy, authorization, retrieval, and lifecycle semantics to an external protocol's release cadence.

## Consequences

- First-party clients use the native API; adapters evolve independently.
- The core owns authentication, authorization, policy, audit, rate limits, and schemas.
- Every adapter declares supported capabilities and preserves provenance.
- ACP support remains deferred until the selected ACP specification and target clients are named.

## Implementation status

The initial native HTTP API, same-process gRPC service, CLI, and thin stdio MCP
adapter implement this decision. A2A, UTCP, and ACP remain deferred rather than
being represented by speculative compatibility endpoints.

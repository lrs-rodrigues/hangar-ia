# ADR 0010: Governed MCP write and lifecycle tools

**Status:** Accepted

## Decision

Extend the local stdio MCP adapter with a deliberately small set of write and
management tools: text-document ingestion, ingestion-job status/retry, and
durable-memory lifecycle transitions. The adapter maps each tool directly to
the existing native HTTP endpoint. It owns neither authorization nor storage.

The server remains the authority for organization/workspace scope, RBAC,
quotas, idempotency, lifecycle validity, guardrails, provenance, audit, and
the asynchronous ingestion publication protocol. In particular, document text
is untrusted data; a proposed memory is not published automatically; retry and
lifecycle transitions require the existing owner role.

The adapter does not expose API-key administration, guardrail policy mutation,
exports, direct filesystem/blob-store access, or arbitrary HTTP forwarding.
The lifecycle tool is marked as destructive because expiry and supersession are
terminal or visibility-changing operations and require explicit host consent.

## Why

Read-only retrieval makes the MCP useful as a context source, but prevents an
agent from submitting a newly discovered document and following it through the
governed pipeline. A curated set of native mutations permits this workflow
without turning MCP into an unbounded administration tunnel or duplicating the
core policy model at the protocol edge.

## Consequences

- A writer-scoped key can queue a document and observe its job, but cannot
  publish or expire durable memory.
- An owner-scoped key can retry a dead-letter job and make permitted lifecycle
  transitions; the core rejects invalid transitions and audits all mutations.
- Hosts retain their normal MCP consent boundary for mutations. The adapter's
  tool descriptions and annotations make side effects explicit.
- The MCP tool list is additive and preserves the native HTTP API as the
  product boundary.

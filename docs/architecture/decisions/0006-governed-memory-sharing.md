# ADR 0006: Canonical, reviewable memory-sharing grants

**Status:** Proposed

## Decision

Hangar shares durable memory through canonical, tenant-scoped ACL grants in
`redb`; it never copies the memory into a target workspace. A grant names one
published source memory and one audience: organization, workspace, agent API
identity, or user API identity. It progresses through
`pending → approved/rejected/revoked`; only an owner of the source workspace
may review it.

Each retrieval checks both the source memory lifecycle/retention and the grant
again. A source memory becoming expired or superseded therefore disappears from
all shared context without a fan-out mutation. Same-organization is mandatory;
cross-organization exchange is future explicit federation.

## Context packages

The core compiles a response rather than storing a reusable prompt. It ranks
authorized local and approved shared memories, includes only whole evidence
items that fit a caller-selected bounded token budget, and returns provenance,
source workspace, content hash, memory version, and share ID. Retrieved text is
labelled untrusted data. It cannot modify instructions, policy, authorization,
or tool permissions.

## Rationale

Copying a memory to every target workspace would create revocation, expiry,
provenance, and conflict-reconciliation problems. A small canonical ACL record
is transactional with its review/audit/outbox event and keeps the one-binary,
one-volume profile. Review state and a source-memory-version snapshot make
decisions visible without treating an LLM extraction as organization truth.

## Consequences

- API-key subjects declare whether they represent an `agent` or `user`; grants
  to either match the subject type and immutable key identity exactly.
- Organization and workspace grants do not bypass normal authentication or the
  request workspace scope.
- Duplicate active grants for the same memory/audience are rejected as a
  deterministic conflict. Clients must review/revoke the existing grant rather
  than silently create competing ACLs.
- Owners can audit source-workspace proposals/reviews; every context read is
  audited and share mutations appear in the canonical outbox.
- More expressive attribute policy remains the later guardrail/policy engine;
  it can add a deny decision before context is returned without changing the
  canonical grant model.

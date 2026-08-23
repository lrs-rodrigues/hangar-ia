# ADR 0007: Ephemeral working memory with explicit durable promotion

**Status:** Accepted

## Decision

Keep working memory in a bounded in-process session store. Scope every session
to organization, workspace, and authenticated creator principal; enforce TTL
and byte/entry/session limits on the server; and discard the store on process
restart. A session entry becomes durable only through an explicit promotion
operation that creates a new `proposed` memory with immutable session-entry
provenance. No model-generated summary or promotion is automatic.

Durable retention is represented by `indefinite` or `expire_at`. When an
expiry is due, transition any non-terminal item to `expired` in `redb`,
write its system audit event, and append the lifecycle outbox event
transactionally. Retain the expired metadata
for audit; destructive purge and legal holds require a later governed design.

## Rationale

Persisting every transient exchange would make the default deployment more
expensive, complicate deletion/retention, and silently turn unreviewed agent
output into organizational history. An external cache would violate the
one-container/one-volume baseline. Bounded process-local state is enough for a
working session while the explicit boundary makes its loss on restart honest
and predictable.

## Consequences

- Working sessions are not shareable or recoverable after restart; governed
  sharing is a separate control-plane concern.
- Session content remains untrusted data and cannot affect authorization,
  policies, or tools.
- Promotion preserves evidence but still requires validation and publication.
- Expiration is observable and replayable; it is not an invisible filter.

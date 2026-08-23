# Working and durable memory lifecycle

This vertical provides private working sessions and durable proposed memories
without a required external service. It is deliberately conservative: an agent
can record untrusted working data and request promotion, but cannot turn it
into retrievable shared knowledge by itself.

## Working memory

`HangarStore` owns a bounded in-process session map. A session is scoped by
organization, workspace, and creator principal. The server prunes it before
every session operation, and never serializes it to `redb`, blobs, indexes, or
the outbox. Restarting the process therefore removes all working state.

Each session accepts typed `note`, `tool_output`, and `observation` entries plus
one explicitly supplied summary. Every entry and summary has a SHA-256 hash,
principal, timestamp, and (for summaries) version. The limits are hard server
limits, not client hints: 30-minute default TTL / 24-hour maximum, 1,024
sessions, 64 entries per session, 8 KiB per entry or summary, and 64 KiB total.
The service rejects capacity overflow instead of evicting context that a caller
may still depend on.

Working content is always untrusted data. It cannot grant access, alter a
policy, invoke a tool, or become durable automatically.

## Promotion and durable state

Promotion copies one live session entry into a new durable memory at
`proposed`. Its provenance records the session ID, entry ID/hash, and session
creator. The API caller then follows the existing owner-controlled state graph:

```text
proposed → validated → published → superseded
                 │          └→ expired
proposed/validated ──────────→ expired
```

Only published and unexpired memory is retrievable. The replacement for a
superseded memory must be published, unexpired, and in the same workspace.
Durable expiry is a state transition, not a read-time omission: startup and
durable-memory reads materialize any non-terminal due item as `expired`, increment the memory version, write a
system audit event, and append a metadata-only `memory.lifecycle_changed.v1` outbox event with
`reason=retention_expired` in the same `redb` transaction.

The initial retention policy is `indefinite` or `expire_at`; expiry must be no
more than one year after the write. Metadata remains after expiry for audit and
lineage. Destructive retention purge, legal holds, classification-based rules,
and organization-wide sharing are deliberately outside this vertical and must
preserve the lifecycle/provenance contract when introduced.

## Acceptance checks

- A different API-key principal cannot read, mutate, or promote a live session,
  even with access to the same workspace.
- A session entry never appears in durable retrieval before explicit promotion
  and owner-controlled publication.
- A promoted memory keeps source hash and session provenance after the session
  expires or the server restarts.
- Expired durable memory is non-retrievable, auditable, and replayable through
  the canonical outbox.

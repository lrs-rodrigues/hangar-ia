# Governed sharing and context packages

## What is shared

The source workspace remains the owner of a durable memory. Sharing creates a
small ACL grant referencing that memory; it never copies content to another
workspace. This means source provenance, lifecycle, expiry, retention and a
future replacement reference remain authoritative.

The initial audiences are:

| Audience | Reader match |
| --- | --- |
| `organization` | Any authorized subject in the same organization |
| `workspace` | Any authorized subject querying the named workspace |
| `agent` | Exactly the named API-key identity whose subject type is `agent` |
| `user` | Exactly the named API-key identity whose subject type is `user` |

There is no implicit cross-organization sharing, wildcard identity, or content
replication. A source workspace owner reviews each proposal. States are
`pending`, `approved`, `rejected`, and `revoked`. An approved grant becomes
ineffective if either its own expiry or the source memory's lifecycle/expiry
makes it ineligible.

## Conflict and audit behavior

The server rejects a second pending/approved grant for the same memory and
audience. This prevents competing ACL decisions. A reviewer can reject or
revoke the first grant and then approve a replacement. The grant records
proposer, reviewer, source-memory version, timestamps, note and its own
version; APIs also write audit and canonical outbox events without memory
content.

## Context packages

`POST /v1/context-packages` compiles authorized local published memories plus
approved shares for a query. It applies lexical ranking and, for a local memory
whose declared source matches an authorized hybrid document result, a bounded
source-rank boost. Shared memories never use the source-workspace document
index. The package adds whole memory/evidence items only while they fit the
explicit token budget.
The token estimate is intentionally conservative (`ceil(UTF-8 bytes / 4)`) and
is a budget guard, not model tokenizer accounting.

Each returned item includes a citation: memory ID, content hash, source,
version, source workspace and (for shared material) share ID. `untrusted` is
always `true`: clients must render this as data, never concatenate it into
system instructions or use it to authorize a tool.

## Limitations of the alpha identity model

API keys are the current identity mechanism. `subject_kind` declares whether a
key represents an agent or user and its issued UUID is the stable target for a
direct grant. OIDC/service-account claims and attribute policy are later
extensions; they must map to the same canonical subject/audience check rather
than move authorization into a model or protocol adapter.

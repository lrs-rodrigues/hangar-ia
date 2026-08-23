# Reference architecture

```text
MCP / A2A / UTCP / ACP / REST-gRPC clients
                 │
          Protocol adapters
                 │
    Core API: identity, authorization, policy,
  memory lifecycle, retrieval planning, audit
        │              │               │
 ingestion workers   context compiler  event stream
        │              │               │
 blob store ─ metadata/ACL ─ vector index ─ graph projection ─ session cache
```

## Planes

The **control plane** owns organizations, workspaces, identities, service accounts, roles/attributes, policy bundles, skill catalogs, connector configuration, quotas, and audit configuration. Its mutations are transactional and strongly consistent within a deployment region.

The **data plane** stores and serves knowledge:

| Store | Responsibility | Baseline |
| --- | --- | --- |
| Canonical metadata | tenancy, ACLs, provenance, lifecycle, catalog, graph edges | redb, embedded |
| Blob store | original files, normalized artifacts, large snapshots | content-addressed local filesystem |
| Text index | lexical candidates and BM25 | Tantivy, embedded |
| Vector index | embedding candidates | USearch, embedded |
| Session store | ephemeral working memory, leases, rate limits | in-process bounded store; persisted only when promoted |

The redb record and immutable event are canonical. Text, embedding, and graph traversal indexes are rebuildable projections, making re-embedding, schema evolution, and recovery safe.

## Memory and retrieval

- **Working memory:** session-scoped notes, tool outputs, and compact summaries; TTL, size limits, and explicit promotion only.
- **Durable memory:** source-backed facts, decisions, preferences, procedures, and summaries; versioned with freshness and retention policy.
- **Shared knowledge:** curated durable memory published to workspace or organization scope; access evaluated per retrieval.

A write moves through `proposed → validated → published → superseded/expired`. Only published and unexpired items enter shared retrieval. Each item includes tenant scope, author/agent, source/evidence, confidence, timestamps, content hash, version, expiry, and replacement reference.

Retrieval authenticates the caller, applies ACL filters at source, combines lexical/vector/recency/graph/policy scores, fetches evidence, and returns a token-budgeted context package with citations, provenance, policy notices, and a retrieval trace—not an opaque giant prompt.

## File performance

Clients stream uploads to the Hangar API; the server writes content-addressed blobs to its mounted data volume and persists a manifest before emitting an ingest event. Workers scan, normalize, chunk, enrich, embed, extract relations, and write projections asynchronously. Content-addressable deduplication, idempotency keys, bounded in-process queues, durable retry/dead-letter records, and reprocessing by pipeline version are mandatory. Database blobs and synchronous embedding on the upload path are prohibited.

## Protocol strategy

- **MCP:** curated resources and tools such as search, get context, propose memory, and list skills.
- **A2A:** an Agent Card and task-facing collaboration surface when Hangar acts as a knowledge agent; not the storage API.
- **UTCP:** tool descriptions that call Hangar's native HTTPS/gRPC endpoints directly.
- **ACP:** an isolated adapter. “ACP” denotes more than one emerging agent protocol, so adoption waits for a named target specification and client.

All adapters map to the same canonical API, identity claims, policy decision, and audit event.

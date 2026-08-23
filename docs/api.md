# HTTP API — alpha

The alpha API uses a bootstrap token and scoped API keys. All requests except `GET /health` require `Authorization: Bearer <api-key>`. The server persists only a SHA-256 hash of an API key and emits audit events for key, memory, retrieval, policy, and catalog actions. OIDC remains future work; the embedded deterministic guardrail evaluator is available now.

## Bootstrap an organization

Set `HANGAR_BOOTSTRAP_TOKEN` when starting the server. It is used only to create the first organization owner key:

`POST /v1/organizations`

```json
{ "organization_id": "acme" }
```

Use `Authorization: Bearer <bootstrap-token>`. The response exposes an owner API key exactly once. Store it in a secret manager.

## Create a scoped API key

`POST /v1/api-keys` requires an organization owner key. Roles are `owner`, `writer`, and `reader`; a workspace scope is optional.

```json
{
  "organization_id": "acme",
  "workspace_id": "payments",
  "role": "writer",
  "subject_kind": "agent"
}
```

`subject_kind` is `agent` by default and may be `user`. Its issued API-key ID
is the stable identity used by direct governed-sharing grants.

## Health

`GET /health`

`GET /readyz` verifies that the embedded canonical store and blob directory are
available. `GET /metrics` exposes content-free Prometheus text metrics. Neither
endpoint requires an API key; keep them on an internal network boundary where
deployment policy requires it.

## Operations, usage, and export

An organization owner can inspect only an authorized workspace's resource use:

`GET /v1/operations/usage?organization_id=acme&workspace_id=payments`

It reports durable memory/document/blob counts and bytes and active ingestion
queue depth. The server applies configured per-workspace document, blob, and
memory quotas to every HTTP and gRPC write. An idempotent document replay does
not consume quota a second time.

An owner can export one workspace with:

`GET /v1/exports/workspace?organization_id=acme&workspace_id=payments`

Export evaluates the `export` guardrail action and writes an audit event. It
contains only the workspace's memories, original document payloads, skills,
and guardrail policies. Returned content is labelled untrusted data; API keys,
audit history, blobs, and other workspaces are never included. See
[`operations.md`](operations.md) for offline backup/restore, verification, and
all configured limits.

## Create proposed memory

`POST /v1/memories`

```json
{
  "organization_id": "acme",
  "workspace_id": "payments",
  "content": "Use OIDC for service authentication.",
  "source": "architecture-decision-12",
  "confidence": 0.9,
  "expires_at_unix_ms": 1800000000000
}
```

The initial lifecycle is always `proposed`; promotion is intentionally not automatic.

This endpoint needs a `writer` or `owner` key with access to the requested organization/workspace. Reading needs `reader`, `writer`, or `owner`.

## Govern memory lifecycle

`POST /v1/memories/{id}/lifecycle?organization_id=acme&workspace_id=payments` requires an `owner` key. Valid transitions are:

```text
proposed → validated → published → superseded
                 │          └→ expired
proposed/validated ──────────→ expired
```

```json
{ "lifecycle": "validated" }
```

Only `published` and unexpired memories participate in shared retrieval. Every memory includes its creator, SHA-256 content hash, structured provenance, retention state, source, timestamps, version, optional expiry, and optional replacement reference. A `superseded` transition must identify a published, unexpired `superseded_by` in the same workspace. Expiry materializes every due non-terminal memory as terminal `expired` on startup and before durable-memory reads; it writes a system audit event, emits an outbox event, and retains the metadata rather than silently deleting it.

`expires_at_unix_ms`, whether supplied at proposal or validation/publishing, must be in the future and no more than one year away. This embedded profile intentionally has no destructive purge or legal-hold API yet.

## Working-memory sessions

Working memory is in-process only: it is private to the API-key principal that
created it, scoped to one organization/workspace, and is discarded on server
restart or TTL expiry. It is untrusted data, not executable instructions.

`POST /v1/sessions` requires a writer key:

```json
{
  "organization_id": "acme",
  "workspace_id": "payments",
  "ttl_ms": 1800000
}
```

TTL defaults to 30 minutes and is capped at 24 hours. The embedded limits are
1,024 live sessions process-wide, 64 entries/session, 8 KiB/entry or summary,
and 64 KiB/session. Capacity is rejected explicitly; entries are never silently
evicted. The session owner may read it with
`GET /v1/sessions/{id}?organization_id=acme&workspace_id=payments`.

Append an untrusted note, tool output, or observation with a writer key:

`POST /v1/sessions/{id}/entries?organization_id=acme&workspace_id=payments`

```json
{ "kind": "tool_output", "content": "OIDC credential refresh succeeded." }
```

Write an explicit compact summary with
`PUT /v1/sessions/{id}/summary?organization_id=acme&workspace_id=payments`:

```json
{ "content": "Authentication work is complete; refresh was verified." }
```

The server records hash, author, time, and version, but does not ask a model to
summarize or promote data automatically.

## Promote working memory

`POST /v1/sessions/{session_id}/entries/{entry_id}/promote?organization_id=acme&workspace_id=payments`
requires the session owner's writer key. It makes a new **durable** memory in
`proposed` state; it does not publish it:

```json
{
  "confidence": 0.8,
  "expires_at_unix_ms": 1800000000000
}
```

The resulting durable memory contains immutable session/entry IDs, entry hash,
and original session principal in its `provenance`. The normal owner-governed
`proposed → validated → published` lifecycle remains mandatory.

## Read and retrieve memory

`GET /v1/memories/{id}?organization_id=acme&workspace_id=payments`

`POST /v1/retrieve`

```json
{
  "organization_id": "acme",
  "workspace_id": "payments",
  "query": "service authentication",
  "limit": 8
}
```

Memory retrieval is currently lexical and always filters by organization and workspace before scoring. Document retrieval already provides the embedded Tantivy, vector, and Graph-RAG paths; durable-memory retrieval will adopt those projections in its own vertical while preserving this API contract.

## Governed memory sharing

`POST /v1/memory-shares` lets a writer propose a grant for one already
published, unexpired memory in its source workspace. The grant is `pending`
until an owner reviews it; no content is copied to a target workspace.

```json
{
  "organization_id": "acme",
  "source_workspace_id": "payments",
  "memory_id": "018f...",
  "audience": { "kind": "workspace", "workspace_id": "security" },
  "expires_at_unix_ms": 1767225600000
}
```

Audiences are `{ "kind": "organization" }`, `{ "kind": "workspace",
"workspace_id": "..." }`, `{ "kind": "agent", "subject_id": "uuid" }`,
or `{ "kind": "user", "subject_id": "uuid" }`. All stay inside one
organization. Creating a second pending or approved grant for the same memory
and audience returns a conflict-like validation error.

`POST /v1/memory-shares/{id}/review?organization_id=acme&workspace_id=payments`
requires an owner of the source workspace. It accepts a valid state transition:

```json
{ "state": "approved", "review_note": "source reviewed" }
```

States are `pending → approved/rejected/revoked` and `approved → revoked`.
`GET /v1/memory-shares?organization_id=acme&workspace_id=payments` is an
owner-only view of grants issued by that source workspace.

## Compile a bounded context package

`POST /v1/context-packages` requires a reader key. It ranks local published
memories and approved grants visible to the requester, then returns only whole
evidence items that fit the explicit `token_budget` (1–8192).

```json
{
  "organization_id": "acme",
  "workspace_id": "security",
  "query": "credential rotation",
  "token_budget": 1200,
  "limit": 8
}
```

Every item carries memory/source/hash/version/share evidence and `untrusted:
true`. Retrieved content remains data: clients must not treat it as executable
instructions, a policy change, or authority to invoke a tool.

## Skills and deterministic guardrails

Skills and guardrail policies are scoped and versioned catalog records. A writer
creates a draft; an owner is required to publish/revoke a skill or to enforce/
retire a policy. Neither a skill body nor a retrieved document can grant a
permission: the server evaluates RBAC and the enforced policy before returning
memory, RAG context, or a published skill.

`POST /v1/skills` creates a draft skill. Its body is stored as portable content
and its declared tools are informational capability metadata, never tool
authority:

```json
{
  "organization_id": "acme",
  "workspace_id": "payments",
  "name": "release-check",
  "description": "Review a release checklist",
  "content": "Check the cited evidence before publishing.",
  "capabilities": {
    "declared_tools": ["github/issues"],
    "declared_context_actions": ["context_read"]
  }
}
```

`POST /v1/skills/{id}/lifecycle?organization_id=acme&workspace_id=payments`
accepts `{ "lifecycle": "published" }` or `{ "lifecycle": "revoked" }`.
Readers only see published records through `GET /v1/skills` and
`GET /v1/skills/{id}`. `POST /v1/skills/{id}/authorize-use` evaluates
`skill_use` and returns the skill with `content_trust: "untrusted_data"`; it
does not execute tools.

`POST /v1/guardrail-policies` creates a draft deterministic policy, for example:

```json
{
  "organization_id": "acme",
  "workspace_id": "payments",
  "name": "production-tool-boundary",
  "rules": [
    {
      "id": "deny-reader-production-deploy",
      "action": "tool_invoke",
      "effect": "deny",
      "roles": ["reader"],
      "targets": ["production-deploy"]
    }
  ]
}
```

Use `POST /v1/guardrail-policies/{id}/lifecycle` with `enforced` or `retired`;
owners can inspect their workspace catalog with `GET /v1/guardrail-policies`.
Rules match an action, optional role list, and exact target or `*`; a matching
`deny` always wins. With no matching enforced rule the scoped RBAC baseline is
preserved. Supported actions are `memory_read`, `memory_share`, `context_read`,
`export`, `skill_read`, `skill_use`, and `tool_invoke`.

`POST /v1/guardrails/evaluate` lets a protocol adapter or client request the
same server-side preflight used by native reads:

```json
{
  "organization_id": "acme",
  "workspace_id": "payments",
  "action": "tool_invoke",
  "target": "production-deploy"
}
```

It returns an auditable allow/deny decision with matched policy/rule IDs. A
denial is returned as `403`; clients must not reinterpret retrieved content as
instructions to override it.

## Store a blob

`POST /v1/blobs` accepts an arbitrary request body and requires these headers:

```text
X-Hangar-Organization-Id: acme
X-Hangar-Workspace-Id: payments
Content-Type: text/plain
```

The response includes the SHA-256 content address. Repeated content is deduplicated on disk.

## Ingest text documents

`POST /v1/documents` requires a `writer` or `owner` key and returns `202 Accepted`. The pipeline durably records the source payload and a queued job before responding. The single-node worker then splits text into paragraph-aware chunks of up to 1,000 characters. Only chunks from a `succeeded` job participate in retrieval.

Supply `Idempotency-Key` to safely repeat an upload. The response contains `document`, `job`, and `deduplicated`; the full input SHA-256 and pipeline version are also used to deduplicate equivalent work within the same workspace.

```json
{
  "organization_id": "acme",
  "workspace_id": "payments",
  "name": "security-runbook.md",
  "source": "git://example/security-runbook.md",
  "content": "The document text to index."
}
```

`GET /v1/documents/{id}?organization_id=acme&workspace_id=payments` returns document metadata to an authorized reader.

`GET /v1/ingestion/jobs/{id}?organization_id=acme&workspace_id=payments` returns the job state to an authorized reader. Job states are `queued`, `processing`, `succeeded`, `retry_wait`, and `dead_letter`. A worker retries bounded failures with backoff; an `owner` may requeue a dead letter through `POST /v1/ingestion/jobs/{id}/retry?organization_id=acme&workspace_id=payments`.

`POST /v1/retrieve/documents` accepts the same scoped query body as memory retrieval and returns matching chunks with `document_id`, `document_name`, `source`, `ordinal`, BM25 lexical `score`, optional `vector_score`, optional `graph_score`/`graph_hops`, `final_score`, `embedding_provider`, and `embedding_model_revision`. These fields are the citation contract for RAG clients.

The HTTP document and graph retrieval responses include
`content_trust: "untrusted_data"`; the native gRPC document response includes
`retrieved_content_is_untrusted: true`. Clients must preserve this boundary
when rendering results to a model.

The current embedded profile fuses candidates from a per-workspace Tantivy BM25 generation and a per-workspace USearch index. A candidate is returned only when its canonical manifest is `ready`, its document is `succeeded`, and its organization/workspace matches the request. The default `hashing-v1` provider is deterministic and offline; it proves the pipeline and does not claim production semantic quality.

## Retrieve Graph-RAG evidence

`POST /v1/retrieve/graph` requires a reader key and takes the normal scope,
query, optional `limit` (1–50), and optional `max_hops` (1–3):

```json
{
  "organization_id": "acme",
  "workspace_id": "payments",
  "query": "zephyr credential",
  "limit": 8,
  "max_hops": 2
}
```

It returns bounded, evidence-backed `co_occurs_with` relationships with source
and target entity names, confidence, hop count, and the cited document chunk.
The initial extractor is deterministic and local; its results are proposals,
not trusted instructions or authorization facts.

## Read canonical outbox events

`GET /v1/outbox/events?organization_id=acme&workspace_id=payments&after=<uuid>&limit=100`
requires an owner key. `after` is an exclusive immutable UUIDv7 cursor. The
response contains only events in the authorized organization/workspace; clients
must persist their own cursor and deduplicate by event ID. The service does not
acknowledge or delete events for a consumer.

## Rebuild a vector projection

`POST /v1/vector-index/rebuild` requires an organization `owner` key. It accepts the normal organization/workspace scope JSON and returns the number of canonical ready chunk vectors published into a replacement index generation.

```json
{
  "organization_id": "acme",
  "workspace_id": "payments"
}
```

The operation never uses the current ANN file as truth. It rebuilds from canonical manifests and succeeded documents, writes a sibling temporary file, and atomically publishes it. Start, success, and failure are recorded in the audit log.

## Rebuild a text projection

`POST /v1/text-index/rebuild` requires an organization `owner` key and accepts the same organization/workspace JSON. It creates and activates a fresh Tantivy BM25 generation only from canonical ready chunk records, returning `chunks_indexed`.

As with vector rebuilds, its start, success, and failure are audited. It never trusts an existing index directory as source data.

## Rebuild a graph projection

`POST /v1/graph/rebuild` requires an organization `owner` key and accepts the
normal organization/workspace JSON. It recreates graph entities, adjacency, and
edge evidence only from canonical ready graph manifests and succeeded documents,
returning `chunks_indexed`. Its start, success, and failure are audited.

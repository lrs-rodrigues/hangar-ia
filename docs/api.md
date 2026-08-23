# HTTP API — alpha

The alpha API uses a bootstrap token and scoped API keys. All requests except `GET /health` require `Authorization: Bearer <api-key>`. The server persists only a SHA-256 hash of an API key, and emits audit events for successful key, memory, retrieval, and blob actions. OIDC and policy-as-code are still future work.

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
  "role": "writer"
}
```

## Health

`GET /health`

## Create proposed memory

`POST /v1/memories`

```json
{
  "organization_id": "acme",
  "workspace_id": "payments",
  "content": "Use OIDC for service authentication.",
  "source": "architecture-decision-12",
  "confidence": 0.9
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

Only `published` and unexpired memories participate in shared retrieval. Every memory includes its creator, SHA-256 content hash, source, timestamps, version, optional expiry, and optional replacement reference. A `superseded` transition must identify `superseded_by` in the same workspace.

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

Retrieval is lexical in this first vertical and always filters by organization and workspace before scoring. Tantivy and vector/graph retrieval will replace this implementation while preserving the API contract.

## Store a blob

`POST /v1/blobs` accepts an arbitrary request body and requires these headers:

```text
X-Hangar-Organization-Id: acme
X-Hangar-Workspace-Id: payments
Content-Type: text/plain
```

The response includes the SHA-256 content address. Repeated content is deduplicated on disk.

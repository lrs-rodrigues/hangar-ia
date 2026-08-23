# Native contracts and integrations

Hangar owns authorization, policy, lifecycle, provenance, audit, and canonical
events. Protocol clients are adapters; they never receive direct storage access
or an independent permission model.

## Native API

HTTP/JSON remains the complete alpha API described in [`api.md`](api.md). Its
stable version prefix is `/v1`; clients must treat additive response fields as
forward-compatible.

The initial gRPC service is `hangar.v1.HangarService`, defined in
[`packages/hangar-server/proto/hangar/v1/hangar.proto`](../packages/hangar-server/proto/hangar/v1/hangar.proto).
It runs from the same `hangar-server` process at `HANGAR_GRPC_LISTEN_ADDR`
(default `127.0.0.1:50051`). gRPC sends the same scoped API key through standard
`authorization: Bearer <api-key>` metadata and applies the same server-side
authorization as HTTP. It deliberately begins with the highest-frequency health,
durable/working memory, promotion, and retrieval calls; HTTP remains the
canonical coverage path until a subsequent additive gRPC expansion covers
administration and projection operations.

No gRPC identity, storage, lifecycle, or audit behavior is separate from HTTP.

## CLI

`hangar-cli` is a thin HTTP client and never reads the mounted server volume.

```bash
cargo run -p hangar-cli -- --server-url http://127.0.0.1:8080 health
cargo run -p hangar-cli -- --api-key "$HANGAR_API_KEY" search-memory \
  --organization-id acme --workspace-id payments --query "OIDC"
```

It supports health, memory/document search, governed context compilation, and
proposal creation. It neither publishes memory nor bypasses policy or review.

## MCP adapter

`hangar-mcp` is a local stdio JSON-RPC adapter pinned to the compatible MCP
`2025-06-18` subset: initialization, ping, `tools/list`, and `tools/call`.
It uses `HANGAR_SERVER_URL` and `HANGAR_API_KEY` (or equivalent flags) to call
the native HTTP API.

```json
{
  "mcpServers": {
    "hangar": {
      "command": "hangar-mcp",
      "env": {
        "HANGAR_SERVER_URL": "http://127.0.0.1:8080",
        "HANGAR_API_KEY": "replace-with-scoped-key"
      }
    }
  }
}
```

The adapter exposes the following curated tools:

- `hangar_search_memory`
- `hangar_search_documents`
- `hangar_get_context`
- `hangar_propose_memory`
- `hangar_ingest_document`
- `hangar_get_ingestion_job`
- `hangar_retry_ingestion_job`
- `hangar_transition_memory`

Search tools return provenance-bearing data. Their descriptions and response
instructions explicitly mark it untrusted; the adapter does not expose policy,
guardrail, API-key, export, direct-storage, or arbitrary tool-execution
operations. Ingestion is writer-authorized and remains asynchronous. Retrying
an ingestion job and transitioning memory require an owner key; the server
still validates lifecycle rules, quotas, scope, audit and guardrails.
MCP tool invocation requires host/user consent in addition to Hangar's own
authorization.

For a persistent local server plus a containerized stdio adapter, use
[`deploy/local/`](../deploy/local/). Codex must launch the adapter as a stdio
child process; it should not be exposed as an HTTP service. The compose profile
keeps the API key in an ignored local `.env` file and connects the adapter to
the server only over Compose's private network.

## Deferred protocols

A2A is intentionally deferred until the knowledge-agent task and Agent Card
scope are defined. UTCP is deferred until its target consumer and transport
contract are selected. ACP remains deferred until a single named specification
and supported client are chosen. These decisions prevent speculative adapters
from redefining the core API.

## Contract tests

The workspace tests the CLI command validation, MCP JSON-RPC initialization and
tool discovery, and the gRPC service against the same authorization and storage
paths as HTTP. The Docker build is the canonical test environment:

```bash
docker build -t hangar-ai .
```

MCP protocol behavior is based on the [MCP lifecycle specification](https://modelcontextprotocol.io/specification/2025-03-26/basic/lifecycle)
and [tools specification](https://modelcontextprotocol.io/specification/2025-06-18/server/tools).

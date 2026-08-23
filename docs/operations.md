# Operations and recovery

Hangar's default deployment is one `hangar-server` process and one mounted
data directory. Only one process may own a data directory at a time.

## Probes and metrics

- `GET /health` is an unauthenticated liveness probe. It only proves that the
  HTTP process is answering.
- `GET /readyz` is an unauthenticated readiness probe. It verifies the mounted
  blob directory and opens canonical redb tables; it returns an error when the
  store is not usable.
- `GET /metrics` is Prometheus text exposition. It reports process start time,
  request/server-error counters, and the `hangar_up` readiness gauge. It never
  emits knowledge content, queries, API keys, principal IDs, or tenant labels.

An organization owner can inspect one authorized workspace through:

```text
GET /v1/operations/usage?organization_id=acme&workspace_id=payments
```

The result reports memory/document/blob counts and bytes plus the number of
queued or processing ingestion jobs. It is audited and is intentionally not a
global administrative listing.

## Limits and quotas

The server rejects writes before they consume unbounded disk or memory. These
environment variables are evaluated for every organization/workspace:

| Variable | Default | Meaning |
| --- | ---: | --- |
| `HANGAR_MAX_DOCUMENT_BYTES` | 1 MiB | One accepted document payload |
| `HANGAR_MAX_DOCUMENTS_PER_WORKSPACE` | 10,000 | Durable documents |
| `HANGAR_MAX_INGESTION_BYTES_PER_WORKSPACE` | 512 MiB | Original document payloads |
| `HANGAR_MAX_BLOB_BYTES` | 8 MiB | One blob upload |
| `HANGAR_MAX_BLOB_BYTES_PER_WORKSPACE` | 1 GiB | Blob manifest bytes |
| `HANGAR_MAX_MEMORIES_PER_WORKSPACE` | 50,000 | Durable memories |
| `HANGAR_MAX_MEMORY_BYTES_PER_WORKSPACE` | 64 MiB | Durable memory content |

Existing session, context-token, graph-hop, and retrieval-result caps continue
to apply. Limits are enforced inside the canonical store for HTTP and gRPC;
they are not client guidance. An idempotent document replay does not consume a
second quota unit.

## Optional local semantic model

`hashing-v1` remains the default, zero-download compatibility profile. The
optional `local-multilingual-v1` profile is provisioned deliberately; the
server never downloads model artifacts while serving ingestion or retrieval.

For a developer machine, install once into a persistent, dedicated directory:

```bash
hangar-server models install-local \
  --destination /var/lib/hangar/models/local-multilingual-v1

# This performs only manifest and SHA-256 validation; it does not use the network.
hangar-server models verify-local \
  --source /var/lib/hangar/models/local-multilingual-v1
```

To use that profile, set both values on the serving process (or Compose
service) before it starts:

```text
HANGAR_EMBEDDING_PROFILE=local-multilingual-v1
HANGAR_LOCAL_MODEL_DIR=/var/lib/hangar/models/local-multilingual-v1
```

The installer fetches the pinned multilingual ONNX artifact, runs a 384-vector
smoke check, and writes `hangar-local-model-manifest.json` with the provider,
revision, dimensions, and checksum of every artifact. It refuses a non-empty
destination. Enterprise deployments should provision the same directory through
their approved artifact pipeline, then run `verify-local` before deploying the
server. Do not put weights in the base image or let runtime containers fetch
them.

Changing the provider or model revision requires a controlled rebuild from the
canonical chunks. Never reuse a vector generation created by another embedding
profile.

## Backup, verification, and restore

Stop the server first. The commands intentionally refuse a concurrently owned
redb data directory rather than risk a cross-file-inconsistent copy.

```bash
# Server is stopped. The destination must not already exist.
hangar-server --data-dir /var/lib/hangar backup \
  --destination /backups/hangar-2026-08-23

# Validate all file checksums and the copied canonical redb database.
hangar-server verify-backup --source /backups/hangar-2026-08-23

# Restore only into a new, non-existent directory.
hangar-server restore \
  --source /backups/hangar-2026-08-23 \
  --destination /var/lib/hangar-restored
```

The backup contains the complete data tree and a
`hangar-backup-manifest.json` with SHA-256 checksums. It includes canonical
metadata, blobs, and derived vector/text/graph projections. It is not an
incremental backup format. Restore verifies first, publishes atomically, and
normal startup reconciliation rebuilds any interrupted projection safely.

For Docker, run maintenance commands with the server stopped and mount the
data volume plus a separate backup location into a one-off container. Override
the image entrypoint so it executes `hangar-server` directly.

[`deploy/local/`](../deploy/local/) provides a production-like single-host
Compose environment: persistent data and backup volumes, local-only API ports,
a health check, and an offline maintenance profile. It intentionally does not
claim a live replica. An embedded data directory has one active writer; copying
it while the server is running is neither replication nor a valid backup.

## Workspace export

An owner can export one workspace through:

```text
GET /v1/exports/workspace?organization_id=acme&workspace_id=payments
```

The action evaluates the `export` guardrail, emits an audit event, and returns
only that workspace's durable memories, original document payloads, skills,
and guardrail policies. It never returns API keys, audit history, blobs, or
another workspace. Every exported content field remains untrusted data.

Export is for review or a future migration importer; it is not a replacement
for a complete backup.

## Routine checks

1. Scrape `/metrics` and alert on `hangar_up == 0` or increasing server errors.
2. Poll owner-authorized workspace usage and investigate sustained queue growth
   or quota pressure before increasing a limit.
3. Create and verify a backup on a schedule appropriate for the deployment.
4. Periodically restore a backup into an isolated new directory and start a
   disposable server against it. A backup not restored is only an assumption.
5. Keep the data directory and backup media protected as sensitive data: they
   contain tenant knowledge even though API key plaintext is never persisted.

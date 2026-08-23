# Local production-like environment

This Compose environment runs one active Hangar server against persistent named
volumes and exposes its HTTP and gRPC ports only on the local machine. It is a
safe starting point for a developer or single-host pilot; it is not a
multi-node production topology.

## Start

Copy `.env.example` to `.env` and replace the two placeholders. Keep `.env`
private: it contains the bootstrap token and the API key used by the local MCP
adapter.

```bash
docker compose -f deploy/local/compose.yaml up --build -d hangar-server
docker compose -f deploy/local/compose.yaml ps
curl http://127.0.0.1:8080/readyz
```

Bootstrap an organization once, using the bootstrap token, then create a
workspace-scoped `agent` key with the least role needed by Codex. Put that key
in `HANGAR_API_KEY` in `.env`. Restarting `hangar-server` does not erase data:
the `hangar-local-data` volume holds the canonical store, blobs, and derived
indexes.

## Optional local semantic profile

The default `hashing-v1` profile needs no model download. To evaluate the
local multilingual semantic profile, first provision its weights once into the
persistent volume, while the normal server is stopped:

```bash
docker compose -f deploy/local/compose.yaml stop hangar-server
docker compose -f deploy/local/compose.yaml run --rm --no-deps backup \
  models install-local --destination /var/lib/hangar/models/local-multilingual-v1
docker compose -f deploy/local/compose.yaml run --rm --no-deps backup \
  models verify-local --source /var/lib/hangar/models/local-multilingual-v1
```

Set `HANGAR_EMBEDDING_PROFILE=local-multilingual-v1` in `.env`, then start the
server again. Provisioning is the only operation that downloads the model;
normal startup verifies the manifest and loads it without network access. A
profile/revision change requires rebuilding vector projections from canonical
chunks before treating retrieval results as comparable.

## Codex MCP process

The MCP adapter is intentionally a local stdio process. It is not exposed on a
TCP port and it has no direct access to the server volume. The Codex
configuration runs this exact command:

```text
docker compose -f C:\absolute\path\to\hangar-ia\deploy\local\compose.yaml run --rm -T hangar-mcp
```

Compose connects that one-off process to `hangar-server` on its private network.
Its standard output is reserved for MCP JSON-RPC; use `docker compose logs` for
server diagnostics instead.

## Backup, verification, and recovery drill

Hangar deliberately supports an offline verified snapshot only. Stop the
active server first; a backup while it owns `canonical.redb` fails by design.
The maintenance container mounts the data volume read/write only because redb
must open its advisory lock while checking the database; never run it alongside
the active server.

```bash
docker compose -f deploy/local/compose.yaml stop hangar-server
docker compose -f deploy/local/compose.yaml run --rm --no-deps backup \
  --data-dir /var/lib/hangar backup --destination /backups/hangar-2026-08-23
docker compose -f deploy/local/compose.yaml run --rm --no-deps backup \
  verify-backup --source /backups/hangar-2026-08-23
docker compose -f deploy/local/compose.yaml start hangar-server
```

For a restore drill, restore into a new named volume or an isolated host path;
never overwrite `hangar-local-data`. See [`docs/operations.md`](../../docs/operations.md)
for the full restore command and validation rules.

## About replicas

The embedded server currently has a single-writer data model. Starting a second
server against the active volume is unsafe and is refused by the database lock.
This Compose environment therefore provides one active instance plus verified
backup media, not a falsely advertised live replica. A real replica requires a
replication protocol and a consensus/failover design; it is intentionally not
emulated by copying an active volume.

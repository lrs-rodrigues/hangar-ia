# Hangar AI

<p align="center"><strong>Governed, portable context for AI agents.</strong><br />One embedded server, one persistent volume, and evidence agents can cite.</p>

<p align="center">
  <a href="https://github.com/lrs-rodrigues/hangar-ia/actions/workflows/ci.yml"><img src="https://github.com/lrs-rodrigues/hangar-ia/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="Apache-2.0" /></a>
  <img src="https://img.shields.io/badge/status-alpha-orange.svg" alt="Alpha" />
</p>

<p align="center"><a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.es.md">Español</a></p>

Hangar AI is an open-source knowledge and memory platform for AI agents. It provides governed documents, durable memory, private working sessions, and token-bounded context without copying whole conversations into every prompt or locking knowledge inside one model vendor.

Hangar is **embedded-first**: one Rust binary (or container) and one persistent volume. PostgreSQL, Redis, a vector database, a graph database, a queue, and object storage are not required.

> **Alpha notice:** suitable for local evaluation and controlled pilots. It is not multi-node, OIDC is not implemented yet, and it must not be the sole authority for high-impact decisions.

## Why Hangar

- **Portable context:** HTTP, gRPC, CLI, and MCP use the same core API.
- **Governed memory:** private sessions expire; durable memory requires explicit promotion and owner-controlled publication.
- **Evidence-first retrieval:** results preserve source, chunk ordinal, scores, model identity, provenance, and an untrusted-content label.
- **Isolation by design:** organization and workspace scope is enforced in records, indexes, queries, sharing, exports, and audit events.
- **Simple operations:** embedded storage, rebuildable projections, quotas, metrics, and verified offline backup/restore.

## Architecture

```text
MCP / CLI / HTTP / gRPC clients
              │
       thin protocol adapters
              │
 authorization · policies · lifecycle · audit
              │
 ingestion ─ context compiler ─ retrieval planner
              │
 redb metadata · file blobs · Tantivy · USearch · graph projection
```

Canonical records own scope, provenance, lifecycle, and policy. Text, vector, and graph indexes are rebuildable projections—not sources of authorization. Retrieved content is always **untrusted data**. Read the [reference architecture](docs/architecture/reference-architecture.md) and [security model](docs/security-model.md) for the underlying contract.

## Included in alpha

- Asynchronous document ingestion, idempotency, retry, dead letter, and content-addressed blobs.
- BM25, local vector, graph evidence, and calibrated hybrid retrieval.
- Offline `hashing-v1` plus optional verified `local-multilingual-v1`.
- Working memory, governed durable-memory lifecycle, context packages, and reviewed cross-workspace memory grants.
- Scoped API keys, deterministic guardrails, audit events, outbox, quotas, readiness, metrics, export, and verified backup/restore.
- Native HTTP/gRPC plus the `hangar` CLI and local stdio `hangar-mcp` adapter.

## Quick start

### Docker

```bash
docker build -t hangar-ai .
docker run --rm -p 8080:8080 \
  -e HANGAR_BOOTSTRAP_TOKEN=change-me \
  -v hangar-data:/var/lib/hangar \
  hangar-ai

curl http://127.0.0.1:8080/readyz
```

For a persistent local environment, backups, and Codex MCP, use [`deploy/local/`](deploy/local/). Copy `.env.example` to `.env`, choose a strong bootstrap token, and keep that file private.

### From source

Requires Rust 1.88. On Windows, install Visual Studio Build Tools with the Desktop development with C++ workload.

```bash
cargo run -p hangar-server -- --data-dir ./data --bootstrap-token change-me
```

HTTP listens on `127.0.0.1:8080`; gRPC defaults to `127.0.0.1:50051`.

## Use it

1. Bootstrap an organization with `POST /v1/organizations`.
2. Create a least-privilege, workspace-scoped API key.
3. Ingest documents with `POST /v1/documents`.
4. Retrieve cited chunks with `POST /v1/retrieve/documents` or compile bounded governed memory with `POST /v1/context-packages`.
5. Run `hangar-mcp` as a local stdio process when an MCP host needs the same governed context.

The [API guide](docs/api.md) has complete request contracts. MCP, CLI, and gRPC setup are in [integrations](docs/integrations.md).

### Optional local semantic profile

`hashing-v1` is deterministic and needs no download. To evaluate semantic retrieval, provision the local model once, verify its manifest, and start with `HANGAR_EMBEDDING_PROFILE=local-multilingual-v1`. Serving ingestion and queries never downloads model artifacts. See the [local deployment guide](deploy/local/README.md) and [embedding decision](docs/architecture/decisions/0011-optional-local-embedding-profile.md).

## Controlled benchmark

The repository includes a synthetic, versioned benchmark—not customer data. It compares BM25 with `hashing-v1`, semantic-only retrieval, and final hybrid ranking on the same 12 Portuguese queries.

| Hybrid local profile result | Value |
| --- | ---: |
| Recall@5 | 100% |
| MRR@10 | 1.00 |
| Citation precision@1 | 100% |
| Context sufficiency | 100% |
| Cross-workspace leakage | 0 |

These results demonstrate the controlled corpus only, not universal quality.

```bash
python hangar-ia-e2e/semantic_benchmark.py --help
```

See [evaluation methodology](docs/evaluation.md) and the [v1 readiness report](docs/v1-public-launch-readiness.md) for thresholds, results, and limitations.

## Project layout

```text
packages/hangar-server  Embedded API, ingestion, storage, retrieval, policy
packages/hangar-cli     Thin native HTTP command-line client
packages/hangar-mcp     Local stdio MCP adapter
deploy/local            Single-host Compose deployment and maintenance
hangar-ia-e2e           Synthetic acceptance and quality benchmarks
docs                     API, operations, security, architecture decisions
```

## Operations and safety

- The embedded deployment has one active writer; it is not replicated.
- Stop the server before an offline backup. Restore to a new location and verify it before use.
- Protect data volumes and backup media: they contain tenant knowledge.
- Treat every retrieved document, memory, and skill body as data—not policy, tool authority, or a system instruction.

See [operations](docs/operations.md), [memory lifecycle](docs/memory-lifecycle.md), and [governed sharing](docs/governed-sharing.md).

## Contributing and license

Hangar AI is licensed under [Apache-2.0](LICENSE). Read [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and [RELEASE.md](RELEASE.md) before opening a pull request. Contributors and AI agents should start with [AGENTS.md](AGENTS.md).

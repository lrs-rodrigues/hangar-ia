# Hangar AI

**An open, interoperable memory and knowledge platform for AI agents.**

Hangar AI gives people and agents a shared, governed knowledge layer across models, IDEs, CLIs, and agent runtimes. It reduces repeated context loading, preserves reliable organizational knowledge, and makes memory portable instead of locking it inside a single AI product.

It is **embedded-first**: the default deployment is one Rust binary (or one container) and one persistent volume—no PostgreSQL, Redis, vector database, graph database, queue, or object-storage service is required.

> Status: architecture and community foundation. Implementation starts after the decisions in [`docs/architecture/`](docs/architecture/) are accepted.

## What it will provide

- RAG over documents, code, and structured data.
- Graph-RAG for entity, relationship, provenance, and multi-hop retrieval.
- Session, durable, and shared organizational memory.
- Tenant-aware sharing among agents from different operators and platforms.
- Versioned skills and enforceable guardrails, with provenance and auditability.
- MCP, A2A, UTCP, and ACP adapters without coupling the core to any one protocol.
- An administrative console built with Next.js and shadcn/ui.

## Start here

Read [`AGENTS.md`](AGENTS.md), then the proposed architecture in [`docs/architecture/`](docs/architecture/). The structured maps in [`.agents/`](.agents/) let a human or AI contributor load only the guidance relevant to a task.

## Run the alpha server

The first vertical is available in `packages/hangar-server`: embedded `redb` memory records, content-addressed blobs, workspace-isolated lexical retrieval, API keys, and an HTTP API. It remains an alpha; configurable policies and OIDC are not implemented yet.

```bash
cargo run -p hangar-server -- --data-dir ./data --bootstrap-token change-me
curl http://127.0.0.1:8080/health
```

On Windows, install Rust plus **Visual Studio Build Tools** with the Desktop development with C++ workload so `link.exe` is available. Linux/macOS users need a standard C/C++ linker. The container image can be built without host Rust:

```bash
docker build -t hangar-ai .
docker run --rm -p 8080:8080 -e HANGAR_BOOTSTRAP_TOKEN=change-me -v hangar-data:/var/lib/hangar hangar-ai
```

See the [alpha API](docs/api.md) for requests.

## License and contributing

Hangar AI is licensed under [Apache-2.0](LICENSE). Please read [CONTRIBUTING.md](CONTRIBUTING.md), [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), and [SECURITY.md](SECURITY.md) before contributing.

Releases follow the protected-branch workflow in [RELEASE.md](RELEASE.md).

# Delivery roadmap

## Phase 0 — architecture acceptance

- Confirm product name, initial customer/persona, hosting model, and data-residency needs.
- Turn proposed ADRs into accepted/rejected decisions.
- Define SLOs and an evaluation corpus before selecting specialized stores.
- Produce native API/event schemas and a threat model.

## Phase 1 — governed memory core

- Organization/workspace identity, authorization, audit, embedded metadata, content-addressed file ingestion, and asynchronous pipeline.
- Working/durable memory lifecycle; hybrid RAG; evidence-rich context packages.
- Native HTTP/gRPC API, one MCP adapter, and CLI environment.
- Benchmark retrieval quality, latency, ingestion throughput, and token savings.

The v1 implementation sequence is tracked in `docs/ingestion-pipeline.md`: durable jobs come before heavyweight parsing, embeddings, and graph extraction.

## Phase 2 — collaboration and Graph-RAG

- Canonical events/outbox, entity/relation projection, graph retrieval planner, conflict/review workflow, and memory-sharing policies.
- Skills registry, deterministic guardrail engine, A2A and UTCP adapters.
- Dashboards, backups/restore, evaluation, and adversarial tests. The embedded
  baseline now includes verified offline backup/restore, operational probes,
  content-free metrics, workspace usage/exports, and write quotas; the admin
  console will consume those server contracts rather than replace them.

## Phase 3 — scale and federation

- Partitioning, replication, or external index/graph engines only if benchmarked need is proven.
- Regional deployment, federation contracts, enterprise identity, residency, HA runbooks, and community integrations.
- Reassess a narrow Raft coordination component only with a written SLO and failure model.

## V1 stabilization and release

`hangar-server` is completed and hardened before starting `hangar-admin`. The admin console is the final product vertical before the V1 release; it consumes stable server APIs and must not become a prerequisite for core-server operation.

Before declaring the server mature, validate it with a real, bounded knowledge corpus from an active AI-assisted development conversation. Start the server locally, ingest selected non-sensitive conversation artifacts, and retrieve them from Codex Desktop through the native API or a thin local adapter. If the desktop client cannot reach the local server directly, record the limitation and validate the same workflow through an equivalent local client. The acceptance criteria are scoped retrieval, useful citations, no cross-workspace leakage, and a measurable reduction in repeated context supplied to the model.

## Success measures

- Context reuse rate and avoided input tokens per workflow.
- Retrieval precision/recall and citation coverage on a versioned evaluation set.
- P95 context-assembly latency, ingestion freshness, and projection lag.
- Policy denial correctness, tenant-isolation tests, and audit completeness.
- Cost per indexed/retrieved unit and operator effort per deployment.

The current synthetic Docker baseline and its explicit limits are documented in
[`evaluation.md`](evaluation.md). Production SLOs require a named deployment
profile and controlled corpus before they become release gates.

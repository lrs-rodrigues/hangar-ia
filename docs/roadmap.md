# Delivery roadmap

## Phase 0 — architecture acceptance

- Confirm product name, initial customer/persona, hosting model, and data-residency needs.
- Turn proposed ADRs into accepted/rejected decisions.
- Define SLOs and an evaluation corpus before selecting specialized stores.
- Produce native API/event schemas and a threat model.

## Phase 1 — governed memory core

- Organization/workspace identity, authorization, audit, embedded metadata, content-addressed file ingestion, and asynchronous pipeline.
- Working/durable memory lifecycle; hybrid RAG; evidence-rich context packages.
- Native HTTP/gRPC API, one MCP adapter, CLI environment, and admin foundations.
- Benchmark retrieval quality, latency, ingestion throughput, and token savings.

## Phase 2 — collaboration and Graph-RAG

- Canonical events/outbox, entity/relation projection, graph retrieval planner, conflict/review workflow, and memory-sharing policies.
- Skills registry, deterministic guardrail engine, A2A and UTCP adapters.
- Dashboards, backups/restore, evaluation, and adversarial tests.

## Phase 3 — scale and federation

- Partitioning, replication, or external index/graph engines only if benchmarked need is proven.
- Regional deployment, federation contracts, enterprise identity, residency, HA runbooks, and community integrations.
- Reassess a narrow Raft coordination component only with a written SLO and failure model.

## Success measures

- Context reuse rate and avoided input tokens per workflow.
- Retrieval precision/recall and citation coverage on a versioned evaluation set.
- P95 context-assembly latency, ingestion freshness, and projection lag.
- Policy denial correctness, tenant-isolation tests, and audit completeness.
- Cost per indexed/retrieved unit and operator effort per deployment.

# Backend map

The first backend vertical lives in `packages/hangar-server`. Read `docs/api.md`, `docs/architecture/reference-architecture.md`, ADRs 0002, 0003, 0005, 0006, 0007, and 0008, and `docs/security-model.md` before changing it. Read `docs/memory-lifecycle.md`, `docs/skills-and-guardrails.md`, `docs/vector-retrieval.md`, `docs/text-retrieval.md`, `docs/graph-retrieval.md`, and `docs/ingestion-pipeline.md` for lifecycle, guardrail, retrieval, and worker changes.

Keep the canonical API independent from MCP, A2A, UTCP, and ACP. Generated protocol bindings belong at the edge, with contract tests against the core API. For native HTTP/gRPC, CLI, and MCP boundaries, read `docs/integrations.md` and the protobuf contract before changing a public route.

For probes, metrics, capacity limits, workspace export, or recovery commands,
read `docs/operations.md` and ADR 0009. Backups are offline verified snapshots;
never add a live directory copy as a recovery mechanism.

For governed sharing and context assembly, read `docs/governed-sharing.md` and
ADR 0006. Memory grants are canonical ACL records in `redb`, never copied
between workspaces. Context packages must preserve token budgets, citations and
the `untrusted` label.

# Backend map

The first backend vertical lives in `packages/hangar-server`. Read `docs/api.md`, `docs/architecture/reference-architecture.md`, ADRs 0002 and 0003, and `docs/security-model.md` before changing it.

Keep the canonical API independent from MCP, A2A, UTCP, and ACP. Generated protocol bindings belong at the edge, with contract tests against the core API.

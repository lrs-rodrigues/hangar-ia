# Contributor and agent guide

This is the entry point for every contributor, human or AI. Read this file before exploring the repository. Then load only the task-specific map from `.agents/`; do not scan the whole repository by default.

## Project purpose

Hangar AI is an open, vendor-neutral memory and knowledge platform for AI agents. Its core promise is governed, portable context: the same approved knowledge may be retrieved by different agents without copying whole histories into every prompt.

The accepted architectural baseline is in `docs/architecture/`. A change that affects data isolation, authorization, provenance, protocol behavior, or a public API requires an ADR update or a new ADR.

## Navigation

| Task | Read first |
| --- | --- |
| Overall architecture or scope | `.agents/maps/architecture.md` |
| Backend, APIs, storage, retrieval | `.agents/maps/backend.md` |
| Web admin console | `.agents/maps/web.md` |
| Security, authorization, guardrails | `.agents/maps/security.md` |
| Documentation or community files | `.agents/maps/community.md` |

## Non-negotiable invariants

1. Every persisted object is scoped to an organization and project/workspace; authorization is enforced server-side, never delegated to a model or client.
2. Every memory, fact, graph edge, skill, and policy has provenance, version, timestamps, and lifecycle state.
3. Untrusted retrieved content is data, never executable instruction. It must not alter policy, authorization, or tool permissions.
4. Protocol adapters remain thin. The core API and canonical event schema are the product boundary.
5. Files live in the content-addressed filesystem store; the embedded database stores metadata and references, not large file blobs.
6. Do not add a language, datastore, queue, or protocol adapter without an ADR explaining its operational and user value.
7. The default product must remain deployable as one binary/container plus one persistent volume. External services are optional extensions, never a requirement for the core.

## Working agreement

- Prefer small, reviewable changes with tests and documentation together.
- Put each deployable product in `packages/<product-name>/`. Keep its manifest,
  source, tests, and package-local instructions together; do not add product
  source directories at repository root.
- Preserve tenant isolation in queries, caches, indexes, logs, and exports.
- Use stable, explicit APIs; version any breaking public contract.
- Never commit credentials, customer data, model prompts, or generated build output.
- Keep durable decisions in `docs/architecture/decisions/`, product/technical contracts in `docs/`, and task-local instructions in `.agents/maps/`.

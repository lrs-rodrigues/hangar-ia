# Admin console

The initial console will live in `packages/hangar-admin` as a Next.js + TypeScript application using shadcn/ui. It calls the same public management API as automation; it does not directly query data stores or implement policy locally.

## First release screens

1. Organization, workspace, environment, members, service accounts, and roles.
2. Knowledge sources, ingestion status, file lifecycle, and reprocessing.
3. Memory explorer: evidence, scope, provenance, confidence, lifecycle, lineage, and delete/expiry actions.
4. Graph explorer: entity/edge evidence and bounded traversal trace.
5. Skills catalog: publisher, version, permissions, approval, and revocation.
6. Guardrail/policy editor: version, diff, simulator, rollout, and audit.
7. Agent/client registry: credentials, allowed protocols, quotas, and sessions.
8. Operations: retrieval quality, latency, spend/token savings, queue health, audit search, and OpenTelemetry trace links.

Every high-impact action shows scope, policy outcome, and reversibility. Raw retrieved content is visibly separated from system policy. Destructive actions support retention/legal-hold checks and an audit reason.

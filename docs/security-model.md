# Security and governance model

## Trust boundaries

The model, agent, tool output, retrieved document, connector, and protocol adapter are separate trust boundaries. None may grant itself data access or change a policy. The core service verifies identity, tenant scope, authorization, and policy before every read, write, share, tool execution request, and export.

## Required controls

- OIDC workload/user identity; short-lived credentials and service accounts.
- RBAC plus attributes/labels for organization, workspace, data classification, purpose, environment, and agent trust level.
- Policy-as-code with versioned bundles, dry runs, decisions, and immutable audit records. The first evaluator must be deterministic and server-side.
- Encryption in transit and at rest; tenant-scoped keys where required.
- Provenance and content hashes for every ingestion, extraction, embedding, and memory mutation.
- Sensitive-data classification, retention/deletion workflows, legal hold, and export controls.
- Signed skill packages, publisher identity, dependency/SBOM metadata, capability manifest, version pinning, and revocation.
- Limits for token/context budget, retrieval depth, graph hops, rate, concurrency, spend, and tool authority.
- Audit logs for context reads, policy decisions, memory changes, skill use, and connector actions; telemetry redacts content by default.

## Memory poisoning and prompt injection

Ingested content can contain adversarial instructions. It is stored and rendered as untrusted data, separated from system instructions. Automated extraction produces proposals, not automatically organization-wide truth. Promotion rules can require source allowlists, confidence thresholds, corroboration, reviewer approval, or time-based expiry. Retrieval responses label untrusted content and include sources.

## Guardrails are layered

1. **Deterministic gates:** authorization, schema validation, destination/tool allowlists, secret/PII controls, and budgets.
2. **Policy decisions:** tenant rules controlling who may retrieve, write, promote, share, export, or invoke a skill.
3. **Model-assisted classifiers:** optional risk/content classification; useful signals, never the only authorization control.
4. **Detection and response:** tracing, anomaly alerts, revocation, quarantine, rollback, and forensics.

This covers the key OWASP LLM/MCP risks: prompt injection, insecure memory references, excessive agency, sensitive-data disclosure, and supply-chain exposure.

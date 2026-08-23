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
- Server-enforced limits for durable memory, document/blob bytes, token/context
  budget, retrieval depth, graph hops, rate, concurrency, spend, and tool
  authority.
- Audit logs for context reads, policy decisions, memory changes, skill use, and connector actions; telemetry redacts content by default.
- Shared-memory grants are same-organization, source-workspace ACL records with
  explicit review/revocation. A context read re-evaluates both the grant and
  the source memory lifecycle/expiry; it never trusts a copied target record.

## Memory poisoning and prompt injection

Ingested content can contain adversarial instructions. It is stored and rendered as untrusted data, separated from system instructions. Automated extraction produces proposals, not automatically organization-wide truth. Promotion rules can require source allowlists, confidence thresholds, corroboration, reviewer approval, or time-based expiry. Retrieval responses label untrusted content and include sources.

## Guardrails are layered

1. **Deterministic gates:** authorization, schema validation, destination/tool allowlists, secret/PII controls, and budgets.
2. **Policy decisions:** tenant rules controlling who may retrieve, write, promote, share, export, or invoke a skill.
3. **Model-assisted classifiers:** optional risk/content classification; useful signals, never the only authorization control.
4. **Detection and response:** tracing, anomaly alerts, revocation, quarantine, rollback, and forensics.

This covers the key OWASP LLM/MCP risks: prompt injection, insecure memory references, excessive agency, sensitive-data disclosure, and supply-chain exposure.

## Current deterministic evaluator

The embedded server now enforces a small, versioned policy evaluator for
memory/context reads, skill reads/uses, and tool-invocation preflight. It runs
after tenant/RBAC authentication and before a protected response is exposed.
Rules are server-owned data created through authenticated catalog APIs; a
matching deny overrides an allow, and both outcomes are audited. No document,
memory, tool output, or skill body is parsed as a policy or authorization
directive. See `docs/skills-and-guardrails.md` and ADR 0008 for its exact
contract and intentional limits.

## Operational data handling

Backups and mounted data directories contain tenant knowledge and must be
protected as sensitive material. Hangar's embedded backup command is offline,
checksum-verified, and restores only into a new directory so it cannot silently
overwrite an active deployment. Workspace export is separately authorized by
owner RBAC plus the deterministic `export` guardrail, scoped to one workspace,
audited, and never includes API keys or audit history. Metrics redact content,
queries, principals, and tenant labels by default. See [`operations.md`](operations.md).

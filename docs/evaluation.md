# Evaluation and adversarial regression suite

Hangar evaluates retrieval as a governed context product, not only as a search
engine. Every release candidate should measure:

- **Quality:** expected evidence is retrieved, cited, scoped to the caller, and
  labelled untrusted.
- **Latency:** report median and P95 ingest-to-retrieval and context-assembly
  latency on a named environment; do not compare different machines as an SLO.
- **Cost avoidance:** compare estimated tokens in the complete source corpus
  with the token-bounded context package returned for the task.
- **Safety:** exercise cross-workspace access, prompt-injection content,
  enforced deny rules, malformed input, and quota exhaustion.
- **Recovery:** create, verify, restore, and start an isolated copy of a real
  test volume. A snapshot is not accepted only because its copy command exited
  successfully.

## Baseline corpus

[`hangar-ia-e2e/`](../hangar-ia-e2e/) contains a small, versioned synthetic
development conversation. It intentionally includes an adversarial instruction
and tests the same native HTTP contract used by the CLI and MCP adapter. Run it
with Docker Compose:

```bash
docker build -t hangar-server-operational-test .
docker compose -f hangar-ia-e2e/compose.yaml up --abort-on-container-exit --exit-code-from e2e
docker compose -f hangar-ia-e2e/compose.yaml down -v
```

The JSON result records document-citation count, a small retrieval P95 sample,
corpus/context token estimates, avoided input tokens, tenant-isolation status,
untrusted-content status, and a guardrail-denial assertion. It deliberately
does not claim a general quality score or production latency SLO.

## Adding a corpus

Keep fixtures small, synthetic, and reviewable. Never commit real conversations,
credentials, customer records, or proprietary source. Each query should state
its expected source/citation and expected access decision. Add a regression when
an incident identifies a missed evidence result, unsafe policy interaction, or
performance cliff.

For production retrieval quality, maintain a separate controlled evaluation
dataset with data owners and retention policy. Record embedding provider/model
revision, pipeline version, hardware, corpus version, token estimator, and the
pass/fail threshold with every benchmark result.

## Semantic-quality release gate

The offline `hashing-v1` profile is a functional baseline, not a semantic
quality baseline. A semantic-quality report must compare all three retrieval
plans against the same versioned query set:

1. BM25 plus `hashing-v1` (the compatibility baseline);
2. the selected semantic embedding provider by itself; and
3. Hangar's final hybrid ranking.

Every query names one or more accepted evidence chunks, its expected access
decision, and whether a returned context package is sufficient to complete the
task without supplying the source corpus. Reports record Recall@5, Recall@10,
MRR@10, nDCG@10, citation precision, context sufficiency, P50/P95 ingestion,
retrieval and context-assembly latency, and cost per indexed document and per
query. They also rerun tenant isolation, prompt-injection, and enforced-policy
denial tests with the semantic provider selected.

The initial quality gates are:

| Measure | Initial gate |
| --- | ---: |
| Recall@5 | 90% or higher |
| MRR@10 | 0.80 or higher |
| Citation precision | 95% or higher |
| Cross-workspace leakage | 0 |
| Context package | Within requested token budget |

These are release gates for a named corpus and deployment profile, not a claim
that one score generalizes to every customer domain.

## Deployment profiles

The same corpus and judgments run in both profiles.

- **Solo:** explicit one-time local model installation, no required external
  service, and a practical CPU/RAM/disk budget recorded alongside latency.
- **Enterprise:** a controlled, retention-governed corpus; model artifact
  provenance/checksum; data-residency record; auditable access decisions; and
  cost and latency thresholds agreed for the named deployment.

Neither profile permits a model artifact to download during ingestion or query
execution. The benchmark records the embedding provider, model revision,
artifact checksum and corpus revision so an apparent quality change remains
reproducible.

## Executable controlled benchmark

[`hangar-ia-e2e/semantic_benchmark.py`](../hangar-ia-e2e/semantic_benchmark.py)
is the repository's small, synthetic release-gate runner. It runs the native
HTTP path once per profile, writes machine-readable reports, then compares the
BM25 + `hashing-v1` baseline, semantic-only ranking, and final hybrid ranking.
Its `compare` command exits non-zero for a `NO-GO`; this makes a quality gate
usable in CI without treating a passing smoke test as a production claim. See
the harness README for commands and
[`v1-public-launch-readiness.md`](v1-public-launch-readiness.md) for the
current recorded decision.

The controlled runner also verifies the memory boundary: private working
session ownership and TTL disposal; explicit promotion into a `proposed`
durable memory; preserved session-entry provenance; and retrieval only after
owner validation and publication.

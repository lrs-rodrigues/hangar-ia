# ADR 0011: Optional, verified local embedding profile

**Status:** Accepted

## Decision

Keep `hashing-v1` as the zero-download compatibility baseline and add an
optional `local-multilingual-v1` embedding profile for semantic-quality
evaluation and deployments that choose it. The initial local profile uses a
version-pinned, ONNX-compatible multilingual MiniLM artifact and executes
inside the Hangar server process; it does not send document or query content to
an external service.

Model weights are never included in the base image and are never downloaded on
the ingestion or retrieval path. Development installation is an explicit
operator command that downloads a pinned artifact into the persistent volume.
Production startup accepts only an already provisioned model directory and a
manifest containing the provider ID, model revision, dimensions and SHA-256
checksums. A missing or mismatched artifact fails startup clearly.

Every vector manifest and USearch generation includes the embedding provider,
model revision and dimensions. Selecting a different profile or revision
requires a controlled rebuild from canonical chunks; generations from distinct
models are never mixed.

## Why

The built-in hashing vector proves indexing and recovery but does not measure
semantic retrieval quality. A local, multilingual profile provides a credible
privacy-preserving evaluation path for Portuguese and multilingual knowledge
without making model weights, network access or an external inference service a
requirement for the default deployment.

## Consequences

- Developers can opt into one explicit installation command rather than manage
  tokenizer and ONNX artifacts manually.
- Enterprise operators can mirror, review, checksum and provision the exact
  same artifact in offline or regulated environments.
- The server has a provider-neutral vector contract; future remote providers
  use the same manifest, audit and reindexing boundaries.
- Semantic quality claims require a controlled benchmark that records corpus,
  profile revision, hardware and pass/fail thresholds.

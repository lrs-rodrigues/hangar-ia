# Hangar documentation

This directory is the versioned source of truth for Hangar's public technical
documentation. Keep it with the code so API, operational, and security changes
are reviewed in the same pull request that changes behavior.

It can be mirrored to a repository Wiki for easier browsing, but the Wiki
should be treated as a published copy rather than an independent source.

## Start here

- [API](api.md) — HTTP contracts, lifecycle, retrieval, sessions, and policies.
- [Integrations](integrations.md) — CLI, MCP, and gRPC boundaries.
- [Operations](operations.md) — deployment, limits, backup, restore, and export.
- [Security model](security-model.md) — authorization and trust boundaries.
- [Evaluation](evaluation.md) — synthetic quality and adversarial regression.
- [Roadmap](roadmap.md) — current product direction.

## Architecture

- [Vision](architecture/vision.md)
- [Reference architecture](architecture/reference-architecture.md)
- [Architecture decisions](architecture/decisions/)

## Product guides

- [Ingestion pipeline](ingestion-pipeline.md)
- [Text retrieval](text-retrieval.md)
- [Vector retrieval](vector-retrieval.md)
- [Graph retrieval](graph-retrieval.md)
- [Memory lifecycle](memory-lifecycle.md)
- [Governed sharing](governed-sharing.md)
- [Skills and guardrails](skills-and-guardrails.md)

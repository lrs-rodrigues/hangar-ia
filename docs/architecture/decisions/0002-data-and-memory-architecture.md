# ADR 0002: Embedded-first storage with rebuildable projections

**Status:** Proposed

## Decision

Ship a self-contained Rust service with one mounted persistent data directory. Use redb as the canonical embedded store for metadata, authorization, lifecycle, outbox, and graph records; use a content-addressed local filesystem for blobs; Tantivy for full-text search; USearch for ANN vector search; and a bounded in-process store for unpromoted session memory. Do not require PostgreSQL, Redis, S3/MinIO, a vector database, or a graph database.

## Why

This minimizes adoption and operating cost for solo developers while preserving a credible scale path for enterprises. The product owns the data model, storage layout, backup, and API, but does not reimplement durable B-trees, full-text indexing, or ANN algorithms. Embeddings, lexical indexes, and extracted graph relations are derived artifacts: they need versioning and rebuilds, but not independent truth ownership.

## Consequences

- A single process owns each data directory; local filesystem locking prevents unsafe multi-process access.
- Outbox consumers must be idempotent and support replay.
- Every projection is tenant-scoped and supports deletion/rebuild.
- Graph-RAG is evidence-aware: edges carry source IDs, confidence, extractor, version, timestamps, and review state.
- The graph is application-owned adjacency data with bounded traversals, not a vendor query language.
- A distributed deployment is a separate profile: partitioning, replication, and consensus are introduced only with measured SLOs and an explicit ADR.

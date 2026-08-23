# Graph-RAG and canonical outbox

This vertical adds the embedded graph path without a graph database, queue, or
model-provider credential. It is complete locally and is pending the next
repository commit/release. The Docker build validates formatting, 19 unit
tests, a release build, and the HTTP acceptance flow.

## Graph contract

`redb` holds an application-owned, workspace-scoped adjacency graph:

- `entities` stores deterministic keyword entities with extractor/version provenance.
- `graph_edges`, `edges_by_source`, and `edges_by_target` store a bounded
  `co_occurs_with` relationship.
- `edge_evidence` links every relationship to its source chunk and content hash.
- `graph_manifests` records the source chunk, extractor version, pipeline
  version, and `pending`/`ready` visibility state.

The initial `deterministic-keyword-v1` extractor deliberately produces
proposals, not facts. It selects at most eight non-stopword lexical terms per
chunk and records co-occurrence with confidence `0.5`. A future model extractor
must use a new named version and preserve this manifest/review boundary.

Traversal is bounded server-side: at most three hops, 128 traversed edges, and
the caller result limit. Document retrieval fuses BM25, vector, and graph
scores, returning graph score and hop count with its normal source citation.

## Publication and recovery

During ingestion, chunks and vector/text/graph manifests first commit as
`pending`. Vector and text replacement generations are then published. In the
final `redb` transaction, graph entities/edges/evidence are materialized, every
manifest becomes `ready`, and the document/job becomes `succeeded`.

Graph records never grant access by themselves. Every result rechecks
organization, workspace, succeeded document, manifest readiness, pipeline
version, and source evidence. A crash may leave pending manifests or derived
graph records, but no partial document is retrievable. Startup and an
owner-authorized rebuild recreate the graph from canonical ready chunk records.

## Canonical outbox

`outbox_events` is a durable, transactionally appended event feed in `redb`.
Every event has an immutable UUIDv7 ID, `spec_version`, type, subject,
organization/workspace scope, metadata-only data payload, and time. The first
families are `memory.proposed.v1`, `memory.lifecycle_changed.v1`,
`document.ingestion_queued.v1`, `document.ingestion_succeeded.v1`,
`document.ingestion_failed.v1`, and `graph.projection_ready.v1`.

An owner reads a scoped feed with an exclusive `after` cursor. The server never
acknowledges or deletes events for a consumer: consumers persist their cursor
and deduplicate by event ID, making replay safe and keeping delivery adapters
outside the core boundary.

## Acceptance criteria

- A graph result has an edge, source chunk, document citation, extractor, and confidence provenance.
- Graph, hybrid retrieval, and outbox reads remain organization/workspace isolated.
- A pending graph manifest or non-succeeded document is never returned.
- Rebuild and startup reconciliation use only ready canonical chunks.
- Outbox writes occur in the same `redb` transaction as their domain mutation; replay is ordered and cursor-based.

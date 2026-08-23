# Tantivy/BM25 lexical retrieval vertical

This vertical replaces the prototype term-count score with an embedded Tantivy
BM25 projection. It extends the existing hybrid retrieval contract without
adding a service, daemon, or required external configuration.

**Implementation status:** complete locally, pending the next repository
commit/release. The canonical Docker build verifies formatting, 16 unit tests,
and a release build. The container smoke test verifies BM25 ranking, hybrid
retrieval, and both owner-authorized rebuild operations.

## Deliverable

1. A Tantivy text index under `text/`, isolated by organization and workspace.
2. Canonical `redb` manifests for chunk text projections: scope, source hash,
   pipeline version, lifecycle state, and active index generation.
3. BM25 candidates in `POST /v1/retrieve/documents`, fused deterministically
   with the existing vector candidates while retaining the citation fields.
4. An owner-authorized text-index rebuild API and startup reconciliation.
5. Audit events for text projection, rebuild, reconciliation, and retrieval.

## Publication protocol

Tantivy directories and `redb` cannot share an ACID transaction. A document is
therefore published through this recoverable sequence:

1. Persist its chunks and text/vector manifests as `pending` in `redb`.
2. Build complete replacement text and vector generations from canonical ready
   entries plus the current pending document.
3. Commit both index generations to sibling temporary locations and publish
   their final locations atomically.
4. In one `redb` transaction, mark both sets of manifests ready, set the text
   generation active, and mark the document/job succeeded.

Retrieval always validates the document state, scope, manifest readiness, and
active text generation. A crash before step 4 can leave files behind, but no
partial document is eligible for lexical, vector, or hybrid retrieval.

Startup removes temporary and unreferenced text generations, then rebuilds
every workspace represented by a text manifest from canonical ready records.

## Acceptance criteria

- BM25 ranks an exact lexical match above unrelated content.
- A text generation survives a restart and is reconstructed after deletion.
- Pending text data and unpublished generations are not searchable.
- Workspaces cannot retrieve one another's text candidates.
- Text and vector candidates fuse under the existing stable citation response.
- Docker validates formatting, unit tests, release build, and an HTTP smoke
  test that exercises ingestion, hybrid retrieval, and both rebuild endpoints.

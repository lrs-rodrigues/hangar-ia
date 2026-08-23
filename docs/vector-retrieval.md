# Hybrid vector retrieval vertical

This vertical turns the current document retrieval contract into an embedded,
rebuildable hybrid RAG path. It is complete only when every item below is
implemented and Docker-validated together.

**Implementation status:** complete locally, pending the next repository
commit/release. The canonical Docker build runs formatting, the 13 unit tests,
and a release build. The container smoke test covers bootstrap, asynchronous
ingestion, hybrid citations, and an owner-authorized rebuild.

## Deliverable

1. A versioned embedding-provider interface, with the offline `hashing-v1`
   implementation and its declared dimensions.
2. Canonical `redb` embedding metadata for every succeeded document chunk:
   source hash, provider/model revision, dimensions, pipeline version,
   workspace, and stable ANN key.
3. A separate USearch index file per organization/workspace/provider revision
   under the mounted `vectors/` directory. The index remains a rebuildable
   projection, never the source of authorization or truth.
4. The ingestion worker produces vectors after chunks and makes the document
   retrievable only when its lexical and vector projections are atomically
   recorded as ready.
5. `POST /v1/retrieve/documents` performs lexical and vector candidate search,
   applies scope filters before ranking, normalizes BM25 within the candidate
   set before fusing it with bounded semantic similarity deterministically, and
   returns citations with lexical score, vector score, final score, provider,
   and model revision.
6. A rebuild operation marks a projection rebuilding, recreates it only from
   canonical records, and does not expose partial or mixed-model results.
7. Audit events for vector projection, rebuild start/success/failure, and
   hybrid retrieval. Error responses never reveal another workspace's metadata.

## Execution order

The work is implemented and reviewed as one vertical, in this dependency order:

1. Validate the USearch wrapper, on-disk atomic publishing, deterministic
   offline provider, and Docker toolchain.
2. Add canonical vector manifests and collision-safe chunk-to-ANN-key mappings.
3. Make ingestion write the vector projection before publishing the canonical
   `succeeded` state; a crash may leave orphan index entries, but never a
   retrievable partial document.
4. Implement hybrid candidate fusion and citation/tracing fields in the native
   retrieval response.
5. Add rebuild and recovery paths from canonical manifests, then the complete
   Docker unit and HTTP acceptance suite.

No item in this list is independently declared complete: the vertical closes
only after step 5 passes with the prior items together.

## Reliability acceptance criteria

- A power-loss simulation before manifest readiness never returns the affected
  chunk through vector or hybrid retrieval.
- A power-loss simulation after file publication is repaired on next startup,
  without manual filesystem edits.
- Temporary projection files are never opened for retrieval and are removed by
  reconciliation.
- Rebuild uses only canonical ready manifests, produces a new generation, and
  leaves the prior generation usable until the atomic replacement succeeds.
- Reconciliation, cleanup, rebuild start/success/failure, and rejected stale
  entries emit audit events and safe operational counters.

## Acceptance tests

- A semantically related query ranks an expected chunk through the selected
  provider; a lexical-only query continues to work.
- A vector index survives a server restart, and an intentionally removed index
  is rebuilt from canonical records.
- Different workspaces cannot retrieve each other's vectors, even if their
  content and ANN keys would otherwise collide.
- Changing provider/model revision creates a separate projection rather than
  mixing vectors.
- Ingestion failure or rebuild failure leaves no partially searchable vectors.
- Docker tests cover the above, plus a HTTP smoke test that returns hybrid
  citations.

## Explicitly outside this vertical

External embedding-provider credentials, bundled transformer inference, and
protocol adapters. Tantivy/BM25 and Graph-RAG now consume this stable vector
contract without changing it.

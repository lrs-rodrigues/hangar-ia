# ADR 0005: Pluggable embeddings with a local deterministic baseline

**Status:** Proposed

## Decision

Keep embedding generation behind a server-owned provider boundary. The default
embedded profile ships with `hashing-v1`, a deterministic, no-network baseline
used for functional hybrid retrieval, tests, offline development, and index
rebuilds. It is explicitly not presented as a high-quality semantic model.

Production-quality embeddings are added as named, versioned providers. A
provider may be an in-process model package or an explicitly configured remote
endpoint; neither is required to run the server. Every vector records the
provider ID, model revision, dimensions, source content hash, and pipeline
version.

USearch owns only the per-workspace ANN projection on disk. Canonical redb
records own vector metadata and source-to-index-key mappings, so an index can
be deleted and rebuilt without data loss.

## Why

A bundled general-purpose model would make the default image substantially
larger, add model-download and hardware concerns, and force every open-source
user to operate an inference stack. A client-selected remote provider as the
only option would make the core dependent on credentials, network access, and
per-token cost. The baseline preserves a useful zero-config path while the
versioned boundary avoids baking a provider into the storage model.

## Consequences

- Hybrid ranking reports which embedding provider/model participated.
- Vectors from different model revisions are never mixed in one projection.
- Tenant filtering happens before results are returned; a workspace gets an
  independent on-disk ANN index.
- Re-embedding is a durable, observable pipeline job, not a synchronous API
  side effect.
- `hashing-v1` is a compatibility baseline only; quality evaluation must be
  run before enabling it for a production semantic-retrieval claim.

## Projection reliability protocol

The server cannot make one ACID transaction span `redb` and a USearch file.
It therefore uses a recoverable publication protocol:

1. Persist a canonical vector manifest with `pending` state before projection.
2. Build a replacement index file from canonical ready manifests, write it to
   a temporary sibling path, fsync it, and atomically rename it into place.
3. In one `redb` transaction, mark the manifest and document projection
   `ready` only after the replacement file is published.
4. Retrieval requires both a ready manifest and a succeeded document in the
   caller's workspace; an index entry alone is never sufficient.
5. Startup reconciliation removes stale temporary files and rebuilds every
   workspace represented by a manifest, including a workspace left with only
   pending records. Rebuilds are audited and observable.

A crash may leave an unreferenced index generation or a pending manifest. Both
are safe, bounded, and repaired by reconciliation; neither becomes retrievable
context.

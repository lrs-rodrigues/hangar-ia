# ADR 0009: Offline verified recovery and server-enforced embedded quotas

**Status:** Accepted

## Decision

Keep the default profile as one process and one persistent volume. Backups and
restores are explicit, offline `hangar-server` commands: a backup validates the
canonical redb database, copies the complete data tree into a temporary sibling
directory, writes SHA-256 checksums for every file, and atomically publishes
the snapshot. Verification validates every checksum and opens the copied redb
database. Restore accepts only a verified snapshot and writes only to a new,
non-existent destination.

The HTTP service exposes unauthenticated liveness (`/health`), readiness
(`/readyz`), and Prometheus text metrics (`/metrics`). Owners can inspect
workspace usage and export that workspace's canonical data. Exports are
guardrail-evaluated, audited, tenant-scoped, and mark all returned content as
untrusted data.

The server owns configurable per-workspace limits for durable memories,
documents/ingestion bytes, and blobs. Checks happen inside the store lock
immediately before mutation; idempotent document replays remain accepted.

## Why

Copying a live mounted directory cannot promise a cross-file-consistent view
of redb, content-addressed blobs, and rebuildable projections. An explicit
offline boundary gives solo users a dependable recovery path without an object
store, database cluster, or new daemon. Checksum verification makes a backup a
testable artifact rather than a hopeful filesystem copy.

Resource limits must be core-enforced because adapters and clients cannot be
trusted to protect a shared deployment. Applying them only at an HTTP edge
would let gRPC or future adapters bypass the same controls.

## Consequences

- The service must be stopped before `backup`, `verify-backup`, or `restore`.
  A running process owns the redb data file, so backup validation fails rather
  than publishing an unsafe snapshot.
- Backups include canonical state, blobs, and rebuildable projections. A
  restore validates the canonical database before it becomes active; normal
  startup reconciliation repairs projections from canonical records.
- Export is not a backup and does not include API-key hashes, audit logs,
  blobs, or cross-workspace data. It is a portable, owner-authorized workspace
  record for review or migration.
- `/metrics` never includes document, memory, query, identity, or workspace
  labels. Detailed usage remains owner-authorized to avoid tenant leakage and
  high-cardinality telemetry.
- The embedded limits are deployment-wide defaults. Per-plan billing and
  distributed rate limiting require a later control-plane design and ADR.

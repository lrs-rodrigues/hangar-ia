# ADR 0003: Do not implement Raft in the initial product

**Status:** Proposed

## Decision

Use one Hangar process and one persistent data directory for the default deployment profile. Do not build or embed a Raft cluster in the application. Revisit Raft only for a distributed profile with a narrow need such as membership, shard placement, or replication coordination that cannot be met by the selected deployment platform.

## Assessment

Raft is an excellent fit for replicated, strongly consistent state with a clear leader and a small, stable voting group. It is not a performance solution for high-volume embeddings, file uploads, retrieval traces, or append-only events; forcing those writes through a consensus log increases latency and operational cost. Mature databases already use Raft or equivalent replication internally where appropriate.

If Hangar later needs a dedicated coordination plane, prefer a proven component such as etcd or a mature library with explicit operational ownership. A multi-region deployment must document write-locality and failover semantics before enabling cross-region strong consistency.

## Consequences

- The default profile prioritizes simple backup/restore and optional warm standby over automatic multi-node failover.
- We defer custom shard membership, leader election, and consensus recovery.
- Stateless APIs/workers scale horizontally; storage uses its own replication and partitioning model.

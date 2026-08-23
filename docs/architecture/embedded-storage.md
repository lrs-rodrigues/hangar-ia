# Embedded storage profile

The default Hangar deployment is `hangar-server` plus a mounted data directory. It must work offline and require no managed service.

```text
/var/lib/hangar/
├── canonical.redb/  # memory, ACLs, policies, audit, graph records, manifests
├── blobs/           # immutable content addressed by SHA-256
├── text/            # Tantivy indices, rebuildable
├── vectors/         # USearch indices, rebuildable per embedding model
├── wal/             # durable application event segments
├── dead-letter/     # failed asynchronous work, inspectable and replayable
└── snapshots/       # consistent local backup artifacts
```

## Ownership rules

- `redb` is the canonical store. It holds identifiers and metadata, never original large files.
- `blobs/` is immutable after a successful write. Manifests reference a content hash, size, MIME type, and retention state.
- Text/vector indexes and graph traversal materializations are disposable projections. A rebuild reads canonical events and manifests; an index never becomes a source of truth.
- The server is the sole writer to its directory. Do not mount the same directory read-write into multiple Hangar processes or shared network filesystems.

## Graph representation

Graph data is modelled in redb tables: `entities`, `edges_by_source`, `edges_by_target`, and `edge_evidence`. Every edge has tenant scope, edge type, source evidence, confidence, extraction/policy version, and lifecycle. Traversal is bounded by hop count, candidate count, authorization filter, and request budget.

## Scale path

The embedded profile scales vertically first. The first distributed profile partitions complete workspaces to nodes, preserving local reads/writes for each partition. Replication, routing, backup shipping, and Raft are later capabilities—not requirements of the single-node install.

## Benchmark gate

Before declaring a storage component production-ready, benchmark realistic document sizes, embedding dimensions, tenant filters, concurrent reads/writes, recovery, index rebuild, backup/restore, and deletion. Measure P50/P95/P99 latency, throughput, recall@k, disk amplification, memory use, and recovery point/time objectives.

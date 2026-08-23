# Durable ingestion pipeline

The v1 ingestion API must acknowledge durable acceptance, not completion. A document becomes searchable only after its job succeeds.

## State machine

```text
queued → processing → succeeded
   │          │
   └────────→ retry_wait → processing
                         │
                         └→ dead_letter
```

Each job is tenant/workspace scoped and records input hash, pipeline version, attempt count, timestamps, safe last-error metadata, retry schedule, lease expiry, and an optional idempotency key. Jobs and source payload manifests are canonical `redb` records. Generated chunks, embeddings, extracted entities, and indexes are rebuildable outputs.

## API contract

- `POST /v1/documents` accepts a text document and returns `202 Accepted` with document and ingestion-job records. An optional `Idempotency-Key` header makes repeated requests return the original receipt; equal content in the same workspace and pipeline version is also deduplicated.
- `GET /v1/ingestion/jobs/{id}` returns authorized status and safe failure metadata.
- `POST /v1/ingestion/jobs/{id}/retry` moves a dead-letter job back to `queued`; it requires an owner key and creates an audit event.
- Retrieval includes only documents whose current ingestion job has `succeeded`.

## Worker rules

The single-node worker leases one queued job at a time from the embedded store, changes it to `processing`, then commits either result metadata and `succeeded`, or a bounded retry/dead-letter outcome. It is idempotent by input hash and pipeline version. On process start, an uncompleted lease is returned to `queued`. CPU work occurs outside the storage transaction; chunks, document status, job status, and the success audit event commit together.

## First v1 processors

1. Validate nonempty text input and identity scope.
2. Normalize paragraph-aware text and chunk deterministically.
3. Persist chunks plus lexical, vector, and graph projection manifests atomically.

The embedded `hashing-v1` vector path, Tantivy BM25 path, and deterministic entity/relation proposal path consume this same durable job contract. They must not make partially processed content retrievable; see [`vector-retrieval.md`](vector-retrieval.md), [`text-retrieval.md`](text-retrieval.md), and [`graph-retrieval.md`](graph-retrieval.md).

Failed work is observable and recoverable; it must not silently disappear or make partial output retrievable.

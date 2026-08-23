# ADR 0004: Build the embedded server in Rust

**Status:** Proposed

## Decision

Build the initial core API, workers, storage capabilities, and protocol adapters in Rust. Use TypeScript/Next.js for the admin console. Do not split the first release between Rust and Go.

## Rationale

The first product value is a compact, self-contained storage server. Rust enables safe in-process use of redb, Tantivy, and vector-index libraries without a second daemon or a C++ service boundary. A single server language reduces onboarding and AI-assisted maintenance cost in an open project.

## Consequences

API contracts and benchmarks define any future Go or external-service boundary. Retrieval ranking, ingestion orchestration, and policy evaluation must be profiled before adding a second runtime or service.

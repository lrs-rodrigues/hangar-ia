# Product vision and scope

## Problem

Teams use multiple AI products, models, and agent frameworks. Each keeps separate transient context, forcing people and agents to rediscover facts, reload large documents, and lose decisions when a session ends. The result is cost, inconsistency, and unsafe informal knowledge sharing.

## Product thesis

Hangar AI is not another model runtime or general-purpose database. It is a governed context substrate: it ingests company knowledge, turns it into retrievable and attributable memory, applies policy, and delivers a compact task-specific context package to any compatible agent client.

## Product principles

1. **Portable context, not portable prompts.** Store evidence, facts, relationships, summaries, and policies; compile a fresh bounded context per request.
2. **Evidence before assertion.** A memory retains source references, extraction method, confidence, and review state.
3. **Least context.** Retrieval is authorized, task-scoped, ranked, and token budgeted.
4. **Human and policy control.** Models can propose memories; policy decides whether they become shared knowledge.
5. **Interoperability at the edge.** Core semantics outlive protocols and model vendors.
6. **Operationally boring first.** Begin with proven stores and a modular monolith; earn distributed complexity through measured need.

## Actors and scopes

`Organization → Workspace/Project → Environment → Agent identity → Session` is the isolation hierarchy. A memory can be private to a session, shared with a team, or published to an organization knowledge space. Cross-organization sharing is explicit federation, never an accidental query option.

## In scope

- RAG, Graph-RAG, short/long/shared memory, skills, guardrails, and multi-agent access.
- MCP, A2A, UTCP, and ACP adapters where their semantics fit.
- Self-hosted and managed deployment paths, plus an admin console.

## Out of scope for the first releases

- Training or hosting foundation models.
- Autonomous agent orchestration/scheduling as a primary product.
- A custom distributed database, vector database, or consensus implementation.
- Unbounded global memory or automatic cross-tenant learning.

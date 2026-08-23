# Product packages

Every deployable product belongs in this directory. A package owns its source,
package manifest, tests, and package-local instructions; repository-wide
contracts remain at the root.

| Package | Responsibility |
| --- | --- |
| `hangar-server` | Embedded Rust API, storage engine, ingestion, retrieval, and protocol adapters. |
| `hangar-admin` | Reserved for the future Next.js/shadcn administrative console. |

Shared code is introduced only after at least two packages need it. Put such
code in a clearly named workspace package instead of creating implicit imports
between products.

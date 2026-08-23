# Design references

These primary sources informed the proposed architecture. They are references for implementation and review, not dependencies or endorsements.

- [Model Context Protocol architecture](https://modelcontextprotocol.io/specification/2025-06-18/architecture): host/client/server boundaries and capability negotiation.
- [Google's guide to AI agent protocols](https://developers.googleblog.com/en/developers-guide-to-ai-agent-protocols/): A2A for agent-to-agent discovery and communication, complementary to MCP tool/data integration.
- [UTCP specification](https://github.com/universal-tool-calling-protocol/utcp-specification): direct discovery and native invocation of tools across transports.
- [OpenTelemetry semantic conventions](https://opentelemetry.io/docs/concepts/semantic-conventions/): interoperable traces, metrics, logs, and profiling terminology.
- [OWASP LLM Top 10](https://genai.owasp.org/llm-top-10/), [OWASP MCP Top 10](https://owasp.org/www-project-mcp-top-10/), and [AI Agent Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/AI_Agent_Security_Cheat_Sheet.html): threat model and controls for memory, tools, prompts, and supply chain.
- [CockroachDB's Raft design notes](https://github.com/cockroachdb/cockroach/blob/master/docs/design.md): a pragmatic reference for bounded use of consensus in distributed storage.
- [redb](https://github.com/cberner/redb): embedded, pure-Rust ACID key-value store used for canonical metadata.
- [Tantivy](https://docs.rs/tantivy/latest/tantivy/): embedded Rust full-text search library.
- [USearch](https://docs.rs/usearch/latest/usearch/): embedded approximate-nearest-neighbor vector-search library.

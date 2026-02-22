# Oxigraph MCP Tools — Project Plan

## Vision

Expose an [Oxigraph](https://github.com/oxigraph/oxigraph) RDF triplestore as MCP (Model Context Protocol) tools for Claude Code, enabling AI-assisted knowledge graph exploration, SPARQL querying, and RDF data management directly from the CLI.

Three independent implementations — TypeScript, Python, and Rust — each using native Oxigraph bindings, sharing the same tool interface.

## Milestones

### M0 — Project Foundation ✅
- Define project goals, architecture, and specifications
- Write PLAN.md, TASKS.md, SPECIFICATIONS.md
- Set up repository structure and .gitignore

### M1 — Python Implementation
The fastest path to a working prototype thanks to pyoxigraph's mature bindings and FastMCP's minimal boilerplate.

- Set up Python project with pyproject.toml
- Implement MCP server with all specified tools using pyoxigraph
- On-disk persistent store via RocksDB backend
- Manual testing with Claude Code

### M2 — TypeScript Implementation
WASM-based Oxigraph bindings for a Node.js MCP server.

- Set up TypeScript project with package.json and tsconfig.json
- Implement MCP server with all specified tools using oxigraph npm (WASM)
- In-memory store only (WASM limitation — no RocksDB)
- Manual testing with Claude Code

### M3 — Rust Implementation
Native performance with the oxigraph crate and rmcp MCP SDK.

- Set up Rust project with Cargo.toml
- Implement MCP server with all specified tools using the oxigraph crate
- On-disk persistent store via RocksDB backend
- Manual testing with Claude Code

### M4 — Testing and Documentation
- Add integration tests for each implementation
- Write user-facing README with installation and usage instructions
- Claude Code MCP configuration examples for each implementation

### M5 — Advanced Features
- Named graph management tools
- RDF format conversion tool
- Store statistics / introspection tool
- Namespace prefix management
- Bulk loading with progress reporting

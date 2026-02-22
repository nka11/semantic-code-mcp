# Oxigraph MCP Tools — Task List

## Legend
- [ ] To do
- [x] Done
- [~] In progress

## M0 — Project Foundation

- [x] Write PLAN.md with milestones
- [x] Write TASKS.md with actionable tasks
- [x] Write SPECIFICATIONS.md with global technical specs
- [x] Create .gitignore
- [x] Create directory structure (`ts/`, `python/`, `rust/`)

## M1 — Python Implementation

- [ ] Create `python/pyproject.toml` with dependencies (mcp, pyoxigraph)
- [ ] Create `python/src/oxigraph_mcp/__init__.py`
- [ ] Implement `python/src/oxigraph_mcp/server.py`:
  - [ ] Store initialization (on-disk, configurable path)
  - [ ] `sparql_query` tool
  - [ ] `sparql_update` tool
  - [ ] `load_rdf` tool (file path and inline content)
  - [ ] `list_graphs` tool
- [ ] Manual test: register as Claude Code MCP server and run queries

## M2 — TypeScript Implementation

- [ ] Create `ts/package.json` with dependencies (@modelcontextprotocol/sdk, zod, oxigraph)
- [ ] Create `ts/tsconfig.json`
- [ ] Implement `ts/src/index.ts`:
  - [ ] Store initialization (in-memory)
  - [ ] `sparql_query` tool
  - [ ] `sparql_update` tool
  - [ ] `load_rdf` tool (file path and inline content)
  - [ ] `list_graphs` tool
- [ ] Manual test: register as Claude Code MCP server and run queries

## M3 — Rust Implementation

- [ ] Create `rust/Cargo.toml` with dependencies (rmcp, oxigraph, tokio, serde, schemars)
- [ ] Implement `rust/src/main.rs`:
  - [ ] Store initialization (on-disk, configurable path)
  - [ ] `sparql_query` tool
  - [ ] `sparql_update` tool
  - [ ] `load_rdf` tool (file path and inline content)
  - [ ] `list_graphs` tool
- [ ] Manual test: register as Claude Code MCP server and run queries

## M4 — Testing and Documentation

- [ ] Python: add integration tests with pytest
- [ ] TypeScript: add integration tests with vitest
- [ ] Rust: add integration tests
- [ ] Write README.md with installation and usage instructions
- [ ] Add Claude Code MCP configuration examples

## M5 — Advanced Features

- [ ] `list_namespaces` / `add_namespace` tools for prefix management
- [ ] `store_stats` tool (triple count, graph count, store size)
- [ ] `export_rdf` tool (dump store or graph in chosen format)
- [ ] `drop_graph` tool (remove a named graph)
- [ ] Bulk loading with progress reporting (Python/Rust only)

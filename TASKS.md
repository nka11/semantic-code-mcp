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
- [x] Create directory structure
- [ ] Remove unused `python/` and `ts/` directories

## M1 — Core Server and Generic RDF Tools

- [ ] Create `rust/Cargo.toml` with dependencies (oxigraph, rmcp, tokio, serde, schemars)
- [ ] Implement `rust/src/main.rs` — MCP server entry point with stdio transport
- [ ] Implement `rust/src/store.rs` — Oxigraph store init (on-disk RocksDB, `OXIGRAPH_STORE_PATH`)
- [ ] Implement `rust/src/tools/sparql.rs`:
  - [ ] `sparql_query` tool
  - [ ] `sparql_update` tool
- [ ] Implement `rust/src/tools/rdf.rs`:
  - [ ] `load_rdf` tool (file path and inline content, format auto-detection)
  - [ ] `list_graphs` tool
- [ ] Implement `rust/src/tools/mod.rs` — tool registration
- [ ] Manual test: register as Claude Code MCP server and run SPARQL queries

## M2 — LanguageLoader Trait and Rust Loader

- [ ] Define `LanguageLoader` trait in `rust/src/loaders/mod.rs`
- [ ] Implement loader registry and language auto-detection
- [ ] Implement `rust/src/tools/code.rs` — `load_code` generic dispatcher tool
- [ ] Implement `rust/src/loaders/rust.rs` — Rust loader:
  - [ ] Cargo.toml parsing (project metadata, dependencies)
  - [ ] `.rs` file AST extraction via `syn`:
    - [ ] Module structure and hierarchy
    - [ ] Functions and methods (name, params, return type, visibility, doc comments)
    - [ ] Structs and enums
    - [ ] Traits and impl blocks
    - [ ] Use/import statements
  - [ ] Register `load_rust_code` tool
- [ ] Manual test: load a Rust project and query its structure via SPARQL

## M3 — Python Loader

- [ ] Implement `rust/src/loaders/python.rs` — Python loader:
  - [ ] pyproject.toml / setup.py / requirements.txt parsing for dependencies
  - [ ] `.py` file AST extraction (Rust-based parser):
    - [ ] Modules and packages
    - [ ] Functions and methods (name, params, decorators, docstrings)
    - [ ] Classes (name, bases, methods)
    - [ ] Import statements
    - [ ] Type annotations
  - [ ] Register `load_python_code` tool
- [ ] Manual test: load a Python project and query its structure via SPARQL

## M4 — TypeScript Loader

- [ ] Implement `rust/src/loaders/typescript.rs` — TypeScript loader:
  - [ ] package.json parsing for project metadata and dependencies
  - [ ] `.ts`/`.tsx`/`.js`/`.jsx` file AST extraction (Rust-based parser):
    - [ ] Modules and exports
    - [ ] Functions (name, params, return type, JSDoc)
    - [ ] Classes and interfaces
    - [ ] Type aliases
    - [ ] Import/export statements
  - [ ] Register `load_ts_code` tool
- [ ] Manual test: load a TypeScript project and query its structure via SPARQL

## M5 — Testing and Documentation

- [ ] Integration tests for generic RDF tools (sparql_query, sparql_update, load_rdf, list_graphs)
- [ ] Integration tests for Rust loader
- [ ] Integration tests for Python loader
- [ ] Integration tests for TypeScript loader
- [ ] Write README.md with installation and usage instructions
- [ ] Add Claude Code MCP configuration examples

## M6 — Advanced Features

- [ ] `list_namespaces` / `add_namespace` tools for prefix management
- [ ] `store_stats` tool (triple count, graph count, store size)
- [ ] `export_rdf` tool (dump store or graph in chosen format)
- [ ] `drop_graph` tool (remove a named graph)
- [ ] Bulk loading with progress reporting
- [ ] Additional language loaders (Go, Java, C/C++, etc.)

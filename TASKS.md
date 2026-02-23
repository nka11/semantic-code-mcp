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
- [x] Remove unused `python/` and `ts/` directories (never created)

## M1 — Core Server and Generic RDF Tools ✅

- [x] Create `rust/Cargo.toml` with dependencies (oxigraph, rmcp, tokio, serde, schemars)
- [x] Implement `rust/src/main.rs` — MCP server entry point with stdio transport
- [x] Implement `rust/src/store.rs` — Oxigraph store init (on-disk RocksDB, `OXIGRAPH_STORE_PATH`)
- [x] Implement `rust/src/tools/sparql.rs`:
  - [x] `sparql_query` tool
  - [x] `sparql_update` tool
- [x] Implement `rust/src/tools/rdf.rs`:
  - [x] `load_rdf` tool (file path and inline content, format auto-detection)
  - [x] `list_graphs` tool
- [x] Implement `rust/src/tools/mod.rs` — tool registration
- [x] Manual test: register as Claude Code MCP server and run SPARQL queries

## M2 — LanguageLoader Trait and Rust Loader ✅

- [x] Define `LanguageLoader` trait in `rust/src/loaders/mod.rs`
- [x] Implement loader registry and language auto-detection
- [x] Implement `rust/src/tools/code.rs` — `load_code` generic dispatcher tool
- [x] Implement `rust/src/loaders/rust.rs` — Rust loader:
  - [x] Cargo.toml parsing (project metadata, dependencies)
  - [x] `.rs` file AST extraction via `syn`:
    - [x] Module structure and hierarchy
    - [x] Functions and methods (name, params, return type, visibility, doc comments)
    - [x] Structs and enums
    - [x] Traits and impl blocks
    - [x] Use/import statements
  - [x] Register `load_rust_code` tool
- [x] Manual test: load a Rust project and query its structure via SPARQL

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

## M4 — TypeScript Loader ✅

- [x] Implement `rust/src/loaders/typescript.rs` — TypeScript loader:
  - [x] package.json parsing for project metadata and dependencies
  - [x] `.ts`/`.tsx`/`.js`/`.jsx` file AST extraction (oxc_parser):
    - [x] Modules and exports
    - [x] Functions (name, params, return type, JSDoc)
    - [x] Classes and interfaces
    - [x] Type aliases
    - [x] Import/export statements
  - [x] Register `load_ts_code` tool
- [ ] Manual test: load a TypeScript project and query its structure via SPARQL

## M5 — Testing and Documentation

- [x] Integration tests for generic RDF tools (sparql_query, sparql_update, load_rdf, list_graphs)
- [x] Integration tests for Rust loader
- [ ] Integration tests for Python loader
- [x] Integration tests for TypeScript loader
- [x] Write README.md with installation and usage instructions
- [x] Add Claude Code MCP configuration examples

## M6 — Advanced Features

- [ ] `list_namespaces` / `add_namespace` tools for prefix management
- [ ] `store_stats` tool (triple count, graph count, store size)
- [ ] `export_rdf` tool (dump store or graph in chosen format)
- [ ] `drop_graph` tool (remove a named graph)
- [ ] Bulk loading with progress reporting
- [ ] Additional language loaders (Go, Java, C/C++, etc.)

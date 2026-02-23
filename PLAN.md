# Oxigraph MCP Tools — Project Plan

## Vision

Expose an [Oxigraph](https://github.com/oxigraph/oxigraph) RDF triplestore as MCP (Model Context Protocol) tools for Claude Code. A single Rust MCP server provides generic RDF/SPARQL tools alongside language-specific code-loading tools that parse source code into an RDF knowledge graph — enabling an LLM coding agent to query and reason about a project's structure, dependencies, and relationships.

## Milestones

### M0 — Project Foundation ✅
- Define project goals, architecture, and specifications
- Write PLAN.md, TASKS.md, SPECIFICATIONS.md
- Set up repository structure and .gitignore

### M1 — Core Server and Generic RDF Tools ✅
Minimal working MCP server with the 4 generic RDF tools.

- Set up Rust project with Cargo.toml (oxigraph, rmcp, tokio, serde, schemars)
- Implement MCP server entry point with stdio transport
- Implement Oxigraph store initialization (on-disk RocksDB, configurable path)
- Implement `sparql_query` tool
- Implement `sparql_update` tool
- Implement `load_rdf` tool (file path and inline content)
- Implement `list_graphs` tool
- Manual testing with Claude Code

### M2 — LanguageLoader Trait and Rust Loader ✅
Plugin system foundation plus the first code loader.

- Define the `LanguageLoader` trait
- Implement loader registry and auto-detection
- Implement `load_code` generic dispatcher tool
- Implement Rust loader (`load_rust_code`):
  - Cargo.toml parsing (project metadata, dependencies)
  - `.rs` file AST extraction (modules, functions, structs, enums, traits, impls, use statements)
  - Module hierarchy resolution
- Manual testing: load a Rust project and query its structure via SPARQL

### M3 — Python Loader
- Implement Python loader (`load_python_code`):
  - pyproject.toml / setup.py / requirements.txt parsing
  - `.py` file AST extraction (modules, functions, classes, decorators, imports, docstrings)
- Manual testing: load a Python project and query its structure via SPARQL

### M4 — TypeScript Loader ✅
- Implement TypeScript loader (`load_ts_code`):
  - package.json parsing (metadata, dependencies)
  - `.ts`/`.tsx`/`.js`/`.jsx` file AST extraction via `oxc_parser` (modules, functions, classes, interfaces, type aliases, enums, imports/exports, JSDoc)
- Manual testing: load a TypeScript project and query its structure via SPARQL

### M5 — Testing and Documentation
- Integration tests for generic RDF tools
- Integration tests for each language loader
- User-facing README with installation and usage instructions
- Claude Code MCP configuration examples

### M6 — Git History Loader
Load git commit history into the RDF knowledge graph, enabling queries that join code structure with change history.

- Add `git2` crate (libgit2 bindings) for native repository access
- Implement git history loader in `loaders/git.rs`:
  - Walk commit graph from HEAD or specified branch
  - Extract commit metadata (hash, author, committer, date, message, parents)
  - Extract per-commit file diffs (added, modified, deleted, renamed)
  - Generate `code:Commit` and `code:FileChange` RDF triples
  - Cross-graph linking: `code:FileChange` → `code:Module` via `code:affectsModule`
- Implement `load_git_history` tool in `tools/git.rs`
- Default named graph: `code:git`
- Unit tests with temporary git repositories
- Manual testing: load history and run cross-graph SPARQL queries

### M7 — Advanced Features
- Named graph management tools (`drop_graph`, `export_rdf`)
- Store statistics / introspection tool (`store_stats`)
- Namespace prefix management (`list_namespaces`, `add_namespace`)
- Bulk loading with progress reporting
- Additional language loaders (Go, Java, C/C++, etc.)

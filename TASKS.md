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

## M6 — Git History Loader

- [ ] Add `git2` crate dependency to `rust/Cargo.toml`
- [ ] Implement `rust/src/loaders/git.rs` — Git history loader:
  - [ ] Open repository via `git2::Repository::open()`
  - [ ] Walk commit graph from HEAD (or specified branch/ref)
  - [ ] Extract commit metadata (hash, author, committer, date, message, parents)
  - [ ] Extract per-commit file changes via diff-tree (added, modified, deleted, renamed)
  - [ ] Generate `code:Commit` and `code:FileChange` RDF triples
  - [ ] Link `code:FileChange` to `code:Module` via `code:affectsModule` (same-graph join)
  - [ ] Respect `max_commits` limit
- [ ] Implement `rust/src/tools/git.rs` — `load_git_history` tool:
  - [ ] Tool parameter schema (path, graph, max_commits, branch)
  - [ ] Pure sync function taking `&Store`
  - [ ] Summary output (commit count, file change count)
- [ ] Register `load_git_history` tool in `main.rs`
- [ ] Unit tests:
  - [ ] Test commit metadata extraction with a temp git repo
  - [ ] Test file change detection (add, modify, delete, rename)
  - [ ] Test max_commits limiting
  - [ ] Test module linking via `code:affectsModule`
- [ ] Manual test: load git history and query commits/changes via SPARQL

## M7 — Advanced Features

- [ ] `list_namespaces` / `add_namespace` tools for prefix management
- [ ] `store_stats` tool (triple count, graph count, store size)
- [ ] `export_rdf` tool (dump store or graph in chosen format)
- [ ] `drop_graph` tool (remove a named graph)
- [ ] Bulk loading with progress reporting
- [ ] Additional language loaders (Go, Java, C/C++, etc.)

## M8 — Pluggable Vector Store

- [ ] Restructure `rust/` as a Cargo workspace (workspace `Cargo.toml` + binary member)
- [ ] Create `crates/vector_store/` crate with `Cargo.toml`
- [ ] Define `VectorStore` async trait in `crates/vector_store/src/lib.rs`
- [ ] Define `RagChunk`, `SearchHit`, `Filter` types
- [ ] Implement `InMemoryVectorStore` in `crates/vector_store/src/inmemory.rs`:
  - [ ] Add `hnsw_rs`, `ndarray`, `dashmap` dependencies
  - [ ] Implement `upsert` — insert/update chunks in HNSW index + DashMap
  - [ ] Implement `delete` — remove chunks from index + DashMap
  - [ ] Implement `search` — ANN query with cosine distance, optional filtering
- [ ] Unit tests for upsert, delete, search, filtering
- [ ] Optional: snapshot persistence (serialize index to disk)

## M9 — RAG Pipeline

- [ ] Create `crates/rag_pipeline/` crate with `Cargo.toml`
- [ ] Define embedding provider trait (pluggable API-based embedding)
- [ ] Implement RDF canonicalization:
  - [ ] CURIE expansion to full IRIs
  - [ ] Deterministic predicate ordering
  - [ ] Blank node collapsing
  - [ ] Chunk text generation from RDF triples
- [ ] Implement retrieval pipeline:
  - [ ] Embed user query
  - [ ] Call `VectorStore.search(k, filter)`
  - [ ] Return ranked `SearchHit` results
- [ ] Implement optional reranking pass
- [ ] Implement context compression for LLM prompt fitting
- [ ] Integration tests with `InMemoryVectorStore`

## M10 — Agent Orchestrator

- [ ] Create `crates/agent_orchestrator/` crate with `Cargo.toml`
- [ ] Define `AgentTool` async trait and `ToolInput` / `ToolOutput` types
- [ ] Implement `SparqlTool` — wraps existing Oxigraph store
- [ ] Implement `RagTool` — wraps `rag_pipeline` + `VectorStore`
- [ ] Implement `CodegenTool` — wraps LLM client for code generation
- [ ] Implement planner/router:
  - [ ] Query classification (SPARQL vs RAG vs codegen)
  - [ ] Multi-step planning
  - [ ] Result aggregation
- [ ] Enforce prompt contract:
  - [ ] IRI citation for semantic answers
  - [ ] Chunk ID citation for RAG context
  - [ ] Grounding verification (refuse ungrounded answers)
- [ ] Wire orchestrator into the MCP server as new tool(s)
- [ ] Integration tests with mock LLM client

## M11 — External Vector DB Adapters

- [ ] Implement Qdrant adapter in `crates/vector_store/src/qdrant.rs`:
  - [ ] Add `qdrant-client` dependency (feature-gated)
  - [ ] Implement `VectorStore` trait for Qdrant
  - [ ] Connection management and error handling
- [ ] Implement Milvus adapter in `crates/vector_store/src/milvus.rs`:
  - [ ] Add Milvus client dependency (feature-gated)
  - [ ] Implement `VectorStore` trait for Milvus
- [ ] TOML-based backend configuration (`[rag] backend = "qdrant"`)
- [ ] Implement `VectorBackend` enum and factory function
- [ ] Fallback to in-memory on adapter failure
- [ ] Integration tests with containerized Qdrant / Milvus

## M12 — Observability & Production Hardening

- [ ] Add `request_id` propagation via `tracing` spans
- [ ] Log `retrieved_chunk_ids` and `sparql_queries` per request
- [ ] Implement Precision@K evaluation hook
- [ ] Implement Faithfulness evaluation hook
- [ ] Implement latency tracking and reporting
- [ ] Implement PII redaction before embedding
- [ ] Implement namespace isolation per tenant
- [ ] Implement graph-level ACL filtering
- [ ] Implement hash-based chunk deduplication
- [ ] Performance benchmarks:
  - [ ] Retrieval latency < 20 ms (in-memory)
  - [ ] Recall@10 > 0.9
  - [ ] Cold start (100k chunks) < 5 sec
  - [ ] Memory usage < 2 GB

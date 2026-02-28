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

## M3 — Python Loader ✅

- [x] Implement `rust/src/loaders/python.rs` — Python loader:
  - [x] pyproject.toml parsing for project metadata and dependencies
  - [x] `.py` file AST extraction via `rustpython-parser`:
    - [x] Modules and packages
    - [x] Functions and methods (name, params, decorators, docstrings, async)
    - [x] Classes (name, bases, methods, fields)
    - [x] Import statements
    - [x] Type annotations
  - [x] Register `load_python_code` tool
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

## M5 — Testing and Documentation ✅

- [x] Integration tests for generic RDF tools (sparql_query, sparql_update, load_rdf, list_graphs)
- [x] Integration tests for Rust loader
- [x] Integration tests for Python loader
- [x] Integration tests for TypeScript loader
- [x] Write README.md with installation and usage instructions
- [x] Add Claude Code MCP configuration examples

## M6 — Git History Loader ✅

- [x] Add `git2` crate dependency to `rust/Cargo.toml`
- [x] Implement `rust/src/loaders/git.rs` — Git history loader:
  - [x] Open repository via `git2::Repository::open()`
  - [x] Walk commit graph from HEAD (or specified branch/ref)
  - [x] Extract commit metadata (hash, author, committer, date, message, parents)
  - [x] Extract per-commit file changes via diff-tree (added, modified, deleted, renamed)
  - [x] Generate `code:Commit` and `code:FileChange` RDF triples
  - [x] Link `code:FileChange` to `code:Module` via `code:affectsModule` (same-graph join)
  - [x] Respect `max_commits` limit
- [x] Implement `rust/src/tools/git.rs` — `load_git_history` tool:
  - [x] Tool parameter schema (path, max_commits, branch)
  - [x] Pure sync function taking `&Store`
  - [x] Summary output (commit count, file change count)
- [x] Register `load_git_history` tool in `main.rs`
- [x] Unit tests:
  - [x] Test commit metadata extraction with a temp git repo
  - [x] Test file change detection (add, modify, delete, rename)
  - [x] Test max_commits limiting
  - [x] Test module linking via `code:affectsModule`
- [ ] Manual test: load git history and query commits/changes via SPARQL

## M7 — Advanced Features

- [ ] `list_namespaces` / `add_namespace` tools for prefix management
- [ ] `store_stats` tool (triple count, graph count, store size)
- [ ] `export_rdf` tool (dump store or graph in chosen format)
- [ ] `drop_graph` tool (remove a named graph)
- [ ] Bulk loading with progress reporting
- [ ] Additional language loaders (Go, Java, C/C++, etc.)

## M8 — Pluggable Vector Store ✅

- [x] Restructure `rust/` as a Cargo workspace (workspace `Cargo.toml` + binary member)
- [x] Create `crates/vector_store/` crate with `Cargo.toml`
- [x] Define `VectorStore` async trait in `crates/vector_store/src/lib.rs`
- [x] Define `RagChunk`, `SearchHit`, `Filter` types
- [x] Implement `InMemoryVectorStore` in `crates/vector_store/src/inmemory.rs`:
  - [x] Add `hnsw_rs`, `dashmap` dependencies
  - [x] Implement `upsert` — insert/update chunks in HNSW index + DashMap
  - [x] Implement `delete` — remove chunks from index + DashMap
  - [x] Implement `search` — ANN query with cosine distance, optional filtering
- [x] Unit tests for upsert, delete, search, filtering

## M9 — RAG Pipeline ✅

- [x] Create `crates/rag_pipeline/` crate with `Cargo.toml`
- [x] Define embedding provider trait (`EmbeddingProvider`, `MockEmbeddingProvider`, `HttpEmbeddingProvider`)
- [x] Implement RDF canonicalization:
  - [x] Deterministic predicate ordering
  - [x] Chunk text generation from RDF triples
- [x] Implement retrieval pipeline:
  - [x] Embed user query
  - [x] Call `VectorStore.search(k, filter)`
  - [x] Return ranked `SearchHit` results
- [x] Implement reranking pass (`Reranker` trait, `PassThroughReranker`)
- [x] Implement context compression (`ContextCompressor` trait, `TruncatingCompressor`)
- [x] Integration tests with `InMemoryVectorStore`

## M10 — Agent Orchestrator ✅

- [x] Create `crates/agent_orchestrator/` crate with `Cargo.toml`
- [x] Define `AgentTool` async trait and `ToolInput` / `ToolOutput` types
- [x] Implement `SparqlTool` — wraps existing Oxigraph store
- [x] Implement `RagTool` — wraps `rag_pipeline` + `VectorStore`
- [x] Implement `CodegenTool` — wraps LLM client for code generation
- [x] Implement planner/router (`AgentRouter`):
  - [x] Query classification (SPARQL vs RAG vs codegen)
  - [x] Multi-step planning
  - [x] Result aggregation
- [x] Enforce prompt contract:
  - [x] IRI citation for semantic answers
  - [x] Chunk ID citation for RAG context
  - [x] Grounding verification (refuse ungrounded answers)
- [x] Wire orchestrator into the MCP server as `agent_query` tool
- [x] Integration tests with mock LLM client

## M10.5 — Graph Indexer & Qdrant Backend ✅

- [x] Refactor `RagPipeline` from `Box` to `Arc` for shared state
- [x] Add `HttpEmbeddingProvider` for OpenAI-compatible embedding endpoints
- [x] Implement `GraphIndexer` (SPARQL → canonicalize → embed → upsert) in `rag_pipeline`
- [x] Wire `index_graph` as MCP tool
- [x] Implement `QdrantVectorStore` in `crates/vector_store/src/qdrant.rs`:
  - [x] Add `qdrant-client` dependency
  - [x] Implement `VectorStore` trait for Qdrant over gRPC
  - [x] Auto-create collection on first upsert
  - [x] Client-side post-filtering for IriPrefix
- [x] Auto-select Qdrant when `QDRANT_URL` is set, fall back to in-memory
- [x] Add `docker-compose.yml` for Qdrant (v1.13.2, REST + gRPC)
- [x] Add setup docs in `docs/qdrant-setup.md`
- [x] Configure `QDRANT_URL` in `.mcp.json`

## M11 — External Vector DB Adapters

- [x] ~~Implement Qdrant adapter~~ (done in M10.5)
- [ ] Implement Milvus adapter in `crates/vector_store/src/milvus.rs`:
  - [ ] Add Milvus client dependency
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

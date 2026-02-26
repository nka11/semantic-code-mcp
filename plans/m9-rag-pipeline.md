# M9 — RAG Pipeline

## Context

M8 delivered a pluggable `VectorStore` trait with an `InMemoryVectorStore` (HNSW-based) in `crates/vector_store/`. M9 builds the retrieval pipeline on top of it: embedding, canonicalization, search, reranking, and context compression — everything needed before M10 wires it into the MCP server as agent tools.

## File Structure

```
rust/
├── Cargo.toml                          # MODIFY — add crates/rag_pipeline to workspace members
└── crates/
    └── rag_pipeline/                   # NEW crate
        ├── Cargo.toml
        └── src/
            ├── lib.rs                  # Module declarations + public re-exports
            ├── embedding.rs            # EmbeddingProvider trait + MockEmbeddingProvider
            ├── canonicalize.rs         # RDF canonicalization → deterministic chunk text
            ├── rerank.rs               # Reranker trait + PassThroughReranker
            ├── compress.rs             # ContextCompressor trait + TruncatingCompressor
            └── pipeline.rs            # RagPipeline orchestrator (embed → search → rerank → compress)
```

## Dependencies

`crates/rag_pipeline/Cargo.toml`:
- `vector_store = { path = "../vector_store" }`
- `async-trait = "0.1"`, `anyhow = "1"`, `tokio = { version = "1", features = ["rt"] }`, `tracing = "0.1"`
- dev: `tokio = { version = "1", features = ["full"] }`

## Implementation Tasks

### Task 1 — Scaffold crate
### Task 2 — embedding.rs
### Task 3 — canonicalize.rs
### Task 4 — rerank.rs
### Task 5 — compress.rs
### Task 6 — pipeline.rs + integration tests

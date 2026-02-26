# M8 — Pluggable Vector Store

## Context

The project needs a vector store abstraction to support RAG (Retrieval-Augmented Generation) in future milestones. M8 introduces the `VectorStore` trait and an in-memory HNSW-based default backend as a separate crate, and restructures `rust/` as a Cargo workspace to support multiple crates. This is a prerequisite for M9 (RAG pipeline) and M11 (external vector DB adapters).

## Target Structure

```
rust/
├── Cargo.toml                     # MODIFIED: add [workspace] section
├── crates/
│   └── vector_store/
│       ├── Cargo.toml             # NEW
│       └── src/
│           ├── lib.rs             # NEW: VectorStore trait + types
│           └── inmemory.rs        # NEW: InMemoryVectorStore
└── src/                           # UNCHANGED
```

## Steps

### 1. Convert `rust/Cargo.toml` to workspace root

Prepend a `[workspace]` block — everything else unchanged.

### 2. Create `crates/vector_store/Cargo.toml`

Dependencies: hnsw_rs, anndists, dashmap, async-trait, anyhow, tokio.

### 3. Define types and trait in `lib.rs`

- `RagChunk { id, iri, text, graph, embedding, metadata }`
- `SearchHit { id, score, text, metadata }`
- `Filter { Graph, IriPrefix, MetadataEq }` with `matches()` method
- `VectorStore` async trait: `upsert`, `delete`, `search`

### 4. Implement `InMemoryVectorStore` in `inmemory.rs`

- HNSW index behind `RwLock` for thread-safe mutable access
- ID mapping via DashMap, soft-delete via DashSet
- Dimension enforcement on first insert
- Distance-to-similarity conversion: `1.0 - dist`

### 5. Unit tests (9 tests)

basic_upsert_and_search, upsert_overwrites, delete_removes_from_results, filter_graph, filter_iri_prefix, filter_metadata_eq, dimension_mismatch_error, empty_store_search, k_larger_than_corpus

## Commit Sequence

1. `feat(m8): convert rust/ to Cargo workspace`
2. `feat(m8): add vector_store crate with VectorStore trait and types`
3. `feat(m8): implement InMemoryVectorStore with HNSW backend`
4. `test(m8): add unit tests for InMemoryVectorStore`

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

- launch semantic-code-mcp tool load_rust_code at startup.

## Project

Oxigraph MCP Tools — a Rust MCP server that exposes an Oxigraph RDF triplestore via stdio JSON-RPC. It provides generic SPARQL/RDF tools plus per-language code-loading tools that parse source code into an RDF knowledge graph.

## Build & Development Commands

All commands run from the `rust/` subdirectory:

```bash
cargo build                          # Debug build
cargo build --release                # Release build (binary: target/release/oxigraph-mcp)
cargo run                            # Run MCP server (stdio transport)
cargo test                           # Run all tests
cargo test <name_substring>          # Run a single test by name
cargo test tools::sparql::tests      # Run tests in a specific module
cargo clippy                         # Lint
cargo fmt --check                    # Check formatting
cargo fmt                            # Auto-format
```

## Architecture

```
rust/src/
├── main.rs          # OxigraphServer struct, #[tool_router]/#[tool_handler] MCP dispatch, async entry point
├── store.rs         # Oxigraph Store init (OXIGRAPH_STORE_PATH env var, default ./oxigraph_data)
├── tools/
│   ├── sparql.rs    # sparql_query, sparql_update — pure sync functions
│   ├── rdf.rs       # load_rdf, list_graphs — pure sync functions
│   └── code.rs      # load_code dispatcher, load_rust_code wrapper — pure sync functions
└── loaders/
    ├── mod.rs       # LanguageLoader trait, LoaderRegistry, discover_files()
    └── rust.rs      # RustLoader — parses Cargo.toml + syn AST for .rs files
```

**Key design patterns:**
- Tool logic lives in `tools/` as pure synchronous functions taking `&Store` — easy to unit test without MCP server.
- `main.rs` tool handlers wrap these in `tokio::task::spawn_blocking`.
- `LanguageLoader` trait returns `Vec<Quad>` (graph name baked in). `LoaderRegistry` maps language IDs to loaders.
- Language auto-detection: marker files for dirs (`Cargo.toml`, `package.json`, etc.), file extensions for single files.
- RDF namespace: `https://ds-labs.org/code#` (`CODE_NS` in loaders). Default named graph for Rust: `code:rust`.

**Key crates:** oxigraph 0.5.x (store + SPARQL), rmcp 0.16.x (MCP SDK), syn 2 (Rust AST parsing), sparesults 0.3 (SPARQL result serialization), schemars 1 (JSON Schema for tool params).

## Test Patterns

- Tests use `#[cfg(test)]` modules within each source file.
- Each test module defines `result_text()` and `is_error()` helpers.
- Tests create `Store::new()` (in-memory, no file) for isolation.
- Code loader tests use `tempfile::TempDir` with real Cargo.toml and .rs files, then verify via SPARQL queries.

## Environment Variables

| Var | Default | Description |
|---|---|---|
| `OXIGRAPH_STORE_PATH` | `./oxigraph_data` | RocksDB store path |
| `RUST_LOG` | (unset) | tracing-subscriber log level filter |

## Specifications

See `SPECIFICATIONS.md` for the full RDF ontology (classes, properties), tool interface definitions, and milestone plan. The spec is the source of truth for how code entities map to RDF triples.

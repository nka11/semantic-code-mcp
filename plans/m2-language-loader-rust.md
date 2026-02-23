# M2 — LanguageLoader Trait and Rust Loader

## Context

M1 delivered a working MCP server with 4 generic RDF tools. M2 adds the code-loading plugin system: a `LanguageLoader` trait, a loader registry with auto-detection, a `load_code` generic dispatcher tool, and the first concrete loader for Rust (Cargo.toml parsing + `.rs` AST extraction via `syn`).

## New Dependencies

- `syn = { version = "2", features = ["full", "visit"] }` — Rust AST parsing
- `toml = "0.8"` — Cargo.toml parsing
- `walkdir = "2"` — Recursive directory traversal

## Architecture

```
rust/src/
├── main.rs              # Add load_code + load_rust_code tool methods
├── store.rs             # Unchanged
├── tools/
│   ├── mod.rs           # Add `pub mod code;`
│   ├── sparql.rs        # Unchanged
│   ├── rdf.rs           # Unchanged
│   └── code.rs          # load_code() dispatcher, load_rust_code() wrapper
└── loaders/
    ├── mod.rs           # LanguageLoader trait, LoaderRegistry, LoadError, auto-detection
    └── rust.rs          # RustLoader: Cargo.toml + syn AST extraction
```

## Implementation Steps

1. Add dependencies to Cargo.toml
2. LanguageLoader trait + LoaderRegistry (loaders/mod.rs)
3. RustLoader — Cargo.toml parsing (loaders/rust.rs)
4. RustLoader — .rs file AST extraction
5. tools/code.rs — load_code and load_rust_code
6. Wire tools into main.rs
7. Integration tests
8. Build + smoke test

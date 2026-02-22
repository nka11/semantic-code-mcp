# M1 — Core Server and Generic RDF Tools

## Context

M0 established the project specs. M1 implements the minimal working MCP server: a single Rust binary with 4 generic RDF tools (`sparql_query`, `sparql_update`, `load_rdf`, `list_graphs`). This is the foundation all later milestones (code loaders, plugin system) build on.

## Architecture

All `#[tool]` methods must live in a single `#[tool_router] impl` block on the server struct (rmcp constraint). Helper logic is split into modules. Oxigraph calls are synchronous (RocksDB I/O), so they run inside `tokio::task::spawn_blocking`.

```
main.rs  ──  OxigraphServer struct + #[tool] methods + #[tool_handler] + main()
store.rs ──  open_store() → Store
tools/
  mod.rs    ──  pub mod sparql; pub mod rdf;
  sparql.rs ──  sparql_query(), sparql_update() free functions
  rdf.rs    ──  load_rdf(), list_graphs() free functions
```

## Dependencies (`rust/Cargo.toml`)

```toml
[package]
name = "oxigraph-mcp"
version = "0.1.0"
edition = "2021"
rust-version = "1.75"

[dependencies]
oxigraph = "0.5"
rmcp = { version = "0.16", features = ["server", "transport-io", "macros"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

## Key API Patterns (from research)

### rmcp
- Server struct: `#[derive(Clone)]` with `tool_router: ToolRouter<Self>` field, init via `Self::tool_router()`
- Tools: `#[tool(description = "...")]` on async methods, params via `#[tool(param)]` annotations
- Server trait: `#[tool_handler] impl ServerHandler for T` with `get_info() -> ServerInfo`
- Stdio: `server.serve(rmcp::transport::stdio()).await?; service.waiting().await?`
- Tool errors: `CallToolResult` with `is_error: true` for expected errors, `Err(McpError)` for infrastructure failures

### oxigraph 0.5
- `Store::open(path)` for on-disk RocksDB
- Query: `SparqlEvaluator::new().parse_query(q)?.on_store(&store).execute()` → `QueryResults` (Solutions/Graph/Boolean)
- Update: `SparqlEvaluator::new().parse_update(u)?.on_store(&store).execute()`
- Serialization: `QueryResultsSerializer::from_format(Json)` for SELECT → JSON; `RdfSerializer::from_format(NTriples)` for CONSTRUCT/DESCRIBE
- Load: `store.load_from_reader(RdfParser::from_format(...), reader)`; `store.load_from_slice(parser, bytes)`
- Graphs: `store.named_graphs()` → `GraphNameIter`; `store.quads_for_pattern(None, None, None, Some(GraphNameRef::DefaultGraph))` to check default graph
- `default_graph` parameter: not directly supported by SparqlEvaluator; handle via `FROM <graph>` clause injection or document limitation

## Implementation Steps (commit after each)

### Step 1: Cargo.toml + empty main
- Create `rust/Cargo.toml` with deps above
- Create `rust/src/main.rs` with minimal `fn main() {}`
- Remove `rust/src/.gitkeep`
- `cargo check` to verify deps resolve
- **Commit**: "M1: Initialize Rust project with Cargo.toml"

### Step 2: store.rs
- `open_store() -> Result<Store, Box<dyn Error>>`: read `OXIGRAPH_STORE_PATH` env (default `./oxigraph_data`), create parent dirs, `Store::open(path)`
- **Commit**: "M1: Add Oxigraph store initialization"

### Step 3: tools/sparql.rs
- `sparql_query(store, query, default_graph) -> Result<CallToolResult, McpError>`:
  - Parse via `SparqlEvaluator::new().parse_query(query)`
  - Execute on store, match `QueryResults`:
    - Solutions → `QueryResultsSerializer` with `QueryResultsFormat::Json`
    - Graph → `RdfSerializer` with `RdfFormat::NTriples`
    - Boolean → `"true"` / `"false"` text
  - All oxigraph errors → `CallToolResult::error(...)` (tool-level, not protocol-level)
- `sparql_update(store, update) -> Result<CallToolResult, McpError>`:
  - Parse + execute via `SparqlEvaluator::new().parse_update(update)`
  - Success → "SPARQL UPDATE executed successfully."
- `tools/mod.rs`: declare `pub mod sparql; pub mod rdf;`
- **Commit**: "M1: Add sparql_query and sparql_update tools"

### Step 4: tools/rdf.rs
- `load_rdf(store, input, format, base_iri, graph) -> Result<CallToolResult, McpError>`:
  - Detect file vs inline: `Path::new(input)` → check if absolute + exists
  - Resolve format: explicit param → short name map + MIME type lookup; else file extension; else Turtle
  - Build `RdfParser` with optional `base_iri` and `default_graph`
  - Count: `store.len()` before/after for delta
  - Load via `store.load_from_reader()` (file) or `store.load_from_slice()` (inline)
- `list_graphs(store) -> Result<CallToolResult, McpError>`:
  - Check default graph: `store.quads_for_pattern(None, None, None, Some(GraphNameRef::DefaultGraph)).next().is_some()`
  - Iterate `store.named_graphs()`
  - Output: JSON array of strings
- `resolve_format(name) -> Option<RdfFormat>`: map short names + MIME types
- **Commit**: "M1: Add load_rdf and list_graphs tools"

### Step 5: main.rs — wire everything up
- `OxigraphServer` struct: `store: Arc<Store>`, `tool_router: ToolRouter<Self>`
- `#[tool_router] impl` with 4 `#[tool]` methods delegating to `tools::*` via `spawn_blocking`
- `#[tool_handler] impl ServerHandler` with `get_info()` returning server name + capabilities
- `main()`: init tracing to stderr, `open_store()`, create server, `.serve(stdio()).await`, `.waiting().await`
- **Commit**: "M1: Wire up MCP server with stdio transport and all 4 tools"

### Step 6: Build + smoke test
- `cargo build --release`
- Register in `.mcp.json` and test with Claude Code
- Fix any issues
- **Commit** (if fixes): "M1: Fix issues from smoke testing"

## Files to Create/Modify

| File | Action |
|---|---|
| `rust/Cargo.toml` | Create |
| `rust/src/main.rs` | Create (replace .gitkeep) |
| `rust/src/store.rs` | Create |
| `rust/src/tools/mod.rs` | Create |
| `rust/src/tools/sparql.rs` | Create |
| `rust/src/tools/rdf.rs` | Create |
| `rust/src/.gitkeep` | Delete |

## Verification

1. `cargo check` — compiles without errors
2. `cargo build --release` — produces `rust/target/release/oxigraph-mcp` binary
3. Register in `.mcp.json`:
   ```json
   {"mcpServers":{"oxigraph":{"command":"./rust/target/release/oxigraph-mcp"}}}
   ```
4. Test each tool:
   - `load_rdf` with inline Turtle → expect triple count
   - `sparql_query` SELECT → expect JSON results
   - `sparql_update` INSERT DATA → expect success
   - `list_graphs` → expect JSON array

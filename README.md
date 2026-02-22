# Oxigraph MCP Tools

An MCP (Model Context Protocol) server that exposes an [Oxigraph](https://oxigraph.org) RDF triplestore to Claude Code. Load source code into an RDF knowledge graph and query it with SPARQL.

## Features

**Generic RDF tools:**
- `sparql_query` — Execute read-only SPARQL queries (SELECT, CONSTRUCT, ASK, DESCRIBE)
- `sparql_update` — Execute SPARQL UPDATE operations (INSERT, DELETE, CLEAR, DROP)
- `load_rdf` — Load RDF data from files or inline content (Turtle, N-Triples, N-Quads, TriG, RDF/XML, N3)
- `list_graphs` — List all named graphs in the store

**Code loading tools:**
- `load_code` — Generic dispatcher that auto-detects language and parses source code into RDF
- `load_rust_code` — Parse Rust projects: Cargo.toml metadata, functions, structs, enums, traits, impl blocks, imports, and modules

## Prerequisites

- Rust 1.75+
- Cargo

## Build

```bash
cd rust
cargo build --release
```

The binary is at `rust/target/release/oxigraph-mcp`.

## Configuration

Configure as an MCP server in your Claude Code settings (`~/.claude.json`):

```json
{
  "mcpServers": {
    "oxigraph": {
      "command": "/path/to/oxigraph-mcp",
      "env": {
        "OXIGRAPH_STORE_PATH": "/path/to/store"
      }
    }
  }
}
```

| Environment Variable | Default | Description |
|---|---|---|
| `OXIGRAPH_STORE_PATH` | `./oxigraph_data` | Path to the RocksDB on-disk store |

## Usage Examples

Load a Rust project and query its structure:

```
# Load project
load_rust_code path="/home/user/my-project"

# List all functions
sparql_query query="PREFIX code: <https://oxigraph.org/code#>
SELECT ?name ?file FROM <https://oxigraph.org/code#rust> WHERE {
  ?f a code:Function ; code:name ?name ; code:definedIn ?m .
  ?m code:relativePath ?file .
}"

# List dependencies
sparql_query query="PREFIX code: <https://oxigraph.org/code#>
SELECT ?name ?ver FROM <https://oxigraph.org/code#rust> WHERE {
  ?d a code:Dependency ; code:name ?name ; code:version ?ver .
}"

# List graphs
list_graphs
```

## RTK (Recommended)

For reduced token usage during development with Claude Code, install [RTK](https://github.com/rtk-ai/rtk):

```bash
cargo install --git https://github.com/rtk-ai/rtk.git
```

RTK transparently proxies CLI commands (git, cargo, etc.) and filters verbose output, saving 60-90% on tokens.

## Running Tests

```bash
cd rust
cargo test
```

## Tech Stack

| Component | Technology |
|---|---|
| Language | Rust 1.75+ |
| RDF Store | oxigraph 0.5.x (RocksDB on-disk) |
| MCP SDK | rmcp 0.16.x |
| AST Parsing | syn 2.x |
| Transport | stdio (JSON-RPC) |

## License

See [LICENSE](LICENSE) for details.

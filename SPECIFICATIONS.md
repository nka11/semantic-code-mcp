# Oxigraph MCP Tools — Specifications

## 1. Overview

This project provides MCP (Model Context Protocol) tool servers that expose an Oxigraph RDF triplestore to Claude Code. Three implementations share an identical tool interface:

| Implementation | Language | Oxigraph Binding | MCP SDK | Store Backend |
|---|---|---|---|---|
| `python/` | Python 3.10+ | pyoxigraph 0.5.x | mcp (FastMCP) 1.x | RocksDB (on-disk) |
| `ts/` | TypeScript / Node 18+ | oxigraph 0.5.x (WASM) | @modelcontextprotocol/sdk 1.x | In-memory |
| `rust/` | Rust 1.75+ | oxigraph 0.5.x | rmcp 0.16.x | RocksDB (on-disk) |

## 2. Architecture

```
Claude Code  <──stdio──>  MCP Server  <──native API──>  Oxigraph Store
                          (TS/Py/Rust)                   (memory or disk)
```

- **Transport**: stdio (stdin/stdout JSON-RPC)
- **Store lifecycle**: the store is opened when the MCP server starts and persists for the duration of the session. On-disk stores retain data across sessions.
- **Configuration**: via environment variables (see section 5)

## 3. Tool Interface

All three implementations expose the following tools with identical names, descriptions, and input schemas.

### 3.1 `sparql_query`

Execute a read-only SPARQL query (SELECT, CONSTRUCT, ASK, DESCRIBE).

**Input:**
| Parameter | Type | Required | Description |
|---|---|---|---|
| `query` | string | yes | SPARQL query string |
| `default_graph` | string | no | URI of the default graph to query against |

**Output:**
- SELECT: results serialized as JSON (application/sparql-results+json)
- CONSTRUCT / DESCRIBE: results serialized as N-Triples
- ASK: `"true"` or `"false"`

**Errors:**
- Invalid SPARQL syntax → error message with parse details
- Query timeout → error message (if timeout is configured)

### 3.2 `sparql_update`

Execute a SPARQL UPDATE operation (INSERT DATA, DELETE DATA, DELETE/INSERT WHERE, LOAD, CLEAR, DROP, CREATE).

**Input:**
| Parameter | Type | Required | Description |
|---|---|---|---|
| `update` | string | yes | SPARQL Update string |

**Output:**
- Success: confirmation message with summary of operation
- Failure: error message with details

### 3.3 `load_rdf`

Load RDF data into the store from a file path or inline content.

**Input:**
| Parameter | Type | Required | Description |
|---|---|---|---|
| `input` | string | yes | File path (absolute) or inline RDF content |
| `format` | string | no | MIME type or short name. Default: auto-detect from file extension or content |
| `base_iri` | string | no | Base IRI for relative URI resolution |
| `graph` | string | no | Target named graph URI. Default: default graph |

**Supported formats:**
| Short name | MIME type | File extensions |
|---|---|---|
| `turtle` | text/turtle | .ttl |
| `ntriples` | application/n-triples | .nt |
| `nquads` | application/n-quads | .nq |
| `trig` | application/trig | .trig |
| `rdfxml` | application/rdf+xml | .rdf, .xml |
| `n3` | text/n3 | .n3 |

**Behavior:**
- If `input` is an existing file path, read and parse the file
- Otherwise, treat `input` as inline RDF content
- Format auto-detection: by file extension if path, fallback to Turtle if inline

**Output:**
- Success: number of triples/quads loaded
- Failure: parse error with line/column information

### 3.4 `list_graphs`

List all named graphs in the store.

**Input:** none

**Output:**
- JSON array of graph URIs
- Always includes `"default"` for the default graph if it contains triples

## 4. Project Structure

```
oxigraph-code/
├── PLAN.md
├── TASKS.md
├── SPECIFICATIONS.md
├── README.md
├── .gitignore
│
├── python/
│   ├── pyproject.toml
│   └── src/
│       └── oxigraph_mcp/
│           ├── __init__.py
│           └── server.py
│
├── ts/
│   ├── package.json
│   ├── tsconfig.json
│   └── src/
│       └── index.ts
│
└── rust/
    ├── Cargo.toml
    └── src/
        └── main.rs
```

## 5. Configuration

All implementations read the same environment variables:

| Variable | Default | Description |
|---|---|---|
| `OXIGRAPH_STORE_PATH` | `./oxigraph_data` | Path to the on-disk store directory (ignored by TS/WASM which is always in-memory) |

## 6. Claude Code Integration

Each implementation is registered as an MCP server in Claude Code's configuration (`~/.claude.json` or project-level `.mcp.json`):

**Python:**
```json
{
  "mcpServers": {
    "oxigraph": {
      "command": "python",
      "args": ["-m", "oxigraph_mcp.server"],
      "cwd": "<project>/python",
      "env": {
        "OXIGRAPH_STORE_PATH": "/path/to/store"
      }
    }
  }
}
```

**TypeScript:**
```json
{
  "mcpServers": {
    "oxigraph": {
      "command": "npx",
      "args": ["tsx", "src/index.ts"],
      "cwd": "<project>/ts",
      "env": {}
    }
  }
}
```

**Rust:**
```json
{
  "mcpServers": {
    "oxigraph": {
      "command": "<project>/rust/target/release/oxigraph-mcp",
      "env": {
        "OXIGRAPH_STORE_PATH": "/path/to/store"
      }
    }
  }
}
```

## 7. Error Handling

All tools follow the MCP error convention:
- Tool execution errors return `isError: true` with a descriptive text message
- SPARQL parse errors include the problematic portion of the query
- File I/O errors include the file path and OS error message
- Store errors (corruption, lock contention) are surfaced as-is from Oxigraph

## 8. Constraints and Limitations

- **TypeScript**: in-memory only — data does not persist across sessions. Suitable for small datasets and ephemeral use.
- **File loading**: only local file paths are supported. No HTTP/URL fetching (use SPARQL `LOAD <url>` via `sparql_update` for remote sources where supported).
- **Concurrency**: single-session only. The store is not shared across multiple MCP server instances. On-disk stores are locked while the server is running.
- **No authentication**: the MCP server trusts all incoming requests. It runs locally and inherits the user's file system permissions.

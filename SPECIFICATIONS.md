# Oxigraph MCP Tools — Specifications

## 1. Overview

This project provides a single MCP (Model Context Protocol) server implemented in Rust that exposes an Oxigraph RDF triplestore to Claude Code. The server offers generic RDF/SPARQL tools alongside language-specific code-loading tools that parse source code into an RDF knowledge graph, enabling an LLM coding agent to query and reason about a project's codebase.

| Component | Technology |
|---|---|
| Language | Rust 1.75+ |
| RDF Store | oxigraph 0.5.x (RocksDB on-disk) |
| MCP SDK | rmcp 0.16.x |
| Transport | stdio (JSON-RPC) |

## 2. Architecture

```
Claude Code  <──stdio──>  MCP Server (Rust)  <──native API──>  Oxigraph Store (RocksDB)
                               │
                               ├── Generic RDF tools
                               │   (sparql_query, sparql_update, load_rdf, list_graphs)
                               │
                               └── Code-loading tools
                                   ├── load_code (generic dispatcher)
                                   ├── load_rust_code
                                   ├── load_python_code
                                   └── load_ts_code
                                   │
                                   └── LanguageLoader trait (plugin system)
```

- **Transport**: stdio (stdin/stdout JSON-RPC)
- **Store lifecycle**: the store opens on server start and persists across sessions via RocksDB on-disk storage.
- **Configuration**: via environment variables (see section 6)

## 3. Generic RDF Tool Interface

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

## 4. Code-Loading Tools

### 4.1 Purpose

The code-loading tools parse source code from a project directory and represent it as RDF triples in the Oxigraph store. This enables an LLM coding agent to query structural and semantic information about a codebase using SPARQL — modules, functions, classes, imports, dependencies, call relationships, and file metadata.

### 4.2 RDF Ontology for Code Representation

The code representation builds on existing ontologies, extended as needed:

- **Base namespace**: `https://oxigraph.org/code#` (prefix `code:`)
- **Draws from**: [CodeOntology](https://codeontology.org/) and [SEON](https://se-on.org/) where applicable, with extensions for LLM-agent-oriented codebase description.

#### Core Classes

| Class | Description |
|---|---|
| `code:Project` | A software project / repository |
| `code:Module` | A module or file-level unit |
| `code:Function` | A function or method |
| `code:Class` | A class or struct |
| `code:Trait` | A trait or interface |
| `code:Enum` | An enumeration type |
| `code:Import` | An import/use statement |
| `code:Dependency` | An external dependency (from package manifest) |

#### Core Properties

| Property | Domain | Range | Description |
|---|---|---|---|
| `code:name` | any | xsd:string | Identifier name |
| `code:filePath` | `code:Module` | xsd:string | Absolute file path |
| `code:relativePath` | `code:Module` | xsd:string | Path relative to project root |
| `code:startLine` | any | xsd:integer | Start line number |
| `code:endLine` | any | xsd:integer | End line number |
| `code:definedIn` | any | `code:Module` | Module containing this definition |
| `code:hasFunction` | `code:Module`/`code:Class` | `code:Function` | Contains function/method |
| `code:hasClass` | `code:Module` | `code:Class` | Contains class/struct |
| `code:hasTrait` | `code:Module` | `code:Trait` | Contains trait/interface |
| `code:imports` | `code:Module` | `code:Import` | Import statement |
| `code:importPath` | `code:Import` | xsd:string | What is being imported |
| `code:calls` | `code:Function` | `code:Function` | Function call relationship |
| `code:parameter` | `code:Function` | xsd:string | Parameter name |
| `code:returnType` | `code:Function` | xsd:string | Return type annotation |
| `code:visibility` | any | xsd:string | Visibility modifier (public, private, etc.) |
| `code:docstring` | any | xsd:string | Documentation string |
| `code:dependsOn` | `code:Project` | `code:Dependency` | External dependency |
| `code:version` | `code:Dependency` | xsd:string | Dependency version |
| `code:language` | `code:Module` | xsd:string | Programming language |

### 4.3 `load_code` (Generic Dispatcher)

Load source code from a project directory into the RDF store, auto-detecting or explicitly specifying the language.

**Input:**
| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Path to a file or project directory |
| `language` | string | no | Language hint: `rust`, `python`, `typescript`. Default: auto-detect |
| `graph` | string | no | Target named graph URI. Default: `code:<language>` |

**Behavior:**
- If `path` is a directory, recursively discover source files for the specified (or detected) language
- Delegates to the appropriate `LanguageLoader` implementation
- Respects `.gitignore` and common ignore patterns (e.g., `target/`, `node_modules/`, `__pycache__/`)

**Output:**
- Success: summary of entities loaded (files, functions, classes, etc.)
- Failure: parse errors with file path and line information

### 4.4 `load_rust_code`

Load Rust source code into the RDF store.

**Input:**
| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Path to a `.rs` file, a directory, or a Cargo workspace root |
| `graph` | string | no | Target named graph URI. Default: `code:rust` |

**Rust-specific behavior:**
- Parses `Cargo.toml` for project metadata and dependencies
- Parses `.rs` files using `syn` (or equivalent) for AST extraction
- Extracts: modules, functions, structs, enums, traits, impls, use statements, visibility, doc comments
- Resolves module hierarchy (`mod` declarations, file structure)

### 4.5 `load_python_code`

Load Python source code into the RDF store.

**Input:**
| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Path to a `.py` file, a directory, or a project root with `pyproject.toml` |
| `graph` | string | no | Target named graph URI. Default: `code:python` |

**Python-specific behavior:**
- Parses `pyproject.toml` / `setup.py` / `requirements.txt` for dependencies
- Parses `.py` files for AST extraction (using a Rust-based Python parser such as `ruff_python_ast` or `tree-sitter-python`)
- Extracts: modules, functions, classes, decorators, imports, type annotations, docstrings

### 4.6 `load_ts_code`

Load TypeScript/JavaScript source code into the RDF store.

**Input:**
| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Path to a `.ts`/`.js` file, a directory, or a project root with `package.json` |
| `graph` | string | no | Target named graph URI. Default: `code:typescript` |

**TypeScript-specific behavior:**
- Parses `package.json` for project metadata and dependencies
- Parses `.ts`/`.tsx`/`.js`/`.jsx` files for AST extraction (using a Rust-based parser such as `swc` or `tree-sitter-typescript`)
- Extracts: modules, functions, classes, interfaces, type aliases, imports/exports, JSDoc comments

## 5. Plugin System — LanguageLoader Trait

New language support is added by implementing the `LanguageLoader` trait:

```rust
pub trait LanguageLoader: Send + Sync {
    /// Unique identifier for this language (e.g., "rust", "python", "typescript")
    fn language_id(&self) -> &str;

    /// File extensions this loader handles (e.g., &["rs"])
    fn file_extensions(&self) -> &[&str];

    /// Parse a single source file and return RDF triples
    fn load_file(&self, path: &Path, project_root: &Path) -> Result<Vec<Triple>, LoadError>;

    /// Parse project-level metadata (package manifest, dependencies)
    fn load_project_metadata(&self, project_root: &Path) -> Result<Vec<Triple>, LoadError>;

    /// File/directory patterns to ignore
    fn ignore_patterns(&self) -> &[&str] {
        &[]
    }
}
```

- Language loaders are compiled into the binary and registered at startup.
- The generic `load_code` tool dispatches to the appropriate loader based on the `language` parameter or auto-detection from file extensions.
- Adding a new language requires implementing the trait and registering it — no changes to the MCP tool interface.

## 6. Project Structure

```
oxigraph-code/
├── PLAN.md
├── TASKS.md
├── SPECIFICATIONS.md
├── README.md
├── .gitignore
│
└── rust/
    ├── Cargo.toml
    └── src/
        ├── main.rs              # Entry point, MCP server setup
        ├── store.rs             # Oxigraph store initialization and management
        ├── tools/
        │   ├── mod.rs           # Tool registration
        │   ├── sparql.rs        # sparql_query, sparql_update
        │   ├── rdf.rs           # load_rdf, list_graphs
        │   └── code.rs          # load_code (generic dispatcher)
        └── loaders/
            ├── mod.rs           # LanguageLoader trait, registry, auto-detection
            ├── rust.rs          # Rust loader (load_rust_code)
            ├── python.rs        # Python loader (load_python_code)
            └── typescript.rs    # TypeScript loader (load_ts_code)
```

## 7. Configuration

| Variable | Default | Description |
|---|---|---|
| `OXIGRAPH_STORE_PATH` | `./oxigraph_data` | Path to the on-disk RocksDB store directory |

## 8. Claude Code Integration

Register the server in Claude Code's configuration (`~/.claude.json` or project-level `.mcp.json`):

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

## 9. Error Handling

All tools follow the MCP error convention:
- Tool execution errors return `isError: true` with a descriptive text message
- SPARQL parse errors include the problematic portion of the query
- File I/O errors include the file path and OS error message
- Code parse errors include the source file path, line number, and error details
- Store errors (corruption, lock contention) are surfaced as-is from Oxigraph

## 10. Constraints and Limitations

- **Single server**: one Rust binary serves all tools. No separate Python/TypeScript server implementations.
- **File loading**: only local file paths are supported. No HTTP/URL fetching (use SPARQL `LOAD <url>` via `sparql_update` for remote sources where supported).
- **Concurrency**: single-session only. The store is not shared across multiple MCP server instances. The on-disk store is locked while the server is running.
- **No authentication**: the MCP server trusts all incoming requests. It runs locally and inherits the user's file system permissions.
- **Code parsing fidelity**: AST extraction is best-effort. Macros, metaprogramming, and dynamic constructs may not be fully represented. The goal is to capture the structural information most useful to an LLM coding agent, not a complete compiler-grade AST.

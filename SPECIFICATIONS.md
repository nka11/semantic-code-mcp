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
                               ├── Code-loading tools
                               │   ├── load_code (generic dispatcher)
                               │   ├── load_rust_code
                               │   ├── load_python_code
                               │   └── load_ts_code
                               │   │
                               │   └── LanguageLoader trait (plugin system)
                               │
                               └── Git history tools
                                   └── load_git_history
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

**Single graph model:** All loaders (code and git) write triples into the **default graph**. This avoids the complexity of cross-graph queries and allows simple SPARQL patterns to join code structure with git history. Entities from different languages are distinguished by the `code:language` property on `code:Module` and `code:Project` nodes. The generic `load_rdf` tool retains its optional `graph` parameter for user-managed RDF data.

### 4.2 RDF Ontology for Code Representation

The code representation builds on existing ontologies, extended as needed:

- **Base namespace**: `https://ds-labs.org/code#` (prefix `code:`)
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
| `code:hasMethod` | `code:Trait` | xsd:string | Trait method name |
| `code:hasField` | `code:Class` | xsd:string | Struct field name |
| `code:hasVariant` | `code:Enum` | xsd:string | Enum variant name |
| `code:hasModule` | `code:Module` | `code:Module` | Contains submodule (mod declaration) |
| `code:hasImport` | `code:Module` | `code:Import` | Import statement |
| `code:hasDependency` | `code:Project` | `code:Dependency` | External dependency |
| `code:importPath` | `code:Import` | xsd:string | What is being imported |
| `code:implements` | `code:Class` | xsd:string | Trait implemented by this type |
| `code:edition` | `code:Project` | xsd:string | Language edition (e.g., "2021") |
| `code:calls` | `code:Function` | `code:Function` | Function call relationship (planned, not yet implemented) |
| `code:parameter` | `code:Function` | xsd:string | Parameter name |
| `code:returnType` | `code:Function` | xsd:string | Return type annotation |
| `code:visibility` | any | xsd:string | Visibility modifier (public, private, etc.) |
| `code:docstring` | any | xsd:string | Documentation string |
| `code:version` | `code:Project`/`code:Dependency` | xsd:string | Version string |
| `code:language` | `code:Module` | xsd:string | Programming language |

#### Git History Classes

| Class | Description |
|---|---|
| `code:Commit` | A git commit |
| `code:FileChange` | A file modification within a commit |

#### Git History Properties

| Property | Domain | Range | Description |
|---|---|---|---|
| `code:commitHash` | `code:Commit` | xsd:string | Full SHA-1 hash |
| `code:shortHash` | `code:Commit` | xsd:string | Abbreviated hash (7 chars) |
| `code:authorName` | `code:Commit` | xsd:string | Author name |
| `code:authorEmail` | `code:Commit` | xsd:string | Author email |
| `code:committerName` | `code:Commit` | xsd:string | Committer name |
| `code:committerEmail` | `code:Commit` | xsd:string | Committer email |
| `code:commitDate` | `code:Commit` | xsd:dateTime | Commit timestamp (ISO 8601) |
| `code:message` | `code:Commit` | xsd:string | Full commit message |
| `code:parentCommit` | `code:Commit` | `code:Commit` | Parent commit (multiple for merges) |
| `code:hasChange` | `code:Commit` | `code:FileChange` | File change within this commit |
| `code:changeType` | `code:FileChange` | xsd:string | One of: "added", "modified", "deleted", "renamed" |
| `code:filePath` | `code:FileChange` | xsd:string | Path of the changed file (relative to repo root) |
| `code:oldFilePath` | `code:FileChange` | xsd:string | Previous path (for renames only) |
| `code:affectsModule` | `code:FileChange` | `code:Module` | Links file change to a loaded code Module (if loaded) |

### 4.3 `load_code` (Generic Dispatcher)

Load source code from a project directory into the RDF store, auto-detecting or explicitly specifying the language.

**Input:**
| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Path to a file or project directory |
| `language` | string | no | Language hint. Currently supported: `rust`. Default: auto-detect from project markers |

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

**TypeScript-specific behavior:**
- Parses `package.json` for project metadata and dependencies
- Parses `.ts`/`.tsx`/`.js`/`.jsx` files for AST extraction (using a Rust-based parser such as `swc` or `tree-sitter-typescript`)
- Extracts: modules, functions, classes, interfaces, type aliases, imports/exports, JSDoc comments

### 4.7 `load_git_history`

Load git commit history into the RDF store from a git repository.

**Input:**
| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Path to a git repository (must contain a `.git` directory) |
| `max_commits` | integer | no | Maximum number of commits to load. Default: 500 |
| `branch` | string | no | Branch or ref to walk. Default: `HEAD` |

**Git-specific behavior:**
- Walks the commit graph starting from the specified branch/ref
- Extracts commit metadata: hash, author, committer, date, message, parent(s)
- Extracts per-commit file changes via diff-tree: added, modified, deleted, renamed files
- Each commit is a `code:Commit` node; each file change is a `code:FileChange` node linked to the commit
- File changes are linked to `code:Module` nodes (via `code:affectsModule`) when a corresponding module has been loaded by a code loader — since all data lives in the default graph, simple joins connect git history with code structure
- Commit URIs use the short hash: `code:commit/<short_hash>` (e.g., `code:commit/4ad47e6`)
- FileChange URIs: `code:commit/<short_hash>/<relative_path>` (e.g., `code:commit/4ad47e6/src/main.rs`)
- The `code:Project` node (if present from a code loader) is linked to commits via `code:hasCommit`

**Implementation approach:**
- Uses `git2` crate (libgit2 bindings) for repository access — no shelling out to `git` CLI
- Not a `LanguageLoader` — this is a standalone tool in `tools/git.rs` with its own loader in `loaders/git.rs`
- Pure sync functions consistent with other tool implementations

**Example SPARQL queries after loading:**
```sparql
# Find recent commits that modified a specific file
PREFIX code: <https://ds-labs.org/code#>
SELECT ?hash ?msg ?date WHERE {
  ?c a code:Commit ; code:shortHash ?hash ; code:message ?msg ; code:commitDate ?date ;
     code:hasChange ?ch .
  ?ch code:filePath "src/main.rs" .
} ORDER BY DESC(?date) LIMIT 10

# Find all files changed in a commit
PREFIX code: <https://ds-labs.org/code#>
SELECT ?path ?type WHERE {
  ?c a code:Commit ; code:shortHash "4ad47e6" ; code:hasChange ?ch .
  ?ch code:filePath ?path ; code:changeType ?type .
}

# Find commits that touched functions in a module (single-graph join)
PREFIX code: <https://ds-labs.org/code#>
SELECT ?hash ?msg ?fname WHERE {
  ?c a code:Commit ; code:shortHash ?hash ; code:message ?msg ; code:hasChange ?ch .
  ?ch code:affectsModule ?mod .
  ?mod a code:Module ; code:hasFunction ?f .
  ?f code:name ?fname .
}
```

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
        │   ├── code.rs          # load_code (generic dispatcher)
        │   └── git.rs           # load_git_history
        └── loaders/
            ├── mod.rs           # LanguageLoader trait, registry, auto-detection
            ├── rust.rs          # Rust loader (load_rust_code)
            ├── python.rs        # Python loader (load_python_code)
            ├── typescript.rs    # TypeScript loader (load_ts_code)
            └── git.rs           # Git history loader (commit graph, file changes)
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
- **No OWL DL reasoner**: full OWL Description Logic reasoning is out of scope.
- **No heavy ML inference**: embedding generation is delegated to external APIs or lightweight local models, not run inside Rust.

---

## 11. RAG Data Model

### 11.1 RDF Canonicalization for Embedding

Chunk sources:
- Turtle / RDF/XML files
- SPARQL CONSTRUCT results
- Code comments linked to IRIs

Chunk schema:

```rust
struct RagChunk {
    id: String,
    iri: Option<String>,
    text: String,
    graph: Option<String>,
    embedding: Vec<f32>,
    metadata: HashMap<String, String>,
}
```

### 11.2 Canonicalization Rules

- Expand CURIEs to full IRIs
- Deterministic predicate ordering
- Collapse blank nodes
- Optional SHACL summaries

### 11.3 Search Result Model

```rust
pub struct SearchHit {
    pub id: String,
    pub score: f32,
    pub text: String,
    pub metadata: HashMap<String, String>,
}

pub enum Filter {
    Graph(String),
    IriPrefix(String),
    MetadataEq(String, String),
}
```

---

## 12. Pluggable Vector DB Interface

All vector backends implement a common async trait:

```rust
#[async_trait::async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert(&self, chunks: Vec<RagChunk>) -> anyhow::Result<()>;
    async fn delete(&self, ids: &[String]) -> anyhow::Result<()>;
    async fn search(&self, query: &[f32], k: usize, filter: Option<Filter>)
        -> anyhow::Result<Vec<SearchHit>>;
}
```

This trait enables swapping backends without changing the RAG pipeline or agent code.

---

## 13. Default In-Memory Vector Store

Recommended crates:
- `hnsw_rs` — ANN (approximate nearest neighbor) index
- `ndarray` — vector operations
- `dashmap` — concurrent hash map

```rust
pub struct InMemoryVectorStore {
    index: hnsw_rs::Hnsw<f32, DistCosine>,
    data: dashmap::DashMap<String, RagChunk>,
}
```

Properties:
| Property | Value |
|---|---|
| Persistence | Optional snapshot to disk |
| Concurrency | Thread-safe via `DashMap` |
| Scale | ~100k–500k chunks |

---

## 14. External Vector DB Adapters

All adapters implement the `VectorStore` trait from §12.

```rust
enum VectorBackend {
    InMemory,
    Qdrant { url: String, collection: String },
    Milvus { addr: String, collection: String },
}
```

Each adapter is gated behind a Cargo feature flag (e.g., `qdrant`, `milvus`) so the default binary has no external vector DB dependencies.

---

## 15. RAG Retrieval Pipeline

```
User Query
→ Embed(query)
→ VectorStore.search(k=20, filter)
→ Rerank (optional)
→ Context compression
→ LLM prompt
```

Steps:
1. **Embedding**: user query is embedded via the configured embedding provider (API call or local model).
2. **Retrieval**: top-k chunks retrieved from the configured `VectorStore` backend, optionally filtered by graph or IRI prefix.
3. **Reranking** (optional): re-score retrieved chunks for relevance.
4. **Context compression**: trim / summarize chunks to fit the LLM context window.
5. **Prompt assembly**: inject compressed context into the LLM prompt with chunk ID and IRI citations.

---

## 16. Agent Tools (Extended)

The agent orchestrator dispatches to tools via a common trait:

```rust
#[async_trait::async_trait]
pub trait AgentTool {
    async fn call(&self, input: ToolInput) -> anyhow::Result<ToolOutput>;
}

struct SparqlTool { store: oxigraph::store::Store }
struct RagTool { vectors: Arc<dyn VectorStore> }
struct CodegenTool { llm: LlmClient }
```

The existing MCP tools (`sparql_query`, `sparql_update`, `load_rdf`, `list_graphs`, `load_code`, `load_git_history`) continue to operate as pure sync functions. The `AgentTool` trait is an **additional** abstraction for the agent orchestrator layer.

High-level agent architecture:

```
User Input
→ Agent Orchestrator (Planner/Router)
→ SPARQL Tool (Oxigraph)
→ RAG Tool
→ Vector Store Backend (InMemory | Qdrant | Milvus | Weaviate)
```

---

## 17. Prompt Contract

- Must cite RDF IRIs for semantic answers
- Must cite chunk IDs for RAG-retrieved context
- Must refuse to answer if the response cannot be grounded in retrieved data
- SPARQL verification can be used to cross-check LLM claims against the triplestore

---

## 18. Configuration (Extended)

The existing `OXIGRAPH_STORE_PATH` environment variable remains. Additional configuration is provided via a TOML config file:

```toml
[rag]
backend = "inmemory"          # "inmemory" | "qdrant" | "milvus"
embedding_dim = 1536
top_k = 6

[rag.qdrant]
url = "http://localhost:6334"
collection = "rdf_chunks"

[rag.milvus]
addr = "http://localhost:19530"
collection = "rdf_chunks"

[oxigraph]
path = "./data/oxigraph"

[security]
pii_redaction = true
tenant_isolation = true
```

---

## 19. Observability & Evaluation

### 19.1 Tracing

Each request should propagate:
- `request_id` — unique identifier for the request
- `retrieved_chunk_ids` — IDs of chunks returned by RAG retrieval
- `sparql_queries` — SPARQL queries executed during the request

Uses `tracing` crate spans and fields, compatible with the existing `tracing-subscriber` setup.

### 19.2 Evaluation Metrics

| Metric | Description |
|---|---|
| Precision@K | Fraction of retrieved chunks that are relevant |
| Faithfulness | Whether the LLM response is faithful to retrieved context |
| Latency | End-to-end response time |

---

## 20. Performance Targets

| Metric | Target |
|---|---|
| Retrieval latency | < 20 ms (in-memory) |
| Recall@10 | > 0.9 |
| Cold start (100k chunks) | < 5 sec |
| Memory | < 2 GB |

---

## 21. Security & Multi-Tenancy

- **Namespace isolation**: per-tenant namespace separation
- **Graph-level ACL filtering**: restrict query results by named graph access
- **Redaction before embedding**: PII/sensitive data redacted prior to vectorization
- **Hash-based deduplication**: avoid redundant chunks via content hashing

---

## 22. Failure Modes

| Failure | Mitigation |
|---|---|
| Hallucinations | SPARQL verification against triplestore |
| Wrong context retrieved | Reranking pass to improve precision |
| Stale embeddings | Re-index pipeline triggered by data changes |
| Vector DB outage | Fallback to in-memory vector store |

---

## 23. Project Structure (Extended)

When the RAG and agent capabilities are implemented, the project will be organized as a Cargo workspace with multiple crates:

```
semantic-code-mcp/
├── PLAN.md
├── TASKS.md
├── SPECIFICATIONS.md
├── README.md
├── .gitignore
│
└── rust/
    ├── Cargo.toml                  # Workspace root
    │
    ├── crates/
    │   ├── vector_store/           # VectorStore trait + InMemoryVectorStore
    │   │   ├── Cargo.toml
    │   │   └── src/
    │   │       ├── lib.rs
    │   │       ├── inmemory.rs
    │   │       ├── qdrant.rs       # (feature-gated)
    │   │       └── milvus.rs       # (feature-gated)
    │   │
    │   ├── rag_pipeline/           # Embedding, retrieval, reranking, compression
    │   │   ├── Cargo.toml
    │   │   └── src/
    │   │       └── lib.rs
    │   │
    │   └── agent_orchestrator/     # Planner, router, AgentTool dispatch
    │       ├── Cargo.toml
    │       └── src/
    │           └── lib.rs
    │
    └── src/                        # Existing MCP server binary
        ├── main.rs
        ├── store.rs
        ├── tools/
        │   ├── mod.rs
        │   ├── sparql.rs
        │   ├── rdf.rs
        │   ├── code.rs
        │   └── git.rs
        └── loaders/
            ├── mod.rs
            ├── rust.rs
            ├── typescript.rs
            └── git.rs
```

## 24. Roadmap

Future enhancements beyond the current milestones:

- Hybrid lexical + vector retrieval
- SHACL-aware scoring
- Multi-vector per RDF node
- Incremental embeddings
- WASM reranker

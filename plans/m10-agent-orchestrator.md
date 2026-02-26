# M10 — Agent Orchestrator

## Context

M9 delivered the RAG pipeline (`rag_pipeline` crate) with embedding, canonicalization, reranking, and context compression. M10 builds the agent orchestrator layer on top: a planner/router that dispatches to SPARQL, RAG, and codegen tools, then assembles grounded responses with IRI and chunk ID citations.

## File Structure

```
rust/
├── Cargo.toml                              # MODIFY — add crates/agent_orchestrator to workspace members
└── crates/
    └── agent_orchestrator/                 # NEW crate
        ├── Cargo.toml
        └── src/
            ├── lib.rs                      # Module declarations + public re-exports
            ├── types.rs                    # ToolInput, ToolOutput, AgentTool trait
            ├── sparql_tool.rs              # SparqlTool — wraps oxigraph Store for SPARQL queries
            ├── rag_tool.rs                 # RagTool — wraps RagPipeline for retrieval
            ├── codegen_tool.rs             # CodegenTool — wraps LlmClient trait for generation
            ├── router.rs                   # AgentRouter — plans and dispatches to tools
            └── prompt_contract.rs          # Citation validation and prompt assembly
```

## Dependencies

`crates/agent_orchestrator/Cargo.toml`:
- `rag_pipeline = { path = "../rag_pipeline" }`
- `vector_store = { path = "../vector_store" }`
- `oxigraph = "0.5"`
- `sparesults = "0.3"`
- `async-trait = "0.1"`, `anyhow = "1"`, `tokio = { version = "1", features = ["rt"] }`, `tracing = "0.1"`, `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`
- dev: `tokio = { version = "1", features = ["full"] }`

## Implementation Tasks

### Task 1 — Scaffold crate + types.rs
Create the crate, add to workspace, define core types:
- `ToolInput` — enum with variants: `Sparql { query: String }`, `Rag { query: String, top_k: Option<usize> }`, `Codegen { prompt: String, context: String }`
- `ToolOutput` — struct with `content: String`, `citations: Vec<Citation>`
- `Citation` — enum: `Iri(String)`, `ChunkId(String)`
- `AgentTool` async trait with `fn name()` and `async fn call(ToolInput) -> Result<ToolOutput>`

### Task 2 — sparql_tool.rs
Implement `SparqlTool`:
- Holds `Arc<oxigraph::store::Store>`
- Executes SPARQL query via `sparesults` (reuse logic from `tools/sparql.rs`)
- Extracts IRIs from results as citations
- Returns `ToolOutput` with query results and `Citation::Iri` entries

### Task 3 — rag_tool.rs
Implement `RagTool`:
- Holds `Arc<RagPipeline>` (behind a trait or directly)
- Calls `pipeline.retrieve(query, config)`
- Returns `ToolOutput` with compressed context and `Citation::ChunkId` entries

### Task 4 — codegen_tool.rs
Implement `CodegenTool`:
- Define `LlmClient` async trait: `async fn generate(prompt: &str) -> Result<String>`
- `MockLlmClient` for testing (echoes prompt or returns canned response)
- `CodegenTool` holds `Box<dyn LlmClient>`, calls generate with prompt+context
- Returns `ToolOutput` with generated text

### Task 5 — prompt_contract.rs
Implement citation validation and prompt assembly:
- `validate_citations(output: &ToolOutput) -> bool` — checks every claim has citations
- `assemble_prompt(query: &str, context: &str, citations: &[Citation]) -> String` — builds grounded prompt with citation instructions
- `extract_citations(text: &str) -> Vec<Citation>` — parses `[iri:...]` and `[chunk:...]` patterns from response text

### Task 6 — router.rs + integration tests
Implement `AgentRouter`:
- Holds a registry of named `AgentTool` instances
- `plan(query: &str) -> Vec<ToolInput>` — determines which tools to call based on query analysis (keyword heuristics: SPARQL patterns → SparqlTool, "find/search/retrieve" → RagTool, "generate/write/create" → CodegenTool)
- `execute(query: &str) -> Result<ToolOutput>` — plans, executes tools in sequence, merges outputs, validates citations
- Integration tests with mock LLM and in-memory vector store

### Task 7 — Wire into MCP server
- Add `agent_orchestrator` dependency to root `Cargo.toml`
- Add `agent_query` MCP tool that wraps `AgentRouter::execute`
- Tool description explains the orchestrator and its capabilities

#![allow(clippy::vec_init_then_push)]

mod loaders;
mod store;
mod tools;

use agent_orchestrator::{AgentRouter, CodegenTool, MockLlmClient, RagTool, SparqlTool};
use loaders::LoaderRegistry;
use oxigraph::store::Store;
use rag_pipeline::{
    EmbeddingProvider, GraphIndexer, HttpEmbeddingProvider, MockEmbeddingProvider, RagPipeline,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use vector_store::inmemory::InMemoryVectorStore;
use vector_store::qdrant::QdrantVectorStore;
use vector_store::VectorStore;

#[derive(Debug, Deserialize, JsonSchema)]
struct SparqlQueryParams {
    /// SPARQL query string
    query: String,
    /// URI of the default graph to query against
    default_graph: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SparqlUpdateParams {
    /// SPARQL Update string
    update: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LoadRdfParams {
    /// File path (absolute) or inline RDF content
    input: String,
    /// MIME type or short name (turtle, ntriples, nquads, trig, rdfxml, n3). Default: auto-detect
    format: Option<String>,
    /// Base IRI for relative URI resolution
    base_iri: Option<String>,
    /// Target named graph URI. Default: default graph
    graph: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LoadCodeParams {
    /// Absolute path to a file or project directory
    path: String,
    /// Language identifier. Currently supported: "rust", "typescript". Default: auto-detect from project markers
    language: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LoadRustCodeParams {
    /// Absolute path to a Rust file or Cargo project directory
    path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LoadTsCodeParams {
    /// Absolute path to a TypeScript/JavaScript file or project directory
    path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LoadGitHistoryParams {
    /// Path to a git repository (must contain a .git directory)
    path: String,
    /// Maximum number of commits to load. Default: 500
    max_commits: Option<u32>,
    /// Branch or ref to walk. Default: HEAD
    branch: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AgentQueryParams {
    /// Natural language query or SPARQL query to route through the agent orchestrator.
    /// SPARQL queries (starting with SELECT, ASK, CONSTRUCT, DESCRIBE, PREFIX) are sent directly to the triplestore.
    /// Natural language queries are routed to RAG retrieval and/or LLM generation as appropriate.
    query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct IndexGraphParams {
    /// Named graph URI to index. If not set, indexes the default graph.
    graph: Option<String>,
    /// Only index subjects with IRIs starting with this prefix.
    iri_prefix: Option<String>,
    /// Embedding batch size. Default: 64.
    batch_size: Option<usize>,
}

#[derive(Clone)]
pub struct OxigraphServer {
    store: Arc<Store>,
    registry: Arc<LoaderRegistry>,
    agent_router: Arc<AgentRouter>,
    graph_indexer: Arc<GraphIndexer>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl OxigraphServer {
    pub fn new(store: Store) -> Self {
        let store = Arc::new(store);

        // Shared vector store — use Qdrant if QDRANT_URL is set, otherwise in-memory
        let vector_store: Arc<dyn VectorStore> = if let Ok(url) = std::env::var("QDRANT_URL") {
            let dim: usize = std::env::var("EMBEDDING_DIM")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1536);
            tracing::info!(url = %url, dim = dim, "Using Qdrant vector store");
            Arc::new(QdrantVectorStore::new(url, dim).expect("failed to connect to Qdrant"))
        } else {
            tracing::info!("Using in-memory vector store (set QDRANT_URL for Qdrant)");
            Arc::new(InMemoryVectorStore::new())
        };

        // Select embedder from environment
        let embedder: Arc<dyn EmbeddingProvider> = if let Ok(url) = std::env::var("EMBEDDING_URL") {
            let model = std::env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| "text-embedding-3-small".into());
            let api_key = std::env::var("EMBEDDING_API_KEY").ok();
            let dim: usize = std::env::var("EMBEDDING_DIM")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1536);
            tracing::info!(
                url = %url,
                model = %model,
                dim = dim,
                "Using HTTP embedding provider"
            );
            Arc::new(HttpEmbeddingProvider::new(url, model, api_key, dim))
        } else {
            tracing::info!("Using mock embedding provider (set EMBEDDING_URL for real embeddings)");
            Arc::new(MockEmbeddingProvider::new(1536))
        };

        // Build RAG pipeline with shared state
        let pipeline = Arc::new(RagPipeline::with_shared_defaults(
            vector_store.clone(),
            embedder.clone(),
        ));

        // Build graph indexer with same shared state
        let graph_indexer = Arc::new(GraphIndexer::new(embedder, vector_store));

        // Build the agent router with all available tools
        let mut agent_router = AgentRouter::new();
        agent_router.register(SparqlTool::new(store.clone()));
        agent_router.register(RagTool::new(pipeline));
        agent_router.register(CodegenTool::new(MockLlmClient));

        Self {
            store,
            registry: Arc::new(LoaderRegistry::default()),
            agent_router: Arc::new(agent_router),
            graph_indexer,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Execute a read-only SPARQL query (SELECT, CONSTRUCT, ASK, DESCRIBE). Use this instead of grep/find to search code precisely. All code and git data is in the default graph — no need to set default_graph. Prefix: PREFIX code: <https://ds-labs.org/code#>. Common patterns: find function by name: ?f a code:Function ; code:name \"foo\". Find where defined: ?f code:definedIn ?mod ; get file: ?mod code:filePath ?path. Find struct methods: ?c a code:Class ; code:name \"MyStruct\" ; code:hasFunction ?m. ?m code:name ?name. Find trait implementations: ?c code:implements \"TraitName\". List all functions in a file: ?f a code:Function ; code:definedIn ?mod. ?mod code:relativePath \"src/main.rs\". Get function signature: ?f code:parameter ?p ; code:returnType ?rt. Get line numbers: ?f code:startLine ?start ; code:endLine ?end. Find imports: ?mod code:hasImport ?imp. ?imp code:importPath ?path. Find dependencies: ?proj a code:Project ; code:hasDependency ?dep. ?dep code:name ?name ; code:version ?ver."
    )]
    async fn sparql_query(
        &self,
        Parameters(params): Parameters<SparqlQueryParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            tools::sparql::sparql_query(&store, &params.query, params.default_graph.as_deref())
        })
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("Task join error: {e}"), None))
    }

    #[tool(
        description = "Execute a SPARQL UPDATE operation (INSERT DATA, DELETE DATA, DELETE/INSERT WHERE, LOAD, CLEAR, DROP, CREATE)"
    )]
    async fn sparql_update(
        &self,
        Parameters(params): Parameters<SparqlUpdateParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || tools::sparql::sparql_update(&store, &params.update))
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(format!("Task join error: {e}"), None))
    }

    #[tool(description = "Load RDF data into the store from a file path or inline content")]
    async fn load_rdf(
        &self,
        Parameters(params): Parameters<LoadRdfParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            tools::rdf::load_rdf(
                &store,
                &params.input,
                params.format.as_deref(),
                params.base_iri.as_deref(),
                params.graph.as_deref(),
            )
        })
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("Task join error: {e}"), None))
    }

    #[tool(description = "List all named graphs in the store")]
    async fn list_graphs(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || tools::rdf::list_graphs(&store))
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(format!("Task join error: {e}"), None))
    }

    #[tool(
        description = "Load source code into the RDF store by parsing project metadata and source files. Supports auto-detection of language from project markers (Cargo.toml, package.json, tsconfig.json). Currently supports Rust and TypeScript/JavaScript. Produces RDF triples using the code: namespace (https://ds-labs.org/code#) with classes: Project, Module, Function, Class, Enum, Trait, Import, Dependency. All triples are stored in the default graph."
    )]
    async fn load_code(
        &self,
        Parameters(params): Parameters<LoadCodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let store = self.store.clone();
        let registry = self.registry.clone();
        tokio::task::spawn_blocking(move || {
            tools::code::load_code(&store, &registry, &params.path, params.language.as_deref())
        })
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("Task join error: {e}"), None))
    }

    #[tool(
        description = "Load Rust source code into the RDF store. Parses Cargo.toml for project metadata and .rs files for functions, structs, enums, traits, and impl blocks. Produces RDF triples in the code: namespace (https://ds-labs.org/code#). All triples stored in the default graph. After loading, use sparql_query to query. Classes: Project (name, version, edition, language, hasDependency, hasModule), Module (name, filePath, relativePath, hasFunction, hasImport), Function (name, visibility, parameter, returnType, startLine, endLine, definedIn, docstring), Class (structs: name, visibility, hasField, hasFunction/hasMethod, implements, startLine, endLine, definedIn, docstring), Enum (name, visibility, hasVariant, startLine, endLine, definedIn), Trait (name, visibility, hasFunction, startLine, endLine, definedIn), Import (importPath), Dependency (name, version). Entity URIs use relative paths: code:src/main.rs, code:src/main.rs/MyStruct, code:src/main.rs/my_function. Use definedIn to navigate from entity to module, filePath/relativePath to get the actual file path for reading source code."
    )]
    async fn load_rust_code(
        &self,
        Parameters(params): Parameters<LoadRustCodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let store = self.store.clone();
        let registry = self.registry.clone();
        tokio::task::spawn_blocking(move || {
            tools::code::load_rust_code(&store, &registry, &params.path)
        })
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("Task join error: {e}"), None))
    }

    #[tool(
        description = "Load TypeScript/JavaScript source code into the RDF store. Parses package.json for project metadata and .ts/.tsx/.js/.jsx files for functions, classes, interfaces, enums, type aliases, and imports. Produces RDF triples in the code: namespace (https://ds-labs.org/code#). All triples stored in the default graph. After loading, use sparql_query to query. Classes: Project (name, version, language, hasDependency), Module (name, filePath, relativePath, hasFunction, hasImport), Function (name, visibility, parameter, returnType, startLine, endLine, definedIn, docstring), Class (name, visibility, hasField, hasFunction, implements, extends, startLine, endLine, definedIn, docstring), Trait (interfaces: name, visibility, hasMethod, hasField, extends, startLine, endLine, definedIn, docstring), Enum (name, visibility, hasVariant, startLine, endLine, definedIn), Import (importPath), Dependency (name, version). Entity URIs use relative paths: code:src/index.ts, code:src/index.ts/MyClass."
    )]
    async fn load_ts_code(
        &self,
        Parameters(params): Parameters<LoadTsCodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let store = self.store.clone();
        let registry = self.registry.clone();
        tokio::task::spawn_blocking(move || {
            tools::code::load_ts_code(&store, &registry, &params.path)
        })
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("Task join error: {e}"), None))
    }

    #[tool(
        description = "Load git commit history into the RDF store from a git repository. Walks the commit graph and extracts commit metadata (hash, author, committer, date, message, parents) and per-commit file changes (added, modified, deleted, renamed). Produces RDF triples in the code: namespace (https://ds-labs.org/code#). All triples stored in the default graph. After loading, use sparql_query to query. Classes: Commit (commitHash, shortHash, authorName, authorEmail, committerName, committerEmail, commitDate, message, parentCommit, hasChange), FileChange (changeType, filePath, oldFilePath, affectsModule). Commit URIs: code:commit/<short_hash>. FileChange URIs: code:commit/<short_hash>/<relative_path>. When code has been loaded first, FileChanges are automatically linked to Module nodes via affectsModule, and the Project node is linked to commits via hasCommit."
    )]
    async fn load_git_history(
        &self,
        Parameters(params): Parameters<LoadGitHistoryParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            tools::git::load_git_history(
                &store,
                &params.path,
                params.max_commits,
                params.branch.as_deref(),
            )
        })
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("Task join error: {e}"), None))
    }

    #[tool(
        description = "Index RDF triples from the Oxigraph store into the vector store for RAG retrieval. Pipeline: SPARQL query → canonicalize triples by subject → embed text → upsert vector chunks. Run this after loading code or RDF data to enable semantic search via agent_query. Supports optional named graph and IRI prefix filtering."
    )]
    async fn index_graph(
        &self,
        Parameters(params): Parameters<IndexGraphParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let store = self.store.clone();
        let indexer = self.graph_indexer.clone();
        let graph = params.graph;
        let iri_prefix = params.iri_prefix;
        let batch_size = params.batch_size.unwrap_or(64);

        match indexer
            .index(&store, graph.as_deref(), iri_prefix.as_deref(), batch_size)
            .await
        {
            Ok(result) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Indexed {} chunks from {} subjects.",
                result.chunks_indexed, result.subjects_processed
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Indexing error: {e}"
            ))])),
        }
    }

    #[tool(
        description = "Agent orchestrator that routes queries to the appropriate tool (SPARQL, RAG, or code generation). Accepts natural language queries or direct SPARQL queries. SPARQL queries are executed directly against the triplestore. Natural language queries are routed through RAG retrieval for context, then to the LLM for grounded generation. Responses include citations: [iri:...] for RDF IRIs from SPARQL results, [chunk:...] for RAG chunk references. The agent enforces a prompt contract requiring all claims to be grounded in retrieved data."
    )]
    async fn agent_query(
        &self,
        Parameters(params): Parameters<AgentQueryParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let router = self.agent_router.clone();
        match router.execute(&params.query).await {
            Ok(output) => {
                let mut text = output.content;
                if !output.citations.is_empty() {
                    text.push_str("\n\n---\nCitations:\n");
                    for citation in &output.citations {
                        match citation {
                            agent_orchestrator::Citation::Iri(iri) => {
                                text.push_str(&format!("- [iri:{iri}]\n"));
                            }
                            agent_orchestrator::Citation::ChunkId(id) => {
                                text.push_str(&format!("- [chunk:{id}]\n"));
                            }
                        }
                    }
                }
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Agent error: {e}"
            ))])),
        }
    }
}

#[tool_handler]
impl ServerHandler for OxigraphServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Oxigraph MCP Server — an RDF triplestore with SPARQL query, update, RDF loading, and code loading tools."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let store = store::open_store()?;
    let server = OxigraphServer::new(store);

    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .inspect_err(|e| tracing::error!("Server error: {e}"))?;

    service.waiting().await?;
    Ok(())
}

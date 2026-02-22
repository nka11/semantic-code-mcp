mod loaders;
mod store;
mod tools;

use loaders::LoaderRegistry;
use oxigraph::store::Store;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;

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
    /// Language identifier (rust, python, typescript). Default: auto-detect
    language: Option<String>,
    /// Target named graph URI. Default: code:<language>
    graph: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LoadRustCodeParams {
    /// Absolute path to a Rust file or Cargo project directory
    path: String,
    /// Target named graph URI. Default: code:rust
    graph: Option<String>,
}

#[derive(Clone)]
pub struct OxigraphServer {
    store: Arc<Store>,
    registry: Arc<LoaderRegistry>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl OxigraphServer {
    pub fn new(store: Store) -> Self {
        Self {
            store: Arc::new(store),
            registry: Arc::new(LoaderRegistry::default()),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Execute a read-only SPARQL query (SELECT, CONSTRUCT, ASK, DESCRIBE)")]
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

    #[tool(description = "Execute a SPARQL UPDATE operation (INSERT DATA, DELETE DATA, DELETE/INSERT WHERE, LOAD, CLEAR, DROP, CREATE)")]
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

    #[tool(description = "Load source code into the RDF store by parsing project metadata and source files. Supports auto-detection of language from project markers (Cargo.toml, package.json, etc.)")]
    async fn load_code(
        &self,
        Parameters(params): Parameters<LoadCodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let store = self.store.clone();
        let registry = self.registry.clone();
        tokio::task::spawn_blocking(move || {
            tools::code::load_code(
                &store,
                &registry,
                &params.path,
                params.language.as_deref(),
                params.graph.as_deref(),
            )
        })
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("Task join error: {e}"), None))
    }

    #[tool(description = "Load Rust source code into the RDF store. Parses Cargo.toml for project metadata and .rs files for functions, structs, enums, traits, and impl blocks.")]
    async fn load_rust_code(
        &self,
        Parameters(params): Parameters<LoadRustCodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let store = self.store.clone();
        let registry = self.registry.clone();
        tokio::task::spawn_blocking(move || {
            tools::code::load_rust_code(
                &store,
                &registry,
                &params.path,
                params.graph.as_deref(),
            )
        })
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("Task join error: {e}"), None))
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

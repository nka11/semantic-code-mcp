use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A citation referencing the source of a claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Citation {
    /// An RDF IRI from a SPARQL result.
    Iri(String),
    /// A RAG chunk identifier.
    ChunkId(String),
}

/// Input to an agent tool.
#[derive(Debug, Clone)]
pub enum ToolInput {
    /// Execute a SPARQL query against the triplestore.
    Sparql { query: String },
    /// Retrieve relevant chunks via the RAG pipeline.
    Rag { query: String, top_k: Option<usize> },
    /// Generate text using an LLM with provided context.
    Codegen { prompt: String, context: String },
}

/// Output from an agent tool.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// The text content of the tool's response.
    pub content: String,
    /// Citations grounding the response in source data.
    pub citations: Vec<Citation>,
}

/// Async trait for agent tools that can be dispatched by the router.
#[async_trait::async_trait]
pub trait AgentTool: Send + Sync {
    /// Returns the unique name of this tool.
    fn name(&self) -> &str;

    /// Execute the tool with the given input.
    async fn call(&self, input: ToolInput) -> Result<ToolOutput>;
}

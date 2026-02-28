pub mod inmemory;
pub mod qdrant;

use std::collections::HashMap;

/// A chunk of text with its embedding, ready for vector storage.
#[derive(Debug, Clone)]
pub struct RagChunk {
    pub id: String,
    pub iri: Option<String>,
    pub text: String,
    pub graph: Option<String>,
    pub embedding: Vec<f32>,
    pub metadata: HashMap<String, String>,
}

/// A search result returned by `VectorStore::search`.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub id: String,
    pub score: f32,
    pub text: String,
    pub metadata: HashMap<String, String>,
}

/// Post-filter predicate applied after ANN retrieval.
#[derive(Debug, Clone)]
pub enum Filter {
    Graph(String),
    IriPrefix(String),
    MetadataEq(String, String),
}

impl Filter {
    /// Returns `true` if the chunk passes this filter.
    pub fn matches(&self, chunk: &RagChunk) -> bool {
        match self {
            Filter::Graph(g) => chunk.graph.as_deref() == Some(g.as_str()),
            Filter::IriPrefix(prefix) => chunk
                .iri
                .as_deref()
                .is_some_and(|iri| iri.starts_with(prefix.as_str())),
            Filter::MetadataEq(key, value) => chunk.metadata.get(key).is_some_and(|v| v == value),
        }
    }
}

/// Async trait for pluggable vector storage backends.
#[async_trait::async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert(&self, chunks: Vec<RagChunk>) -> anyhow::Result<()>;
    async fn delete(&self, ids: &[String]) -> anyhow::Result<()>;
    async fn search(
        &self,
        query: &[f32],
        k: usize,
        filter: Option<Filter>,
    ) -> anyhow::Result<Vec<SearchHit>>;
}

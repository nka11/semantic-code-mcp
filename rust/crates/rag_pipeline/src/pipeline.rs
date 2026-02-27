use std::sync::Arc;

use anyhow::Result;
use vector_store::{Filter, SearchHit, VectorStore};

use crate::compress::{ContextCompressor, TruncatingCompressor};
use crate::embedding::EmbeddingProvider;
use crate::rerank::{PassThroughReranker, Reranker};

/// Per-call retrieval configuration.
pub struct RetrievalConfig {
    /// Maximum number of hits to retrieve from the vector store.
    pub top_k: usize,
    /// Optional filter applied during vector search.
    pub filter: Option<Filter>,
    /// Approximate token budget for compressed context.
    /// If 0, skips compression and reranking, returns raw hits.
    pub token_budget: usize,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            top_k: 20,
            filter: None,
            token_budget: 4096,
        }
    }
}

/// Result of a retrieval operation.
pub struct RetrievalResult {
    /// Raw search hits (after reranking).
    pub hits: Vec<SearchHit>,
    /// Compressed context string with citation headers.
    pub context: String,
}

/// Orchestrates the RAG pipeline: embed → search → rerank → compress.
pub struct RagPipeline {
    store: Arc<dyn VectorStore>,
    embedder: Arc<dyn EmbeddingProvider>,
    reranker: Box<dyn Reranker>,
    compressor: Box<dyn ContextCompressor>,
}

impl RagPipeline {
    /// Create a pipeline with all components specified.
    pub fn new(
        store: impl VectorStore + 'static,
        embedder: impl EmbeddingProvider + 'static,
        reranker: impl Reranker + 'static,
        compressor: impl ContextCompressor + 'static,
    ) -> Self {
        Self {
            store: Arc::new(store),
            embedder: Arc::new(embedder),
            reranker: Box::new(reranker),
            compressor: Box::new(compressor),
        }
    }

    /// Create a pipeline from shared Arc instances.
    pub fn with_shared(
        store: Arc<dyn VectorStore>,
        embedder: Arc<dyn EmbeddingProvider>,
        reranker: impl Reranker + 'static,
        compressor: impl ContextCompressor + 'static,
    ) -> Self {
        Self {
            store,
            embedder,
            reranker: Box::new(reranker),
            compressor: Box::new(compressor),
        }
    }

    /// Create a pipeline with default reranker (pass-through) and compressor (truncating).
    pub fn with_defaults(
        store: impl VectorStore + 'static,
        embedder: impl EmbeddingProvider + 'static,
    ) -> Self {
        Self::new(store, embedder, PassThroughReranker, TruncatingCompressor)
    }

    /// Create a pipeline from shared Arc instances with default reranker and compressor.
    pub fn with_shared_defaults(
        store: Arc<dyn VectorStore>,
        embedder: Arc<dyn EmbeddingProvider>,
    ) -> Self {
        Self::with_shared(store, embedder, PassThroughReranker, TruncatingCompressor)
    }

    /// Run the full retrieval pipeline: embed query → search → rerank → compress.
    pub async fn retrieve(&self, query: &str, config: &RetrievalConfig) -> Result<RetrievalResult> {
        // 1. Embed query
        let embeddings = self.embedder.embed(&[query.to_string()]).await?;
        let query_vec = embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("embedding returned no vectors"))?;

        // 2. Search
        let hits = self
            .store
            .search(&query_vec, config.top_k, config.filter.clone())
            .await?;

        // 3. If token_budget is 0, skip reranking and compression
        if config.token_budget == 0 {
            return Ok(RetrievalResult {
                hits,
                context: String::new(),
            });
        }

        // 4. Rerank
        let hits = self.reranker.rerank(query, hits).await?;

        // 5. Compress
        let context = self.compressor.compress(&hits, config.token_budget);

        Ok(RetrievalResult { hits, context })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::MockEmbeddingProvider;
    use std::collections::HashMap;
    use vector_store::inmemory::InMemoryVectorStore;
    use vector_store::RagChunk;

    async fn setup_pipeline(chunks: Vec<RagChunk>) -> RagPipeline {
        let dim = 8;
        let store = InMemoryVectorStore::new();
        if !chunks.is_empty() {
            store.upsert(chunks).await.unwrap();
        }
        let embedder = MockEmbeddingProvider::new(dim);
        RagPipeline::with_defaults(store, embedder)
    }

    fn make_chunk(id: &str, text: &str, embedding: Vec<f32>) -> RagChunk {
        RagChunk {
            id: id.into(),
            iri: Some(format!("https://ds-labs.org/code#{id}")),
            text: text.into(),
            graph: None,
            embedding,
            metadata: HashMap::new(),
        }
    }

    fn make_chunk_with_graph(id: &str, text: &str, graph: &str, embedding: Vec<f32>) -> RagChunk {
        RagChunk {
            id: id.into(),
            iri: Some(format!("https://ds-labs.org/code#{id}")),
            text: text.into(),
            graph: Some(graph.into()),
            embedding,
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn retrieve_most_similar() {
        let embedder = MockEmbeddingProvider::new(8);
        // Embed the texts to get their real embeddings
        let vecs = embedder
            .embed(&["hello world".into(), "goodbye moon".into()])
            .await
            .unwrap();

        let chunks = vec![
            make_chunk("hello", "hello world", vecs[0].clone()),
            make_chunk("goodbye", "goodbye moon", vecs[1].clone()),
        ];
        let pipeline = setup_pipeline(chunks).await;

        let config = RetrievalConfig {
            top_k: 1,
            ..Default::default()
        };
        let result = pipeline.retrieve("hello world", &config).await.unwrap();
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.hits[0].id, "hello");
    }

    #[tokio::test]
    async fn filter_restricts_by_graph() {
        let embedder = MockEmbeddingProvider::new(8);
        let vecs = embedder
            .embed(&["alpha".into(), "beta".into()])
            .await
            .unwrap();

        let chunks = vec![
            make_chunk_with_graph("a", "alpha", "graph1", vecs[0].clone()),
            make_chunk_with_graph("b", "beta", "graph2", vecs[1].clone()),
        ];
        let pipeline = setup_pipeline(chunks).await;

        let config = RetrievalConfig {
            top_k: 10,
            filter: Some(Filter::Graph("graph2".into())),
            ..Default::default()
        };
        let result = pipeline.retrieve("beta", &config).await.unwrap();
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.hits[0].id, "b");
    }

    #[tokio::test]
    async fn empty_store_returns_empty() {
        let pipeline = setup_pipeline(vec![]).await;
        let config = RetrievalConfig::default();
        let result = pipeline.retrieve("anything", &config).await.unwrap();
        assert!(result.hits.is_empty());
        assert!(result.context.is_empty());
    }

    #[tokio::test]
    async fn top_k_respected() {
        let embedder = MockEmbeddingProvider::new(8);
        let texts: Vec<String> = (0..5).map(|i| format!("chunk {i}")).collect();
        let vecs = embedder.embed(&texts).await.unwrap();

        let chunks: Vec<RagChunk> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| make_chunk(&format!("c{i}"), t, vecs[i].clone()))
            .collect();
        let pipeline = setup_pipeline(chunks).await;

        let config = RetrievalConfig {
            top_k: 2,
            ..Default::default()
        };
        let result = pipeline.retrieve("chunk 0", &config).await.unwrap();
        assert!(result.hits.len() <= 2);
    }

    #[tokio::test]
    async fn context_contains_citation_headers() {
        let embedder = MockEmbeddingProvider::new(8);
        let vecs = embedder.embed(&["test text".into()]).await.unwrap();

        let chunks = vec![make_chunk("my-chunk", "test text", vecs[0].clone())];
        let pipeline = setup_pipeline(chunks).await;

        let config = RetrievalConfig::default();
        let result = pipeline.retrieve("test text", &config).await.unwrap();
        assert!(result.context.contains("[chunk:my-chunk]"));
    }

    #[tokio::test]
    async fn custom_reranker_applied() {
        use crate::rerank::Reranker;

        /// A reranker that keeps only the first hit.
        struct FirstOnlyReranker;

        #[async_trait::async_trait]
        impl Reranker for FirstOnlyReranker {
            async fn rerank(
                &self,
                _query: &str,
                mut hits: Vec<SearchHit>,
            ) -> anyhow::Result<Vec<SearchHit>> {
                hits.truncate(1);
                Ok(hits)
            }
        }

        let store = InMemoryVectorStore::new();
        let embedder = MockEmbeddingProvider::new(8);
        let vecs = embedder.embed(&["aaa".into(), "bbb".into()]).await.unwrap();

        let chunks = vec![
            make_chunk("first", "aaa", vecs[0].clone()),
            make_chunk("second", "bbb", vecs[1].clone()),
        ];
        store.upsert(chunks).await.unwrap();

        let pipeline = RagPipeline::new(store, embedder, FirstOnlyReranker, TruncatingCompressor);

        let config = RetrievalConfig {
            top_k: 10,
            ..Default::default()
        };
        let result = pipeline.retrieve("aaa", &config).await.unwrap();
        // The custom reranker should have truncated to 1 hit
        assert_eq!(result.hits.len(), 1);
    }

    #[tokio::test]
    async fn zero_budget_skips_compression() {
        let embedder = MockEmbeddingProvider::new(8);
        let vecs = embedder.embed(&["data".into()]).await.unwrap();

        let chunks = vec![make_chunk("x", "data", vecs[0].clone())];
        let pipeline = setup_pipeline(chunks).await;

        let config = RetrievalConfig {
            top_k: 5,
            token_budget: 0,
            ..Default::default()
        };
        let result = pipeline.retrieve("data", &config).await.unwrap();
        assert!(!result.hits.is_empty());
        assert!(result.context.is_empty());
    }
}

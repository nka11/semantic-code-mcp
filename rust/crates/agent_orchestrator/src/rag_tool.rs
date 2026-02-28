use std::sync::Arc;

use anyhow::{anyhow, Result};
use rag_pipeline::{RagPipeline, RetrievalConfig};

use crate::types::{AgentTool, Citation, ToolInput, ToolOutput};

/// Agent tool that retrieves relevant context via the RAG pipeline.
pub struct RagTool {
    pipeline: Arc<RagPipeline>,
}

impl RagTool {
    pub fn new(pipeline: Arc<RagPipeline>) -> Self {
        Self { pipeline }
    }
}

#[async_trait::async_trait]
impl AgentTool for RagTool {
    fn name(&self) -> &str {
        "rag"
    }

    async fn call(&self, input: ToolInput) -> Result<ToolOutput> {
        let (query, top_k) = match input {
            ToolInput::Rag { query, top_k } => (query, top_k),
            _ => return Err(anyhow!("RagTool expects ToolInput::Rag")),
        };

        let config = RetrievalConfig {
            top_k: top_k.unwrap_or(20),
            ..Default::default()
        };

        let result = self.pipeline.retrieve(&query, &config).await?;

        let citations = result
            .hits
            .iter()
            .map(|hit| Citation::ChunkId(hit.id.clone()))
            .collect();

        Ok(ToolOutput {
            content: result.context,
            citations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rag_pipeline::{EmbeddingProvider, MockEmbeddingProvider, RagPipeline};
    use std::collections::HashMap;
    use vector_store::inmemory::InMemoryVectorStore;
    use vector_store::{RagChunk, VectorStore};

    async fn setup_rag_tool(chunks: Vec<RagChunk>) -> RagTool {
        let store = InMemoryVectorStore::new();
        if !chunks.is_empty() {
            store.upsert(chunks).await.unwrap();
        }
        let embedder = MockEmbeddingProvider::new(8);
        let pipeline = Arc::new(RagPipeline::with_defaults(store, embedder));
        RagTool::new(pipeline)
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

    #[tokio::test]
    async fn retrieves_and_returns_citations() {
        let embedder = MockEmbeddingProvider::new(8);
        let vecs = embedder
            .embed(&["hello world".into(), "goodbye moon".into()])
            .await
            .unwrap();

        let chunks = vec![
            make_chunk("hello", "hello world", vecs[0].clone()),
            make_chunk("goodbye", "goodbye moon", vecs[1].clone()),
        ];
        let tool = setup_rag_tool(chunks).await;

        let output = tool
            .call(ToolInput::Rag {
                query: "hello world".into(),
                top_k: Some(1),
            })
            .await
            .unwrap();

        assert!(!output.content.is_empty());
        assert!(output
            .citations
            .iter()
            .any(|c| matches!(c, Citation::ChunkId(id) if id == "hello")));
    }

    #[tokio::test]
    async fn empty_store_returns_empty() {
        let tool = setup_rag_tool(vec![]).await;
        let output = tool
            .call(ToolInput::Rag {
                query: "anything".into(),
                top_k: None,
            })
            .await
            .unwrap();
        assert!(output.content.is_empty());
        assert!(output.citations.is_empty());
    }

    #[tokio::test]
    async fn wrong_input_type_returns_error() {
        let tool = setup_rag_tool(vec![]).await;
        let result = tool
            .call(ToolInput::Sparql {
                query: "SELECT * WHERE {}".into(),
            })
            .await;
        assert!(result.is_err());
    }
}

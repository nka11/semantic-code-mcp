use anyhow::Result;
use vector_store::SearchHit;

/// Async trait for reranking search results.
#[async_trait::async_trait]
pub trait Reranker: Send + Sync {
    /// Rerank search hits given the original query.
    async fn rerank(&self, query: &str, hits: Vec<SearchHit>) -> Result<Vec<SearchHit>>;
}

/// A pass-through reranker that returns hits unchanged.
pub struct PassThroughReranker;

#[async_trait::async_trait]
impl Reranker for PassThroughReranker {
    async fn rerank(&self, _query: &str, hits: Vec<SearchHit>) -> Result<Vec<SearchHit>> {
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn order_preserved() {
        let reranker = PassThroughReranker;
        let hits = vec![
            SearchHit {
                id: "a".into(),
                score: 0.9,
                text: "first".into(),
                metadata: HashMap::new(),
            },
            SearchHit {
                id: "b".into(),
                score: 0.5,
                text: "second".into(),
                metadata: HashMap::new(),
            },
            SearchHit {
                id: "c".into(),
                score: 0.1,
                text: "third".into(),
                metadata: HashMap::new(),
            },
        ];
        let result = reranker.rerank("query", hits).await.unwrap();
        let ids: Vec<&str> = result.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }
}

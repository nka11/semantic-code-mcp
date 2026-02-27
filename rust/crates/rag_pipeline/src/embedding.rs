use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Async trait for embedding text into dense vectors.
#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a batch of texts into vectors.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;

    /// Returns the fixed dimension of output vectors, if known.
    fn dimension(&self) -> Option<usize>;
}

/// Deterministic hash-based embedding provider for testing.
///
/// Produces L2-normalized vectors of a fixed dimension by hashing each input
/// string. The same input always produces the same output.
pub struct MockEmbeddingProvider {
    dim: usize,
}

impl MockEmbeddingProvider {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }

    /// Deterministic hash function producing a raw (unnormalized) vector.
    fn hash_to_vec(&self, text: &str) -> Vec<f32> {
        let mut vec = vec![0.0f32; self.dim];
        // FNV-1a style mixing: each byte cascades through all dimensions.
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in text.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
            for (j, v) in vec.iter_mut().enumerate() {
                let bits = hash.wrapping_add(j as u64).wrapping_mul(0x517cc1b727220a95);
                *v += (bits as i32) as f32 * 1e-10;
            }
        }
        // L2-normalize
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut vec {
                *x /= norm;
            }
        }
        vec
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| self.hash_to_vec(t)).collect())
    }

    fn dimension(&self) -> Option<usize> {
        Some(self.dim)
    }
}

/// Embedding provider that calls an OpenAI-compatible `/v1/embeddings` endpoint.
///
/// Works with OpenAI, Ollama (`http://localhost:11434/v1/embeddings`),
/// LMStudio (`http://localhost:1234/v1/embeddings`), and other compatible APIs.
pub struct HttpEmbeddingProvider {
    client: reqwest::Client,
    url: String,
    model: String,
    api_key: Option<String>,
    dim: usize,
}

impl HttpEmbeddingProvider {
    pub fn new(url: String, model: String, api_key: Option<String>, dim: usize) -> Self {
        Self {
            client: reqwest::Client::new(),
            url,
            model,
            api_key,
            dim,
        }
    }
}

#[derive(Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[async_trait::async_trait]
impl EmbeddingProvider for HttpEmbeddingProvider {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let body = EmbeddingRequest {
            model: self.model.clone(),
            input: texts.to_vec(),
        };

        let mut req = self.client.post(&self.url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Embedding API returned {status}: {body}");
        }

        let parsed: EmbeddingResponse = resp.json().await?;

        // Validate dimensions
        for (i, d) in parsed.data.iter().enumerate() {
            if d.embedding.len() != self.dim {
                anyhow::bail!(
                    "Embedding {i} has dimension {} but expected {dim}",
                    d.embedding.len(),
                    dim = self.dim
                );
            }
        }

        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }

    fn dimension(&self) -> Option<usize> {
        Some(self.dim)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn deterministic_output() {
        let provider = MockEmbeddingProvider::new(8);
        let texts = vec!["hello world".to_string()];
        let a = provider.embed(&texts).await.unwrap();
        let b = provider.embed(&texts).await.unwrap();
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn correct_dimension() {
        let provider = MockEmbeddingProvider::new(16);
        assert_eq!(provider.dimension(), Some(16));
        let vecs = provider.embed(&vec!["test".to_string()]).await.unwrap();
        assert_eq!(vecs[0].len(), 16);
    }

    #[tokio::test]
    async fn normalized() {
        let provider = MockEmbeddingProvider::new(8);
        let vecs = provider
            .embed(&vec!["some text".to_string()])
            .await
            .unwrap();
        let norm: f32 = vecs[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "norm was {norm}");
    }

    #[tokio::test]
    async fn batch_embedding() {
        let provider = MockEmbeddingProvider::new(4);
        let texts = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let vecs = provider.embed(&texts).await.unwrap();
        assert_eq!(vecs.len(), 3);
        // Each should be different
        assert_ne!(vecs[0], vecs[1]);
        assert_ne!(vecs[1], vecs[2]);
    }

    #[test]
    fn http_provider_reports_dimension() {
        let provider = HttpEmbeddingProvider::new(
            "http://localhost:11434/v1/embeddings".into(),
            "nomic-embed-text".into(),
            None,
            768,
        );
        assert_eq!(provider.dimension(), Some(768));
    }

    #[test]
    fn http_provider_with_api_key() {
        let provider = HttpEmbeddingProvider::new(
            "https://api.openai.com/v1/embeddings".into(),
            "text-embedding-3-small".into(),
            Some("sk-test-key".into()),
            1536,
        );
        assert_eq!(provider.dimension(), Some(1536));
    }

    #[tokio::test]
    async fn http_provider_empty_batch() {
        let provider = HttpEmbeddingProvider::new(
            "http://localhost:99999/v1/embeddings".into(),
            "test".into(),
            None,
            8,
        );
        // Empty input should return immediately without HTTP call
        let result = provider.embed(&[]).await.unwrap();
        assert!(result.is_empty());
    }
}

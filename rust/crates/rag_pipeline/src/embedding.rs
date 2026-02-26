use anyhow::Result;

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
}

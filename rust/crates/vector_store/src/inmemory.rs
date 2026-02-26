use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use anndists::dist::DistCosine;
use dashmap::{DashMap, DashSet};
use hnsw_rs::prelude::*;

use crate::{Filter, RagChunk, SearchHit, VectorStore};

const HNSW_MAX_NB_CONNECTION: usize = 16;
const HNSW_MAX_ELEMENTS: usize = 100_000;
const HNSW_MAX_LAYER: usize = 16;
const HNSW_EF_CONSTRUCTION: usize = 200;

/// Over-sample factor when a filter is present.
const OVERSAMPLE_FILTERED: usize = 10;
/// Over-sample factor without filter.
const OVERSAMPLE_UNFILTERED: usize = 2;

struct Inner {
    hnsw: RwLock<Hnsw<'static, f32, DistCosine>>,
    id_to_idx: DashMap<String, usize>,
    idx_to_id: DashMap<usize, String>,
    data: DashMap<String, RagChunk>,
    deleted_indices: DashSet<usize>,
    next_idx: AtomicUsize,
    dimension: AtomicUsize,
}

/// In-memory vector store backed by an HNSW approximate nearest-neighbor index.
pub struct InMemoryVectorStore {
    inner: Arc<Inner>,
}

impl InMemoryVectorStore {
    /// Create a new empty in-memory vector store.
    pub fn new() -> Self {
        let hnsw = Hnsw::new(
            HNSW_MAX_NB_CONNECTION,
            HNSW_MAX_ELEMENTS,
            HNSW_MAX_LAYER,
            HNSW_EF_CONSTRUCTION,
            DistCosine,
        );
        Self {
            inner: Arc::new(Inner {
                hnsw: RwLock::new(hnsw),
                id_to_idx: DashMap::new(),
                idx_to_id: DashMap::new(),
                data: DashMap::new(),
                deleted_indices: DashSet::new(),
                next_idx: AtomicUsize::new(0),
                dimension: AtomicUsize::new(0),
            }),
        }
    }
}

impl Default for InMemoryVectorStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl VectorStore for InMemoryVectorStore {
    async fn upsert(&self, chunks: Vec<RagChunk>) -> anyhow::Result<()> {
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || {
            for chunk in chunks {
                let dim = chunk.embedding.len();
                if dim == 0 {
                    anyhow::bail!("embedding must not be empty");
                }

                // Enforce consistent dimensionality.
                let stored_dim = inner.dimension.load(Ordering::Relaxed);
                if stored_dim == 0 {
                    inner.dimension.store(dim, Ordering::Relaxed);
                } else if dim != stored_dim {
                    anyhow::bail!("dimension mismatch: expected {stored_dim}, got {dim}");
                }

                // If this ID already exists, soft-delete the old HNSW entry.
                if let Some((_, old_idx)) = inner.id_to_idx.remove(&chunk.id) {
                    inner.deleted_indices.insert(old_idx);
                    inner.idx_to_id.remove(&old_idx);
                }

                // Assign a new internal index.
                let idx = inner.next_idx.fetch_add(1, Ordering::Relaxed);

                // Insert into HNSW index.
                inner.hnsw.read().unwrap().insert((&chunk.embedding, idx));

                // Update maps.
                inner.id_to_idx.insert(chunk.id.clone(), idx);
                inner.idx_to_id.insert(idx, chunk.id.clone());
                inner.data.insert(chunk.id.clone(), chunk);
            }
            Ok(())
        })
        .await?
    }

    async fn delete(&self, ids: &[String]) -> anyhow::Result<()> {
        let ids = ids.to_vec();
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || {
            for id in &ids {
                if let Some((_, idx)) = inner.id_to_idx.remove(id) {
                    inner.deleted_indices.insert(idx);
                    inner.idx_to_id.remove(&idx);
                }
                inner.data.remove(id);
            }
            Ok(())
        })
        .await?
    }

    async fn search(
        &self,
        query: &[f32],
        k: usize,
        filter: Option<Filter>,
    ) -> anyhow::Result<Vec<SearchHit>> {
        if k == 0 {
            return Ok(vec![]);
        }

        let query = query.to_vec();
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || {
            let stored_dim = inner.dimension.load(Ordering::Relaxed);
            if stored_dim == 0 {
                // Empty store.
                return Ok(vec![]);
            }
            if query.len() != stored_dim {
                anyhow::bail!(
                    "query dimension mismatch: expected {stored_dim}, got {}",
                    query.len()
                );
            }

            let oversample = if filter.is_some() {
                OVERSAMPLE_FILTERED
            } else {
                OVERSAMPLE_UNFILTERED
            };
            let ef_search = (k * oversample).max(50);

            let neighbours = inner
                .hnsw
                .read()
                .unwrap()
                .search(&query, ef_search, ef_search);

            let mut hits: Vec<SearchHit> = Vec::with_capacity(k);
            for n in neighbours {
                let idx = n.d_id;
                // Skip soft-deleted entries.
                if inner.deleted_indices.contains(&idx) {
                    continue;
                }
                // Look up the chunk ID from internal index.
                let chunk_id = match inner.idx_to_id.get(&idx) {
                    Some(id) => id.clone(),
                    None => continue,
                };
                // Look up the full chunk data.
                let chunk = match inner.data.get(&chunk_id) {
                    Some(c) => c,
                    None => continue,
                };
                // Apply filter if present.
                if let Some(ref f) = filter {
                    if !f.matches(&chunk) {
                        continue;
                    }
                }
                // Convert HNSW distance to cosine similarity.
                // hnsw_rs DistCosine returns 1 - cos(a,b), so similarity = 1 - distance.
                let similarity = 1.0 - n.distance;
                hits.push(SearchHit {
                    id: chunk_id,
                    score: similarity,
                    text: chunk.text.clone(),
                    metadata: chunk.metadata.clone(),
                });
                if hits.len() >= k {
                    break;
                }
            }
            Ok(hits)
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn make_chunk(id: &str, embedding: Vec<f32>) -> RagChunk {
        RagChunk {
            id: id.to_string(),
            iri: None,
            text: format!("text for {id}"),
            graph: None,
            embedding,
            metadata: HashMap::new(),
        }
    }

    fn make_chunk_full(
        id: &str,
        embedding: Vec<f32>,
        iri: Option<&str>,
        graph: Option<&str>,
        metadata: HashMap<String, String>,
    ) -> RagChunk {
        RagChunk {
            id: id.to_string(),
            iri: iri.map(String::from),
            text: format!("text for {id}"),
            graph: graph.map(String::from),
            embedding,
            metadata,
        }
    }

    #[tokio::test]
    async fn basic_upsert_and_search() {
        let store = InMemoryVectorStore::new();
        // Three orthogonal-ish 3D vectors.
        let chunks = vec![
            make_chunk("a", vec![1.0, 0.0, 0.0]),
            make_chunk("b", vec![0.0, 1.0, 0.0]),
            make_chunk("c", vec![0.0, 0.0, 1.0]),
        ];
        store.upsert(chunks).await.unwrap();

        // Query close to "a".
        let results = store.search(&[1.0, 0.1, 0.0], 1, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a");
        assert!(results[0].score > 0.9);
    }

    #[tokio::test]
    async fn upsert_overwrites() {
        let store = InMemoryVectorStore::new();
        store
            .upsert(vec![make_chunk("x", vec![1.0, 0.0, 0.0])])
            .await
            .unwrap();

        // Overwrite "x" with a different direction.
        store
            .upsert(vec![make_chunk("x", vec![0.0, 1.0, 0.0])])
            .await
            .unwrap();

        // Search toward the new direction.
        let results = store.search(&[0.0, 1.0, 0.0], 1, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "x");
        assert!(results[0].score > 0.9);

        // Search toward the old direction — should still find "x" (it's the only entry)
        // but with low similarity since it now points in the y direction.
        let results = store.search(&[1.0, 0.0, 0.0], 1, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "x");
        assert!(results[0].score < 0.5);
    }

    #[tokio::test]
    async fn delete_removes_from_results() {
        let store = InMemoryVectorStore::new();
        store
            .upsert(vec![
                make_chunk("a", vec![1.0, 0.0, 0.0]),
                make_chunk("b", vec![0.0, 1.0, 0.0]),
            ])
            .await
            .unwrap();

        store.delete(&["a".to_string()]).await.unwrap();

        let results = store.search(&[1.0, 0.0, 0.0], 10, None).await.unwrap();
        assert!(results.iter().all(|h| h.id != "a"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "b");
    }

    #[tokio::test]
    async fn filter_graph() {
        let store = InMemoryVectorStore::new();
        store
            .upsert(vec![
                make_chunk_full("a", vec![1.0, 0.0, 0.0], None, Some("g1"), HashMap::new()),
                make_chunk_full("b", vec![0.9, 0.1, 0.0], None, Some("g2"), HashMap::new()),
            ])
            .await
            .unwrap();

        let results = store
            .search(&[1.0, 0.0, 0.0], 10, Some(Filter::Graph("g2".into())))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "b");
    }

    #[tokio::test]
    async fn filter_iri_prefix() {
        let store = InMemoryVectorStore::new();
        store
            .upsert(vec![
                make_chunk_full(
                    "a",
                    vec![1.0, 0.0, 0.0],
                    Some("https://example.org/code#Foo"),
                    None,
                    HashMap::new(),
                ),
                make_chunk_full(
                    "b",
                    vec![0.9, 0.1, 0.0],
                    Some("https://other.org/data#Bar"),
                    None,
                    HashMap::new(),
                ),
            ])
            .await
            .unwrap();

        let results = store
            .search(
                &[1.0, 0.0, 0.0],
                10,
                Some(Filter::IriPrefix("https://other.org/".into())),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "b");
    }

    #[tokio::test]
    async fn filter_metadata_eq() {
        let store = InMemoryVectorStore::new();
        let mut meta = HashMap::new();
        meta.insert("lang".to_string(), "rust".to_string());
        store
            .upsert(vec![
                make_chunk_full("a", vec![1.0, 0.0, 0.0], None, None, meta),
                make_chunk_full("b", vec![0.9, 0.1, 0.0], None, None, HashMap::new()),
            ])
            .await
            .unwrap();

        let results = store
            .search(
                &[1.0, 0.0, 0.0],
                10,
                Some(Filter::MetadataEq("lang".into(), "rust".into())),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a");
    }

    #[tokio::test]
    async fn dimension_mismatch_error() {
        let store = InMemoryVectorStore::new();
        store
            .upsert(vec![make_chunk("a", vec![1.0, 0.0, 0.0])])
            .await
            .unwrap();

        let result = store.upsert(vec![make_chunk("b", vec![1.0, 0.0])]).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("dimension mismatch"));
    }

    #[tokio::test]
    async fn empty_store_search() {
        let store = InMemoryVectorStore::new();
        let results = store.search(&[1.0, 0.0, 0.0], 5, None).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn k_larger_than_corpus() {
        let store = InMemoryVectorStore::new();
        store
            .upsert(vec![
                make_chunk("a", vec![1.0, 0.0, 0.0]),
                make_chunk("b", vec![0.0, 1.0, 0.0]),
            ])
            .await
            .unwrap();

        let results = store.search(&[1.0, 0.0, 0.0], 100, None).await.unwrap();
        assert_eq!(results.len(), 2);
    }
}

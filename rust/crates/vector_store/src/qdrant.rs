use std::collections::HashMap;

use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter, PointStruct,
    QueryPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::Qdrant;

use crate::{Filter as VsFilter, RagChunk, SearchHit, VectorStore};

const COLLECTION_NAME: &str = "rag_chunks";

/// Vector store backed by a Qdrant instance over gRPC.
pub struct QdrantVectorStore {
    client: Qdrant,
    dimension: usize,
}

impl QdrantVectorStore {
    /// Connect to a Qdrant instance.
    ///
    /// `url` should be the gRPC endpoint (e.g. `http://localhost:6334`).
    /// The collection is created automatically on the first upsert if it
    /// does not already exist.
    pub fn new(url: impl Into<String>, dimension: usize) -> anyhow::Result<Self> {
        let client = Qdrant::from_url(&url.into()).build()?;
        Ok(Self { client, dimension })
    }

    /// Ensure the collection exists, creating it if necessary.
    async fn ensure_collection(&self) -> anyhow::Result<()> {
        if !self.client.collection_exists(COLLECTION_NAME).await? {
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(COLLECTION_NAME).vectors_config(
                        VectorParamsBuilder::new(self.dimension as u64, Distance::Cosine),
                    ),
                )
                .await?;
        }
        Ok(())
    }
}

/// Extract a string from a Qdrant payload `Value`.
fn value_as_str(v: &qdrant_client::qdrant::Value) -> Option<&str> {
    match &v.kind {
        Some(qdrant_client::qdrant::value::Kind::StringValue(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// Convert our `Filter` to a Qdrant `Filter`.
fn to_qdrant_filter(f: &VsFilter) -> Filter {
    match f {
        VsFilter::Graph(g) => Filter::must([Condition::matches("graph", g.clone())]),
        VsFilter::IriPrefix(_) => {
            // Qdrant has no native prefix filter on keyword fields.
            // We skip server-side filtering and rely on client-side post-filtering.
            Filter::default()
        }
        VsFilter::MetadataEq(key, value) => {
            Filter::must([Condition::matches(format!("meta_{key}"), value.clone())])
        }
    }
}

#[async_trait::async_trait]
impl VectorStore for QdrantVectorStore {
    async fn upsert(&self, chunks: Vec<RagChunk>) -> anyhow::Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }
        self.ensure_collection().await?;

        let points: Vec<PointStruct> = chunks
            .iter()
            .map(|c| {
                let mut payload: HashMap<String, qdrant_client::qdrant::Value> = HashMap::new();
                payload.insert("text".to_string(), c.text.clone().into());
                if let Some(ref iri) = c.iri {
                    payload.insert("iri".to_string(), iri.clone().into());
                }
                if let Some(ref graph) = c.graph {
                    payload.insert("graph".to_string(), graph.clone().into());
                }
                for (k, v) in &c.metadata {
                    payload.insert(format!("meta_{k}"), v.clone().into());
                }
                PointStruct::new(c.id.clone(), c.embedding.clone(), payload)
            })
            .collect();

        self.client
            .upsert_points(UpsertPointsBuilder::new(COLLECTION_NAME, points).wait(true))
            .await?;
        Ok(())
    }

    async fn delete(&self, ids: &[String]) -> anyhow::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let point_ids: Vec<qdrant_client::qdrant::PointId> =
            ids.iter().map(|id| id.clone().into()).collect();
        self.client
            .delete_points(
                DeletePointsBuilder::new(COLLECTION_NAME)
                    .points(point_ids)
                    .wait(true),
            )
            .await?;
        Ok(())
    }

    async fn search(
        &self,
        query: &[f32],
        k: usize,
        filter: Option<VsFilter>,
    ) -> anyhow::Result<Vec<SearchHit>> {
        if k == 0 {
            return Ok(vec![]);
        }

        // For IriPrefix we need client-side post-filtering, so fetch more.
        let is_prefix_filter = matches!(&filter, Some(VsFilter::IriPrefix(_)));
        let fetch_limit = if is_prefix_filter { k * 10 } else { k };

        let mut builder = QueryPointsBuilder::new(COLLECTION_NAME)
            .query(query.to_vec())
            .limit(fetch_limit as u64)
            .with_payload(true);

        if let Some(ref f) = filter {
            if !matches!(f, VsFilter::IriPrefix(_)) {
                builder = builder.filter(to_qdrant_filter(f));
            }
        }

        let response = self.client.query(builder).await?;

        let mut hits = Vec::with_capacity(k);
        for scored in response.result {
            let id = match scored.id {
                Some(pid) => match pid.point_id_options {
                    Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u)) => u,
                    Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(n)) => n.to_string(),
                    None => continue,
                },
                None => continue,
            };

            let text = scored
                .payload
                .get("text")
                .and_then(|v| value_as_str(v))
                .unwrap_or("")
                .to_string();

            // Reconstruct metadata from meta_* keys.
            let mut metadata = HashMap::new();
            for (k, v) in &scored.payload {
                if let Some(key) = k.strip_prefix("meta_") {
                    if let Some(s) = value_as_str(v) {
                        metadata.insert(key.to_string(), s.to_string());
                    }
                }
            }

            // Post-filter for IriPrefix (Qdrant has no native prefix match).
            if let Some(VsFilter::IriPrefix(ref prefix)) = filter {
                let iri = scored
                    .payload
                    .get("iri")
                    .and_then(|v| value_as_str(v))
                    .unwrap_or("");
                if !iri.starts_with(prefix.as_str()) {
                    continue;
                }
            }

            hits.push(SearchHit {
                id,
                score: scored.score,
                text,
                metadata,
            });
            if hits.len() >= k {
                break;
            }
        }

        Ok(hits)
    }
}

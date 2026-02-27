use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;

use crate::canonicalize::{Canonicalizer, RawTriple};
use crate::embedding::EmbeddingProvider;
use vector_store::{RagChunk, VectorStore};

/// Result of an indexing operation.
#[derive(Debug)]
pub struct IndexResult {
    pub chunks_indexed: usize,
    pub subjects_processed: usize,
}

/// Reads triples from Oxigraph, canonicalizes them, embeds, and upserts into the vector store.
pub struct GraphIndexer {
    canonicalizer: Canonicalizer,
    embedder: Arc<dyn EmbeddingProvider>,
    vectors: Arc<dyn VectorStore>,
}

impl GraphIndexer {
    pub fn new(embedder: Arc<dyn EmbeddingProvider>, vectors: Arc<dyn VectorStore>) -> Self {
        Self {
            canonicalizer: Canonicalizer::new(),
            embedder,
            vectors,
        }
    }

    /// Index triples from the store into the vector store.
    ///
    /// - `graph`: Optional named graph URI. If None, queries the default graph.
    /// - `iri_prefix`: Optional filter — only index subjects whose IRI starts with this prefix.
    /// - `batch_size`: Number of subjects to embed per batch.
    pub async fn index(
        &self,
        store: &Store,
        graph: Option<&str>,
        iri_prefix: Option<&str>,
        batch_size: usize,
    ) -> Result<IndexResult> {
        // 1. Query triples from the store
        let triples = self.query_triples(store, graph, iri_prefix)?;
        if triples.is_empty() {
            return Ok(IndexResult {
                chunks_indexed: 0,
                subjects_processed: 0,
            });
        }

        // 2. Canonicalize: group by subject → deterministic text
        let canonical = self.canonicalizer.canonicalize(&triples);
        let subjects_processed = canonical.len();

        // 3. Extract rdf:type metadata per subject for chunk metadata
        let type_map = self.extract_types(&triples);

        // 4. Batch embed and upsert
        let mut chunks_indexed = 0;
        for batch in canonical.chunks(batch_size) {
            let texts: Vec<String> = batch.iter().map(|(_, text)| text.clone()).collect();
            let embeddings = self.embedder.embed(&texts).await?;

            let chunks: Vec<RagChunk> = batch
                .iter()
                .zip(embeddings.into_iter())
                .map(|((subject_iri, text), embedding)| {
                    let id = stable_id(subject_iri);
                    let mut metadata = HashMap::new();
                    if let Some(rdf_type) = type_map.get(subject_iri.as_str()) {
                        metadata.insert("rdf:type".to_string(), rdf_type.clone());
                    }

                    RagChunk {
                        id,
                        iri: Some(subject_iri.clone()),
                        text: text.clone(),
                        graph: graph.map(|g| g.to_string()),
                        embedding,
                        metadata,
                    }
                })
                .collect();

            chunks_indexed += chunks.len();
            self.vectors.upsert(chunks).await?;
        }

        Ok(IndexResult {
            chunks_indexed,
            subjects_processed,
        })
    }

    /// Build and execute a SPARQL SELECT ?s ?p ?o query with optional graph and prefix filters.
    fn query_triples(
        &self,
        store: &Store,
        graph: Option<&str>,
        iri_prefix: Option<&str>,
    ) -> Result<Vec<RawTriple>> {
        let mut query = String::new();

        query.push_str("SELECT ?s ?p ?o WHERE { ");

        if let Some(g) = graph {
            query.push_str(&format!("GRAPH <{g}> {{ ?s ?p ?o }}"));
        } else {
            query.push_str("?s ?p ?o");
        }

        if let Some(prefix) = iri_prefix {
            query.push_str(&format!(" FILTER(STRSTARTS(STR(?s), \"{prefix}\"))"));
        }

        query.push_str(" }");

        let prepared = SparqlEvaluator::new()
            .parse_query(&query)
            .map_err(|e| anyhow::anyhow!("SPARQL parse error: {e}"))?;

        let results = prepared
            .on_store(store)
            .execute()
            .map_err(|e| anyhow::anyhow!("Query execution error: {e}"))?;

        let mut triples = Vec::new();
        if let QueryResults::Solutions(solutions) = results {
            for solution in solutions {
                let s = solution.map_err(|e| anyhow::anyhow!("Solution error: {e}"))?;
                let subject = term_to_string(s.get("s"));
                let predicate = term_to_string(s.get("p"));
                let object = term_to_string(s.get("o"));
                if let (Some(subject), Some(predicate), Some(object)) = (subject, predicate, object)
                {
                    triples.push(RawTriple {
                        subject,
                        predicate,
                        object,
                    });
                }
            }
        }

        Ok(triples)
    }

    /// Extract rdf:type values for each subject.
    fn extract_types<'a>(&self, triples: &'a [RawTriple]) -> HashMap<&'a str, String> {
        let rdf_type = self.canonicalizer.expand_curie("rdf:type");
        let mut types = HashMap::new();
        for t in triples {
            if t.predicate == rdf_type {
                types.insert(t.subject.as_str(), t.object.clone());
            }
        }
        types
    }
}

/// Convert an Oxigraph term to a string representation.
fn term_to_string(term: Option<&oxigraph::model::Term>) -> Option<String> {
    use oxigraph::model::Term;
    match term? {
        Term::NamedNode(nn) => Some(nn.as_str().to_string()),
        Term::BlankNode(bn) => Some(format!("_:{}", bn.as_str())),
        Term::Literal(lit) => {
            if lit.datatype().as_str() == "http://www.w3.org/2001/XMLSchema#string"
                || lit.datatype().as_str()
                    == "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString"
            {
                Some(format!("\"{}\"", lit.value()))
            } else {
                Some(format!("\"{}\"^^{}", lit.value(), lit.datatype().as_str()))
            }
        }
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

/// Generate a stable chunk ID from a subject IRI using FNV-1a hash.
fn stable_id(iri: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in iri.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::MockEmbeddingProvider;
    use vector_store::inmemory::InMemoryVectorStore;

    fn store_with_code() -> Store {
        let store = Store::new().unwrap();
        store
            .load_from_slice(
                oxigraph::io::RdfParser::from_format(oxigraph::io::RdfFormat::Turtle),
                br#"
                    @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

                    <https://ds-labs.org/code#src/main.rs/my_func> rdf:type <https://ds-labs.org/code#Function> ;
                        <https://ds-labs.org/code#name> "my_func" ;
                        <https://ds-labs.org/code#visibility> "pub" .

                    <https://ds-labs.org/code#src/main.rs/MyStruct> rdf:type <https://ds-labs.org/code#Class> ;
                        <https://ds-labs.org/code#name> "MyStruct" ;
                        <https://ds-labs.org/code#visibility> "pub" .

                    <https://ds-labs.org/code#src/lib.rs/helper> rdf:type <https://ds-labs.org/code#Function> ;
                        <https://ds-labs.org/code#name> "helper" ;
                        <https://ds-labs.org/code#visibility> "pub(crate)" .
                "#,
            )
            .unwrap();
        store
    }

    #[tokio::test]
    async fn indexes_all_subjects() {
        let store = store_with_code();
        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbeddingProvider::new(8));
        let vectors: Arc<dyn VectorStore> = Arc::new(InMemoryVectorStore::new());
        let indexer = GraphIndexer::new(embedder.clone(), vectors.clone());

        let result = indexer.index(&store, None, None, 64).await.unwrap();
        assert_eq!(result.subjects_processed, 3);
        assert_eq!(result.chunks_indexed, 3);

        // Verify we can search
        let query_vec = embedder.embed(&["my_func".into()]).await.unwrap();
        let hits = vectors.search(&query_vec[0], 10, None).await.unwrap();
        assert_eq!(hits.len(), 3);
    }

    #[tokio::test]
    async fn iri_prefix_filter() {
        let store = store_with_code();
        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbeddingProvider::new(8));
        let vectors: Arc<dyn VectorStore> = Arc::new(InMemoryVectorStore::new());
        let indexer = GraphIndexer::new(embedder, vectors);

        let result = indexer
            .index(
                &store,
                None,
                Some("https://ds-labs.org/code#src/main.rs/"),
                64,
            )
            .await
            .unwrap();
        // Only my_func and MyStruct from src/main.rs
        assert_eq!(result.subjects_processed, 2);
        assert_eq!(result.chunks_indexed, 2);
    }

    #[tokio::test]
    async fn empty_store_returns_zero() {
        let store = Store::new().unwrap();
        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbeddingProvider::new(8));
        let vectors: Arc<dyn VectorStore> = Arc::new(InMemoryVectorStore::new());
        let indexer = GraphIndexer::new(embedder, vectors);

        let result = indexer.index(&store, None, None, 64).await.unwrap();
        assert_eq!(result.subjects_processed, 0);
        assert_eq!(result.chunks_indexed, 0);
    }

    #[tokio::test]
    async fn batching_works() {
        let store = store_with_code();
        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbeddingProvider::new(8));
        let vectors: Arc<dyn VectorStore> = Arc::new(InMemoryVectorStore::new());
        let indexer = GraphIndexer::new(embedder, vectors);

        // Batch size 1 should still work correctly
        let result = indexer.index(&store, None, None, 1).await.unwrap();
        assert_eq!(result.subjects_processed, 3);
        assert_eq!(result.chunks_indexed, 3);
    }

    #[tokio::test]
    async fn metadata_includes_rdf_type() {
        let store = store_with_code();
        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbeddingProvider::new(8));
        let vectors: Arc<dyn VectorStore> = Arc::new(InMemoryVectorStore::new());
        let indexer = GraphIndexer::new(embedder.clone(), vectors.clone());

        indexer.index(&store, None, None, 64).await.unwrap();

        // Search and verify metadata
        let query_vec = embedder.embed(&["my_func".into()]).await.unwrap();
        let hits = vectors.search(&query_vec[0], 10, None).await.unwrap();
        // At least one hit should have rdf:type metadata
        let has_type = hits.iter().any(|h| h.metadata.contains_key("rdf:type"));
        assert!(has_type, "Expected at least one hit with rdf:type metadata");
    }

    #[tokio::test]
    async fn stable_ids_are_deterministic() {
        let id1 = stable_id("https://ds-labs.org/code#src/main.rs/my_func");
        let id2 = stable_id("https://ds-labs.org/code#src/main.rs/my_func");
        assert_eq!(id1, id2);

        let id3 = stable_id("https://ds-labs.org/code#src/main.rs/other");
        assert_ne!(id1, id3);
    }

    #[tokio::test]
    async fn named_graph_indexing() {
        let store = Store::new().unwrap();
        // Insert into a named graph
        let update = r#"
            INSERT DATA {
                GRAPH <http://example.org/g1> {
                    <http://example.org/func1> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://ds-labs.org/code#Function> .
                    <http://example.org/func1> <https://ds-labs.org/code#name> "func1" .
                }
            }
        "#;
        let prepared = SparqlEvaluator::new().parse_update(update).unwrap();
        prepared.on_store(&store).execute().unwrap();

        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbeddingProvider::new(8));
        let vectors: Arc<dyn VectorStore> = Arc::new(InMemoryVectorStore::new());
        let indexer = GraphIndexer::new(embedder, vectors);

        // Should find nothing in default graph
        let result = indexer.index(&store, None, None, 64).await.unwrap();
        assert_eq!(result.chunks_indexed, 0);

        // Should find data in named graph
        let result = indexer
            .index(&store, Some("http://example.org/g1"), None, 64)
            .await
            .unwrap();
        assert_eq!(result.chunks_indexed, 1);
    }
}

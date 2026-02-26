use std::sync::Arc;

use anyhow::{anyhow, Result};
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;
use sparesults::{QueryResultsFormat, QueryResultsSerializer};

use crate::types::{AgentTool, Citation, ToolInput, ToolOutput};

/// Agent tool that executes SPARQL queries against an Oxigraph store.
pub struct SparqlTool {
    store: Arc<Store>,
}

impl SparqlTool {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    fn execute_query(&self, query: &str) -> Result<ToolOutput> {
        let prepared = SparqlEvaluator::new()
            .parse_query(query)
            .map_err(|e| anyhow!("SPARQL parse error: {e}"))?;

        let results = prepared
            .on_store(&self.store)
            .execute()
            .map_err(|e| anyhow!("Query execution error: {e}"))?;

        match results {
            QueryResults::Solutions(solutions) => {
                let variables = solutions.variables().to_vec();
                let mut buffer = Vec::new();
                let serializer = QueryResultsSerializer::from_format(QueryResultsFormat::Json);
                let mut writer = serializer
                    .serialize_solutions_to_writer(&mut buffer, variables)
                    .map_err(|e| anyhow!("Serialization error: {e}"))?;

                let mut iris = Vec::new();
                for solution in solutions {
                    let s = solution.map_err(|e| anyhow!("Solution error: {e}"))?;
                    // Extract IRI citations from bindings
                    for (_var, term) in s.iter() {
                        if let oxigraph::model::TermRef::NamedNode(nn) = term.as_ref() {
                            let iri = nn.as_str().to_string();
                            if !iris.contains(&iri) {
                                iris.push(iri);
                            }
                        }
                    }
                    writer
                        .serialize(&s)
                        .map_err(|e| anyhow!("Serialization error: {e}"))?;
                }
                writer
                    .finish()
                    .map_err(|e| anyhow!("Serialization error: {e}"))?;

                let content = String::from_utf8_lossy(&buffer).into_owned();
                let citations = iris.into_iter().map(Citation::Iri).collect();
                Ok(ToolOutput { content, citations })
            }
            QueryResults::Graph(triples) => {
                let mut buffer = Vec::new();
                let serializer =
                    oxigraph::io::RdfSerializer::from_format(oxigraph::io::RdfFormat::NTriples);
                let mut writer = serializer.for_writer(&mut buffer);
                let mut iris = Vec::new();

                for triple in triples {
                    let t = triple.map_err(|e| anyhow!("Triple error: {e}"))?;
                    // Collect subject and object IRIs
                    if let oxigraph::model::NamedOrBlankNodeRef::NamedNode(nn) = t.subject.as_ref()
                    {
                        let iri = nn.as_str().to_string();
                        if !iris.contains(&iri) {
                            iris.push(iri);
                        }
                    }
                    if let oxigraph::model::TermRef::NamedNode(nn) = t.object.as_ref() {
                        let iri = nn.as_str().to_string();
                        if !iris.contains(&iri) {
                            iris.push(iri);
                        }
                    }
                    writer
                        .serialize_triple(&t)
                        .map_err(|e| anyhow!("Serialization error: {e}"))?;
                }
                writer
                    .finish()
                    .map_err(|e| anyhow!("Serialization error: {e}"))?;

                let content = String::from_utf8_lossy(&buffer).into_owned();
                let citations = iris.into_iter().map(Citation::Iri).collect();
                Ok(ToolOutput { content, citations })
            }
            QueryResults::Boolean(value) => Ok(ToolOutput {
                content: if value { "true" } else { "false" }.to_string(),
                citations: vec![],
            }),
        }
    }
}

#[async_trait::async_trait]
impl AgentTool for SparqlTool {
    fn name(&self) -> &str {
        "sparql"
    }

    async fn call(&self, input: ToolInput) -> Result<ToolOutput> {
        match input {
            ToolInput::Sparql { query } => self.execute_query(&query),
            _ => Err(anyhow!("SparqlTool expects ToolInput::Sparql")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_data() -> Arc<Store> {
        let store = Store::new().unwrap();
        store
            .load_from_slice(
                oxigraph::io::RdfParser::from_format(oxigraph::io::RdfFormat::Turtle),
                br#"
                    @prefix ex: <http://example.org/> .
                    ex:alice ex:name "Alice" .
                    ex:bob ex:name "Bob" .
                "#,
            )
            .unwrap();
        Arc::new(store)
    }

    #[tokio::test]
    async fn select_returns_results_and_citations() {
        let store = store_with_data();
        let tool = SparqlTool::new(store);
        let output = tool
            .call(ToolInput::Sparql {
                query:
                    "SELECT ?s ?name WHERE { ?s <http://example.org/name> ?name } ORDER BY ?name"
                        .into(),
            })
            .await
            .unwrap();

        assert!(output.content.contains("Alice"));
        assert!(output.content.contains("Bob"));
        // Should have IRI citations for the subjects
        assert!(output
            .citations
            .iter()
            .any(|c| matches!(c, Citation::Iri(iri) if iri == "http://example.org/alice")));
        assert!(output
            .citations
            .iter()
            .any(|c| matches!(c, Citation::Iri(iri) if iri == "http://example.org/bob")));
    }

    #[tokio::test]
    async fn ask_returns_boolean() {
        let store = store_with_data();
        let tool = SparqlTool::new(store);
        let output = tool
            .call(ToolInput::Sparql {
                query: "ASK { <http://example.org/alice> <http://example.org/name> \"Alice\" }"
                    .into(),
            })
            .await
            .unwrap();
        assert_eq!(output.content, "true");
        assert!(output.citations.is_empty());
    }

    #[tokio::test]
    async fn invalid_query_returns_error() {
        let store = Arc::new(Store::new().unwrap());
        let tool = SparqlTool::new(store);
        let result = tool
            .call(ToolInput::Sparql {
                query: "NOT VALID".into(),
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn wrong_input_type_returns_error() {
        let store = Arc::new(Store::new().unwrap());
        let tool = SparqlTool::new(store);
        let result = tool
            .call(ToolInput::Rag {
                query: "test".into(),
                top_k: None,
            })
            .await;
        assert!(result.is_err());
    }
}

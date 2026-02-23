use oxigraph::io::{RdfFormat, RdfSerializer};
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;
use rmcp::model::{CallToolResult, Content};
use sparesults::{QueryResultsFormat, QueryResultsSerializer};

fn tool_error(msg: String) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg)])
}

pub fn sparql_query(store: &Store, query: &str, default_graph: Option<&str>) -> CallToolResult {
    let mut prepared = match SparqlEvaluator::new().parse_query(query) {
        Ok(p) => p,
        Err(e) => return tool_error(format!("SPARQL parse error: {e}")),
    };

    if let Some(graph_uri) = default_graph {
        match oxigraph::model::NamedNode::new(graph_uri) {
            Ok(nn) => {
                prepared.dataset_mut().set_default_graph(vec![nn.into()]);
            }
            Err(e) => return tool_error(format!("Invalid default_graph URI: {e}")),
        }
    }

    let results = match prepared.on_store(store).execute() {
        Ok(r) => r,
        Err(e) => return tool_error(format!("Query execution error: {e}")),
    };

    match results {
        QueryResults::Solutions(solutions) => {
            let variables = solutions.variables().to_vec();
            let mut buffer = Vec::new();
            let serializer = QueryResultsSerializer::from_format(QueryResultsFormat::Json);
            let mut writer = match serializer.serialize_solutions_to_writer(&mut buffer, variables)
            {
                Ok(w) => w,
                Err(e) => return tool_error(format!("Serialization error: {e}")),
            };
            for solution in solutions {
                match solution {
                    Ok(s) => {
                        if let Err(e) = writer.serialize(&s) {
                            return tool_error(format!("Serialization error: {e}"));
                        }
                    }
                    Err(e) => return tool_error(format!("Solution error: {e}")),
                }
            }
            if let Err(e) = writer.finish() {
                return tool_error(format!("Serialization error: {e}"));
            }
            let json = String::from_utf8_lossy(&buffer).into_owned();
            CallToolResult::success(vec![Content::text(json)])
        }
        QueryResults::Graph(triples) => {
            let mut buffer = Vec::new();
            let serializer = RdfSerializer::from_format(RdfFormat::NTriples);
            let mut writer = serializer.for_writer(&mut buffer);
            for triple in triples {
                match triple {
                    Ok(t) => {
                        if let Err(e) = writer.serialize_triple(&t) {
                            return tool_error(format!("Serialization error: {e}"));
                        }
                    }
                    Err(e) => return tool_error(format!("Triple error: {e}")),
                }
            }
            if let Err(e) = writer.finish() {
                return tool_error(format!("Serialization error: {e}"));
            }
            let nt = String::from_utf8_lossy(&buffer).into_owned();
            CallToolResult::success(vec![Content::text(nt)])
        }
        QueryResults::Boolean(value) => {
            CallToolResult::success(vec![Content::text(if value { "true" } else { "false" })])
        }
    }
}

pub fn sparql_update(store: &Store, update: &str) -> CallToolResult {
    let prepared = match SparqlEvaluator::new().parse_update(update) {
        Ok(p) => p,
        Err(e) => return tool_error(format!("SPARQL UPDATE parse error: {e}")),
    };

    if let Err(e) = prepared.on_store(store).execute() {
        return tool_error(format!("Update execution error: {e}"));
    }

    CallToolResult::success(vec![Content::text("SPARQL UPDATE executed successfully.")])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result_text(result: &CallToolResult) -> &str {
        match &result.content[0].raw {
            rmcp::model::RawContent::Text(t) => &t.text,
            _ => panic!("Expected text content"),
        }
    }

    fn is_error(result: &CallToolResult) -> bool {
        result.is_error == Some(true)
    }

    fn store_with_data() -> Store {
        let store = Store::new().unwrap();
        let ttl = r#"
            @prefix ex: <http://example.org/> .
            ex:alice ex:name "Alice" .
            ex:bob ex:name "Bob" .
        "#;
        store
            .load_from_slice(
                oxigraph::io::RdfParser::from_format(oxigraph::io::RdfFormat::Turtle),
                ttl.as_bytes(),
            )
            .unwrap();
        store
    }

    #[test]
    fn test_select_query() {
        let store = store_with_data();
        let result = sparql_query(
            &store,
            "SELECT ?s ?name WHERE { ?s <http://example.org/name> ?name } ORDER BY ?name",
            None,
        );
        assert!(!is_error(&result));
        let text = result_text(&result);
        let json: serde_json::Value = serde_json::from_str(text).unwrap();
        let bindings = json["results"]["bindings"].as_array().unwrap();
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0]["name"]["value"], "Alice");
        assert_eq!(bindings[1]["name"]["value"], "Bob");
    }

    #[test]
    fn test_ask_query_true() {
        let store = store_with_data();
        let result = sparql_query(
            &store,
            "ASK { <http://example.org/alice> <http://example.org/name> \"Alice\" }",
            None,
        );
        assert!(!is_error(&result));
        assert_eq!(result_text(&result), "true");
    }

    #[test]
    fn test_ask_query_false() {
        let store = Store::new().unwrap();
        let result = sparql_query(
            &store,
            "ASK { <http://example.org/alice> <http://example.org/name> \"Alice\" }",
            None,
        );
        assert!(!is_error(&result));
        assert_eq!(result_text(&result), "false");
    }

    #[test]
    fn test_construct_query() {
        let store = store_with_data();
        let result = sparql_query(
            &store,
            "CONSTRUCT { ?s <http://example.org/knows> ?s } WHERE { ?s <http://example.org/name> \"Alice\" }",
            None,
        );
        assert!(!is_error(&result));
        let text = result_text(&result);
        assert!(text.contains("<http://example.org/alice>"));
        assert!(text.contains("<http://example.org/knows>"));
    }

    #[test]
    fn test_query_with_default_graph() {
        let store = Store::new().unwrap();
        // Insert data into a named graph
        sparql_update(
            &store,
            "INSERT DATA { GRAPH <http://example.org/g1> { <http://example.org/alice> <http://example.org/name> \"Alice\" } }",
        );
        // Without default_graph, the named graph data is not visible
        let result = sparql_query(
            &store,
            "SELECT ?name WHERE { <http://example.org/alice> <http://example.org/name> ?name }",
            None,
        );
        assert!(!is_error(&result));
        let text = result_text(&result);
        assert!(
            !text.contains("Alice"),
            "Should not find Alice in default graph: {text}"
        );

        // With default_graph set, the named graph becomes the default
        let result = sparql_query(
            &store,
            "SELECT ?name WHERE { <http://example.org/alice> <http://example.org/name> ?name }",
            Some("http://example.org/g1"),
        );
        assert!(!is_error(&result));
        let text = result_text(&result);
        assert!(
            text.contains("Alice"),
            "Should find Alice with default_graph set: {text}"
        );
    }

    #[test]
    fn test_invalid_sparql() {
        let store = Store::new().unwrap();
        let result = sparql_query(&store, "NOT A VALID QUERY", None);
        assert!(is_error(&result));
        assert!(result_text(&result).contains("parse error"));
    }

    #[test]
    fn test_sparql_update_insert() {
        let store = Store::new().unwrap();
        let result = sparql_update(
            &store,
            "INSERT DATA { <http://example.org/alice> <http://example.org/name> \"Alice\" }",
        );
        assert!(!is_error(&result));
        assert!(result_text(&result).contains("successfully"));

        let query_result = sparql_query(
            &store,
            "ASK { <http://example.org/alice> <http://example.org/name> \"Alice\" }",
            None,
        );
        assert_eq!(result_text(&query_result), "true");
    }

    #[test]
    fn test_sparql_update_delete() {
        let store = store_with_data();
        let result = sparql_update(
            &store,
            "DELETE DATA { <http://example.org/alice> <http://example.org/name> \"Alice\" }",
        );
        assert!(!is_error(&result));

        let query_result = sparql_query(
            &store,
            "ASK { <http://example.org/alice> <http://example.org/name> \"Alice\" }",
            None,
        );
        assert_eq!(result_text(&query_result), "false");
    }

    #[test]
    fn test_sparql_update_invalid() {
        let store = Store::new().unwrap();
        let result = sparql_update(&store, "NOT A VALID UPDATE");
        assert!(is_error(&result));
        assert!(result_text(&result).contains("parse error"));
    }
}

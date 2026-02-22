use oxigraph::io::{RdfFormat, RdfParser};
use oxigraph::model::{GraphNameRef, NamedNode};
use oxigraph::store::Store;
use rmcp::model::{CallToolResult, Content};
use std::fs;
use std::io::BufReader;
use std::path::Path;

fn tool_error(msg: String) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg)])
}

fn resolve_format(fmt: &str) -> Option<RdfFormat> {
    match fmt.to_lowercase().as_str() {
        "turtle" | "ttl" => Some(RdfFormat::Turtle),
        "ntriples" | "nt" => Some(RdfFormat::NTriples),
        "nquads" | "nq" => Some(RdfFormat::NQuads),
        "trig" => Some(RdfFormat::TriG),
        "rdfxml" | "rdf/xml" => Some(RdfFormat::RdfXml),
        "n3" => Some(RdfFormat::N3),
        other => RdfFormat::from_media_type(other),
    }
}

fn format_from_extension(path: &Path) -> Option<RdfFormat> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "ttl" => Some(RdfFormat::Turtle),
        "nt" => Some(RdfFormat::NTriples),
        "nq" => Some(RdfFormat::NQuads),
        "trig" => Some(RdfFormat::TriG),
        "rdf" | "xml" => Some(RdfFormat::RdfXml),
        "n3" => Some(RdfFormat::N3),
        _ => None,
    }
}

pub fn load_rdf(
    store: &Store,
    input: &str,
    format: Option<&str>,
    base_iri: Option<&str>,
    graph: Option<&str>,
) -> CallToolResult {
    let path = Path::new(input);
    let is_file = path.is_absolute() && path.exists();

    let rdf_format = if let Some(fmt) = format {
        match resolve_format(fmt) {
            Some(f) => f,
            None => {
                return tool_error(format!(
                    "Unknown RDF format: '{fmt}'. Supported: turtle, ntriples, nquads, trig, rdfxml, n3, or a MIME type."
                ))
            }
        }
    } else if is_file {
        format_from_extension(path).unwrap_or(RdfFormat::Turtle)
    } else {
        RdfFormat::Turtle
    };

    let mut parser = RdfParser::from_format(rdf_format);
    if let Some(iri) = base_iri {
        parser = match parser.with_base_iri(iri) {
            Ok(p) => p,
            Err(e) => return tool_error(format!("Invalid base IRI: {e}")),
        };
    }
    if let Some(graph_uri) = graph {
        let named = match NamedNode::new(graph_uri) {
            Ok(n) => n,
            Err(e) => return tool_error(format!("Invalid graph URI: {e}")),
        };
        parser = parser.with_default_graph(named);
    }

    let count_before = match store.len() {
        Ok(n) => n,
        Err(e) => return tool_error(format!("Store error: {e}")),
    };

    if is_file {
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) => {
                return tool_error(format!(
                    "Cannot open file '{}': {e}",
                    path.display()
                ))
            }
        };
        let reader = BufReader::new(file);
        if let Err(e) = store.load_from_reader(parser, reader) {
            return tool_error(format!(
                "RDF parse error in '{}': {e}",
                path.display()
            ));
        }
    } else if let Err(e) = store.load_from_slice(parser, input.as_bytes()) {
        return tool_error(format!("RDF parse error: {e}"));
    }

    let count_after = match store.len() {
        Ok(n) => n,
        Err(e) => return tool_error(format!("Store error: {e}")),
    };
    let loaded = count_after.saturating_sub(count_before);

    CallToolResult::success(vec![Content::text(format!(
        "Successfully loaded {loaded} triples/quads."
    ))])
}

pub fn list_graphs(store: &Store) -> CallToolResult {
    let mut graphs: Vec<String> = Vec::new();

    let has_default = store
        .quads_for_pattern(None, None, None, Some(GraphNameRef::DefaultGraph))
        .next()
        .is_some();
    if has_default {
        graphs.push("default".to_string());
    }

    for graph in store.named_graphs() {
        match graph {
            Ok(g) => graphs.push(g.to_string()),
            Err(e) => return tool_error(format!("Error listing graphs: {e}")),
        }
    }

    let json = match serde_json::to_string_pretty(&graphs) {
        Ok(j) => j,
        Err(e) => return tool_error(format!("JSON serialization error: {e}")),
    };

    CallToolResult::success(vec![Content::text(json)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::sparql::sparql_query;

    fn result_text(result: &CallToolResult) -> &str {
        match &result.content[0].raw {
            rmcp::model::RawContent::Text(t) => &t.text,
            _ => panic!("Expected text content"),
        }
    }

    fn is_error(result: &CallToolResult) -> bool {
        result.is_error == Some(true)
    }

    #[test]
    fn test_load_inline_turtle() {
        let store = Store::new().unwrap();
        let ttl = r#"
            @prefix ex: <http://example.org/> .
            ex:alice ex:name "Alice" .
            ex:bob ex:name "Bob" .
        "#;
        let result = load_rdf(&store, ttl, None, None, None);
        assert!(!is_error(&result));
        assert!(result_text(&result).contains("2 triples"));
    }

    #[test]
    fn test_load_inline_ntriples() {
        let store = Store::new().unwrap();
        let nt = "<http://example.org/alice> <http://example.org/name> \"Alice\" .\n";
        let result = load_rdf(&store, nt, Some("ntriples"), None, None);
        assert!(!is_error(&result));
        assert!(result_text(&result).contains("1 triples"));
    }

    #[test]
    fn test_load_inline_with_base_iri() {
        let store = Store::new().unwrap();
        let ttl = "<alice> <name> \"Alice\" .";
        let result = load_rdf(
            &store,
            ttl,
            Some("turtle"),
            Some("http://example.org/"),
            None,
        );
        assert!(!is_error(&result));
        assert!(result_text(&result).contains("1 triples"));

        let query_result = sparql_query(
            &store,
            "ASK { <http://example.org/alice> <http://example.org/name> \"Alice\" }",
            None,
        );
        assert_eq!(
            match &query_result.content[0].raw {
                rmcp::model::RawContent::Text(t) => t.text.as_str(),
                _ => panic!(),
            },
            "true"
        );
    }

    #[test]
    fn test_load_into_named_graph() {
        let store = Store::new().unwrap();
        let ttl = r#"
            @prefix ex: <http://example.org/> .
            ex:alice ex:name "Alice" .
        "#;
        let result = load_rdf(
            &store,
            ttl,
            None,
            None,
            Some("http://example.org/graph1"),
        );
        assert!(!is_error(&result));

        let query_result = sparql_query(
            &store,
            "ASK { GRAPH <http://example.org/graph1> { <http://example.org/alice> <http://example.org/name> \"Alice\" } }",
            None,
        );
        assert_eq!(
            match &query_result.content[0].raw {
                rmcp::model::RawContent::Text(t) => t.text.as_str(),
                _ => panic!(),
            },
            "true"
        );
    }

    #[test]
    fn test_load_invalid_format_name() {
        let store = Store::new().unwrap();
        let result = load_rdf(&store, "data", Some("invalid"), None, None);
        assert!(is_error(&result));
        assert!(result_text(&result).contains("Unknown RDF format"));
    }

    #[test]
    fn test_load_invalid_rdf() {
        let store = Store::new().unwrap();
        let result = load_rdf(&store, "{{not valid turtle}}", Some("turtle"), None, None);
        assert!(is_error(&result));
        assert!(result_text(&result).contains("parse error"));
    }

    #[test]
    fn test_load_file_not_found() {
        let store = Store::new().unwrap();
        // Path doesn't exist, so is_file is false and input is treated as inline content.
        // The non-existent path string is not valid Turtle, so we get a parse error.
        let result = load_rdf(
            &store,
            "/nonexistent/path/to/file.ttl",
            None,
            None,
            None,
        );
        assert!(is_error(&result));
        assert!(result_text(&result).contains("parse error"));
    }

    #[test]
    fn test_list_graphs_empty() {
        let store = Store::new().unwrap();
        let result = list_graphs(&store);
        assert!(!is_error(&result));
        let text = result_text(&result);
        let graphs: Vec<String> = serde_json::from_str(text).unwrap();
        assert!(graphs.is_empty());
    }

    #[test]
    fn test_list_graphs_default_only() {
        let store = Store::new().unwrap();
        load_rdf(
            &store,
            r#"@prefix ex: <http://example.org/> . ex:a ex:b ex:c ."#,
            None,
            None,
            None,
        );
        let result = list_graphs(&store);
        assert!(!is_error(&result));
        let graphs: Vec<String> = serde_json::from_str(result_text(&result)).unwrap();
        assert_eq!(graphs, vec!["default"]);
    }

    #[test]
    fn test_list_graphs_named() {
        let store = Store::new().unwrap();
        load_rdf(
            &store,
            r#"@prefix ex: <http://example.org/> . ex:a ex:b ex:c ."#,
            None,
            None,
            Some("http://example.org/mygraph"),
        );
        let result = list_graphs(&store);
        assert!(!is_error(&result));
        let graphs: Vec<String> = serde_json::from_str(result_text(&result)).unwrap();
        assert!(graphs.contains(&"<http://example.org/mygraph>".to_string())
            || graphs.contains(&"http://example.org/mygraph".to_string()),
            "Expected graph URI in list, got: {:?}", graphs);
    }
}

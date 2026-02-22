use oxigraph::io::{RdfFormat, RdfParser};
use oxigraph::model::{GraphNameRef, NamedNode};
use oxigraph::store::Store;
use rmcp::model::CallToolResult;
use std::fs;
use std::io::BufReader;
use std::path::Path;

fn tool_error(msg: String) -> CallToolResult {
    CallToolResult {
        content: vec![rmcp::model::Content::text(msg)],
        is_error: Some(true),
        ..Default::default()
    }
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

    CallToolResult::success(vec![rmcp::model::Content::text(format!(
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

    CallToolResult::success(vec![rmcp::model::Content::text(json)])
}

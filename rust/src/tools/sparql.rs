use oxigraph::io::{RdfFormat, RdfSerializer};
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;
use rmcp::model::CallToolResult;
use sparesults::{QueryResultsFormat, QueryResultsSerializer};

fn tool_error(msg: String) -> CallToolResult {
    CallToolResult {
        content: vec![rmcp::model::Content::text(msg)],
        is_error: Some(true),
        ..Default::default()
    }
}

pub fn sparql_query(
    store: &Store,
    query: &str,
    _default_graph: Option<&str>,
) -> CallToolResult {
    let prepared = match SparqlEvaluator::new().parse_query(query) {
        Ok(p) => p,
        Err(e) => return tool_error(format!("SPARQL parse error: {e}")),
    };

    let results = match prepared.on_store(store).execute() {
        Ok(r) => r,
        Err(e) => return tool_error(format!("Query execution error: {e}")),
    };

    match results {
        QueryResults::Solutions(solutions) => {
            let variables = solutions.variables().to_vec();
            let mut buffer = Vec::new();
            let serializer =
                QueryResultsSerializer::from_format(QueryResultsFormat::Json);
            let mut writer = match serializer
                .serialize_solutions_to_writer(&mut buffer, variables)
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
            CallToolResult::success(vec![rmcp::model::Content::text(json)])
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
            CallToolResult::success(vec![rmcp::model::Content::text(nt)])
        }
        QueryResults::Boolean(value) => CallToolResult::success(vec![
            rmcp::model::Content::text(if value { "true" } else { "false" }),
        ]),
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

    CallToolResult::success(vec![rmcp::model::Content::text(
        "SPARQL UPDATE executed successfully.",
    )])
}

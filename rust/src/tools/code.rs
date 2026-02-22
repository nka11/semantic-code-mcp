use crate::loaders::{discover_files, LoaderRegistry};
use oxigraph::store::Store;
use rmcp::model::CallToolResult;

fn tool_error(msg: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![rmcp::model::Content::text(msg.to_string())])
}

pub fn load_code(
    store: &Store,
    registry: &LoaderRegistry,
    path: &str,
    language: Option<&str>,
    graph: Option<&str>,
) -> CallToolResult {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return tool_error(format!("Path does not exist: {}", path.display()));
    }

    let lang = match language {
        Some(l) => l.to_string(),
        None => match registry.detect_language(path) {
            Some(l) => l.to_string(),
            None => return tool_error("Could not auto-detect language. Please specify the 'language' parameter."),
        },
    };

    let loader = match registry.get_loader(&lang) {
        Some(l) => l,
        None => return tool_error(format!("No loader registered for language: {lang}")),
    };

    let mut all_quads = Vec::new();
    let mut errors = Vec::new();
    let mut files_loaded = 0u32;

    let project_root = if path.is_dir() { path } else { path.parent().unwrap_or(path) };

    // Load project metadata if loading a directory
    if path.is_dir() {
        match loader.load_project_metadata(project_root) {
            Ok(quads) => all_quads.extend(quads),
            Err(e) => errors.push(format!("Metadata: {e}")),
        }

        // Discover and load source files
        let files = discover_files(path, loader.file_extensions(), loader.ignore_patterns());
        for file in &files {
            match loader.load_file(file, project_root) {
                Ok(quads) => {
                    all_quads.extend(quads);
                    files_loaded += 1;
                }
                Err(e) => errors.push(format!("{}: {e}", file.display())),
            }
        }
    } else {
        // Single file
        match loader.load_file(path, project_root) {
            Ok(quads) => {
                all_quads.extend(quads);
                files_loaded += 1;
            }
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }

    // If a custom graph was specified, remap all quads to that graph
    let quads_to_insert = if let Some(graph_uri) = graph {
        match oxigraph::model::NamedNode::new(graph_uri) {
            Ok(g) => {
                let graph_name = oxigraph::model::GraphName::NamedNode(g);
                all_quads
                    .into_iter()
                    .map(|q| {
                        oxigraph::model::Quad::new(
                            q.subject,
                            q.predicate,
                            q.object,
                            graph_name.clone(),
                        )
                    })
                    .collect()
            }
            Err(_) => return tool_error(format!("Invalid graph URI: {graph_uri}")),
        }
    } else {
        all_quads
    };

    let quad_count = quads_to_insert.len();
    for quad in &quads_to_insert {
        if let Err(e) = store.insert(quad) {
            return tool_error(format!("Store insert error: {e}"));
        }
    }

    // Build summary
    let mut summary = format!(
        "Loaded {files_loaded} file(s), {quad_count} triples into graph '{}'.",
        graph.unwrap_or("code:rust")
    );

    if !errors.is_empty() {
        summary.push_str(&format!(
            "\n\nWarnings ({} file(s) failed):\n{}",
            errors.len(),
            errors.join("\n")
        ));
    }

    CallToolResult::success(vec![rmcp::model::Content::text(summary)])
}

pub fn load_rust_code(
    store: &Store,
    registry: &LoaderRegistry,
    path: &str,
    graph: Option<&str>,
) -> CallToolResult {
    load_code(store, registry, path, Some("rust"), graph)
}

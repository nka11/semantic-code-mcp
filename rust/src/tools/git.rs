use crate::loaders::code_ns;
use crate::loaders::git::load_git_history_quads;
use oxigraph::model::{GraphNameRef, NamedNodeRef, NamedOrBlankNode, Term};
use oxigraph::store::Store;
use rmcp::model::CallToolResult;

fn tool_error(msg: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![rmcp::model::Content::text(msg.to_string())])
}

pub fn load_git_history(
    store: &Store,
    path: &str,
    max_commits: Option<u32>,
    branch: Option<&str>,
) -> CallToolResult {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return tool_error(format!("Path does not exist: {}", path.display()));
    }

    let max = max_commits.unwrap_or(500);
    let result = match load_git_history_quads(path, max, branch) {
        Ok(r) => r,
        Err(e) => return tool_error(e),
    };

    let quad_count = result.quads.len();
    if let Err(e) = store.extend(result.quads) {
        return tool_error(format!("Store insert error: {e}"));
    }

    // --- Module linking ---
    // Find FileChange nodes and link them to Module nodes with matching relativePath
    let mut module_links = 0u32;
    let file_path_pred = code_ns("filePath");
    let relative_path_pred = code_ns("relativePath");
    let affects_module_pred = code_ns("affectsModule");
    let file_change_type = code_ns("FileChange");
    let rdf_type = NamedNodeRef::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#type").unwrap();
    let module_type = code_ns("Module");

    // Collect all module relativePaths -> module URIs
    let mut module_map: std::collections::HashMap<String, oxigraph::model::NamedNode> =
        std::collections::HashMap::new();
    for quad in store
        .quads_for_pattern(
            None,
            Some(relative_path_pred.as_ref()),
            None,
            Some(GraphNameRef::DefaultGraph),
        )
        .flatten()
    {
        // Verify this subject is a Module
        if let Term::Literal(lit) = &quad.object {
            if let NamedOrBlankNode::NamedNode(subj) = &quad.subject {
                let is_module = store
                    .quads_for_pattern(
                        Some(subj.as_ref().into()),
                        Some(rdf_type),
                        Some(Term::NamedNode(module_type.clone()).as_ref()),
                        Some(GraphNameRef::DefaultGraph),
                    )
                    .next()
                    .is_some();
                if is_module {
                    module_map.insert(lit.value().to_string(), subj.clone());
                }
            }
        }
    }

    // For each FileChange, check if its filePath matches a module relativePath
    for quad in store
        .quads_for_pattern(
            None,
            Some(rdf_type),
            Some(Term::NamedNode(file_change_type.clone()).as_ref()),
            Some(GraphNameRef::DefaultGraph),
        )
        .flatten()
    {
        if let NamedOrBlankNode::NamedNode(fc_node) = &quad.subject {
            // Get filePath of this FileChange
            for fp_quad in store
                .quads_for_pattern(
                    Some(fc_node.as_ref().into()),
                    Some(file_path_pred.as_ref()),
                    None,
                    Some(GraphNameRef::DefaultGraph),
                )
                .flatten()
            {
                if let Term::Literal(lit) = &fp_quad.object {
                    let file_path_val = lit.value();
                    if let Some(module_uri) = module_map.get(file_path_val) {
                        let link_quad = oxigraph::model::Quad::new(
                            fc_node.clone(),
                            affects_module_pred.clone(),
                            Term::NamedNode(module_uri.clone()),
                            oxigraph::model::GraphName::DefaultGraph,
                        );
                        if store.insert(&link_quad).is_ok() {
                            module_links += 1;
                        }
                    }
                }
            }
        }
    }

    // --- Project linking ---
    // Find the Project node and link all commits to it via hasCommit
    let mut project_links = 0u32;
    let project_type = code_ns("Project");
    let has_commit_pred = code_ns("hasCommit");
    let commit_type = code_ns("Commit");

    let project_node: Option<oxigraph::model::NamedNode> = store
        .quads_for_pattern(
            None,
            Some(rdf_type),
            Some(Term::NamedNode(project_type).as_ref()),
            Some(GraphNameRef::DefaultGraph),
        )
        .find_map(|q| {
            q.ok().and_then(|q| match q.subject {
                NamedOrBlankNode::NamedNode(n) => Some(n),
                _ => None,
            })
        });

    if let Some(proj_uri) = project_node {
        for quad in store
            .quads_for_pattern(
                None,
                Some(rdf_type),
                Some(Term::NamedNode(commit_type).as_ref()),
                Some(GraphNameRef::DefaultGraph),
            )
            .flatten()
        {
            if let NamedOrBlankNode::NamedNode(commit_node) = &quad.subject {
                let link_quad = oxigraph::model::Quad::new(
                    proj_uri.clone(),
                    has_commit_pred.clone(),
                    Term::NamedNode(commit_node.clone()),
                    oxigraph::model::GraphName::DefaultGraph,
                );
                if store.insert(&link_quad).is_ok() {
                    project_links += 1;
                }
            }
        }
    }

    let mut summary = format!(
        "Loaded {} commits, {} file changes ({} triples).",
        result.commit_count, result.change_count, quad_count
    );

    if module_links > 0 || project_links > 0 {
        summary.push_str(&format!(
            " Linked {} changes to modules, {} commits to project.",
            module_links, project_links
        ));
    }

    CallToolResult::success(vec![rmcp::model::Content::text(summary)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::sparql::sparql_query;
    use std::fs;
    use tempfile::TempDir;

    fn result_text(result: &CallToolResult) -> &str {
        match &result.content[0].raw {
            rmcp::model::RawContent::Text(t) => &t.text,
            _ => panic!("Expected text content"),
        }
    }

    fn is_error(result: &CallToolResult) -> bool {
        result.is_error == Some(true)
    }

    fn query_results(store: &Store, sparql: &str) -> String {
        let result = sparql_query(store, sparql, None);
        assert!(!is_error(&result), "Query failed: {}", result_text(&result));
        result_text(&result).to_string()
    }

    fn qq(store: &Store, select: &str, body: &str) -> String {
        let sparql =
            format!("PREFIX code: <https://ds-labs.org/code#>\n{select} WHERE {{ {body} }}");
        query_results(store, &sparql)
    }

    /// Create a test git repository with an initial commit
    fn create_test_repo() -> (TempDir, git2::Repository) {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Configure test user
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();

        (dir, repo)
    }

    /// Commit a file change to the test repo
    fn commit_file(repo: &git2::Repository, path: &str, content: &str, message: &str) -> git2::Oid {
        let root = repo.workdir().unwrap();
        let file_path = root.join(path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&file_path, content).unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(path)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();

        let sig = repo.signature().unwrap();
        let parent: Vec<git2::Commit> = match repo.head() {
            Ok(head) => vec![head.peel_to_commit().unwrap()],
            Err(_) => vec![],
        };
        let parents: Vec<&git2::Commit> = parent.iter().collect();

        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .unwrap()
    }

    /// Delete a file and commit the deletion
    fn delete_file(repo: &git2::Repository, path: &str, message: &str) -> git2::Oid {
        let root = repo.workdir().unwrap();
        fs::remove_file(root.join(path)).unwrap();

        let mut index = repo.index().unwrap();
        index.remove_path(std::path::Path::new(path)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();

        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();

        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
            .unwrap()
    }

    #[test]
    fn test_basic_commit_loading() {
        let (dir, repo) = create_test_repo();
        let oid1 = commit_file(&repo, "file1.txt", "hello", "Initial commit");
        let _oid2 = commit_file(&repo, "file2.txt", "world", "Add file2");
        let _oid3 = commit_file(&repo, "file1.txt", "hello world", "Update file1");

        let store = Store::new().unwrap();
        let result = load_git_history(&store, dir.path().to_str().unwrap(), None, None);
        assert!(!is_error(&result), "Failed: {}", result_text(&result));
        assert!(
            result_text(&result).contains("3 commits"),
            "Expected 3 commits: {}",
            result_text(&result)
        );

        // Verify commit metadata
        let json = qq(
            &store,
            "SELECT ?hash ?msg",
            "?c a code:Commit ; code:shortHash ?hash ; code:message ?msg .",
        );
        assert!(
            json.contains("Initial commit"),
            "Commit message not found: {json}"
        );
        assert!(
            json.contains("Add file2"),
            "Commit message not found: {json}"
        );

        // Verify author
        let json = qq(
            &store,
            "SELECT ?name ?email",
            "?c a code:Commit ; code:authorName ?name ; code:authorEmail ?email .",
        );
        assert!(json.contains("Test User"), "Author name not found: {json}");
        assert!(
            json.contains("test@example.com"),
            "Author email not found: {json}"
        );

        // Verify commitDate is present
        let json = qq(
            &store,
            "SELECT ?date",
            "?c a code:Commit ; code:commitDate ?date .",
        );
        assert!(!json.is_empty(), "Commit date not found: {json}");

        // Verify full hash
        let short = &oid1.to_string()[..7];
        let json = qq(
            &store,
            "SELECT ?full",
            &format!("?c a code:Commit ; code:shortHash \"{short}\" ; code:commitHash ?full ."),
        );
        assert!(
            json.contains(&oid1.to_string()),
            "Full hash not found: {json}"
        );
    }

    #[test]
    fn test_file_changes() {
        let (dir, repo) = create_test_repo();
        commit_file(&repo, "src/main.rs", "fn main() {}", "Add main");
        commit_file(
            &repo,
            "src/main.rs",
            "fn main() { println!(\"hi\"); }",
            "Modify main",
        );
        delete_file(&repo, "src/main.rs", "Delete main");

        let store = Store::new().unwrap();
        let result = load_git_history(&store, dir.path().to_str().unwrap(), None, None);
        assert!(!is_error(&result), "Failed: {}", result_text(&result));

        // Verify change types
        let json = qq(
            &store,
            "SELECT ?type ?path",
            "?fc a code:FileChange ; code:changeType ?type ; code:filePath ?path .",
        );
        assert!(
            json.contains("added"),
            "Added change type not found: {json}"
        );
        assert!(
            json.contains("modified"),
            "Modified change type not found: {json}"
        );
        assert!(
            json.contains("deleted"),
            "Deleted change type not found: {json}"
        );
        assert!(json.contains("src/main.rs"), "File path not found: {json}");
    }

    #[test]
    fn test_max_commits_limit() {
        let (dir, repo) = create_test_repo();
        for i in 0..10 {
            commit_file(
                &repo,
                &format!("file{i}.txt"),
                &format!("content {i}"),
                &format!("Commit {i}"),
            );
        }

        let store = Store::new().unwrap();
        let result = load_git_history(&store, dir.path().to_str().unwrap(), Some(3), None);
        assert!(!is_error(&result), "Failed: {}", result_text(&result));
        assert!(
            result_text(&result).contains("3 commits"),
            "Expected 3 commits: {}",
            result_text(&result)
        );

        // Verify only 3 commits in store
        let json = qq(&store, "SELECT (COUNT(?c) AS ?count)", "?c a code:Commit .");
        assert!(
            json.contains("\"3\""),
            "Expected 3 commits in store: {json}"
        );
    }

    #[test]
    fn test_parent_commit_linking() {
        let (dir, repo) = create_test_repo();
        let oid1 = commit_file(&repo, "file.txt", "v1", "First commit");
        let oid2 = commit_file(&repo, "file.txt", "v2", "Second commit");
        let oid3 = commit_file(&repo, "file.txt", "v3", "Third commit");

        let store = Store::new().unwrap();
        let result = load_git_history(&store, dir.path().to_str().unwrap(), None, None);
        assert!(!is_error(&result), "Failed: {}", result_text(&result));

        // Third commit's parent should be second commit
        let short3 = &oid3.to_string()[..7];
        let short2 = &oid2.to_string()[..7];
        let short1 = &oid1.to_string()[..7];

        let json = qq(
            &store,
            "SELECT ?parentHash",
            &format!(
                "?c a code:Commit ; code:shortHash \"{short3}\" ; code:parentCommit ?p . ?p code:shortHash ?parentHash ."
            ),
        );
        assert!(
            json.contains(short2),
            "Parent of third should be second: {json}"
        );

        // Second commit's parent should be first commit
        let json = qq(
            &store,
            "SELECT ?parentHash",
            &format!(
                "?c a code:Commit ; code:shortHash \"{short2}\" ; code:parentCommit ?p . ?p code:shortHash ?parentHash ."
            ),
        );
        assert!(
            json.contains(short1),
            "Parent of second should be first: {json}"
        );
    }

    #[test]
    fn test_module_linking() {
        let dir = TempDir::new().unwrap();

        // Create a Rust project structure
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"link-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();

        // Init git repo and commit
        let repo = git2::Repository::init(dir.path()).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
        commit_file(&repo, "src/main.rs", "fn main() {}", "Initial commit");

        let store = Store::new().unwrap();

        // First load code
        let registry = crate::loaders::LoaderRegistry::default();
        let code_result =
            crate::tools::code::load_rust_code(&store, &registry, dir.path().to_str().unwrap());
        assert!(
            !is_error(&code_result),
            "Code load failed: {}",
            result_text(&code_result)
        );

        // Then load git history
        let git_result = load_git_history(&store, dir.path().to_str().unwrap(), None, None);
        assert!(
            !is_error(&git_result),
            "Git load failed: {}",
            result_text(&git_result)
        );
        assert!(
            result_text(&git_result).contains("Linked"),
            "Expected linking info: {}",
            result_text(&git_result)
        );

        // Verify affectsModule link
        let json = qq(
            &store,
            "SELECT ?modPath",
            "?fc a code:FileChange ; code:affectsModule ?mod . ?mod code:relativePath ?modPath .",
        );
        assert!(
            json.contains("src/main.rs"),
            "Module link not found: {json}"
        );

        // Verify hasCommit link on Project
        let json = qq(
            &store,
            "SELECT ?hash",
            "?p a code:Project ; code:hasCommit ?c . ?c code:shortHash ?hash .",
        );
        assert!(!json.is_empty(), "Project-commit link not found: {json}");
    }

    #[test]
    fn test_nonexistent_path() {
        let store = Store::new().unwrap();
        let result = load_git_history(&store, "/nonexistent/path", None, None);
        assert!(is_error(&result));
        assert!(result_text(&result).contains("does not exist"));
    }
}

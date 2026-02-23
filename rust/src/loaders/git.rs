use super::{code_ns, quad, quad_type, string_literal};
use oxigraph::model::{GraphName, Literal, NamedNode, Quad, Term};
use std::fmt;
use std::path::Path;

// --- RDF helpers ---

fn default_graph() -> GraphName {
    GraphName::DefaultGraph
}

fn q(subject: &NamedNode, predicate: &str, object: Term) -> Quad {
    quad(subject, predicate, object, default_graph())
}

fn qt(subject: &NamedNode, class: &str) -> Quad {
    quad_type(subject, class, default_graph())
}

fn datetime_literal(value: &str) -> Term {
    Term::Literal(Literal::new_typed_literal(
        value,
        NamedNode::new("http://www.w3.org/2001/XMLSchema#dateTime").unwrap(),
    ))
}

// --- Error type ---

#[derive(Debug)]
pub enum GitLoadError {
    RepoOpen(String),
    RevParse(String),
    Walk(String),
    Diff(String),
}

impl fmt::Display for GitLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitLoadError::RepoOpen(e) => write!(f, "Failed to open repository: {e}"),
            GitLoadError::RevParse(e) => write!(f, "Failed to resolve ref: {e}"),
            GitLoadError::Walk(e) => write!(f, "Error walking commits: {e}"),
            GitLoadError::Diff(e) => write!(f, "Error computing diff: {e}"),
        }
    }
}

impl std::error::Error for GitLoadError {}

// --- Result type ---

pub struct GitHistoryResult {
    pub quads: Vec<Quad>,
    pub commit_count: u32,
    pub change_count: u32,
}

// --- Time formatting ---

fn format_git_time(time: git2::Time) -> String {
    let offset_minutes = time.offset_minutes();
    let utc_offset =
        time::UtcOffset::from_whole_seconds(offset_minutes * 60).unwrap_or(time::UtcOffset::UTC);
    let dt = time::OffsetDateTime::from_unix_timestamp(time.seconds())
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .to_offset(utc_offset);
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

// --- Core loading function ---

pub fn load_git_history_quads(
    path: &Path,
    max_commits: u32,
    branch: Option<&str>,
) -> Result<GitHistoryResult, GitLoadError> {
    let repo =
        git2::Repository::discover(path).map_err(|e| GitLoadError::RepoOpen(e.to_string()))?;

    // Resolve start commit
    let start_oid = match branch {
        Some(refname) => {
            let obj = repo
                .revparse_single(refname)
                .map_err(|e| GitLoadError::RevParse(e.to_string()))?;
            obj.peel_to_commit()
                .map_err(|e| GitLoadError::RevParse(e.to_string()))?
                .id()
        }
        None => {
            let head = repo
                .head()
                .map_err(|e| GitLoadError::RevParse(e.to_string()))?;
            head.peel_to_commit()
                .map_err(|e| GitLoadError::RevParse(e.to_string()))?
                .id()
        }
    };

    let mut revwalk = repo
        .revwalk()
        .map_err(|e| GitLoadError::Walk(e.to_string()))?;
    revwalk
        .push(start_oid)
        .map_err(|e| GitLoadError::Walk(e.to_string()))?;
    revwalk
        .set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
        .map_err(|e| GitLoadError::Walk(e.to_string()))?;

    let mut quads = Vec::new();
    let mut commit_count = 0u32;
    let mut change_count = 0u32;

    for oid_result in revwalk {
        if commit_count >= max_commits {
            break;
        }

        let oid = oid_result.map_err(|e| GitLoadError::Walk(e.to_string()))?;
        let commit = repo
            .find_commit(oid)
            .map_err(|e| GitLoadError::Walk(e.to_string()))?;

        let hash = oid.to_string();
        let short_hash = &hash[..7.min(hash.len())];
        let commit_uri = code_ns(&format!("commit/{short_hash}"));

        // Commit metadata
        quads.push(qt(&commit_uri, "Commit"));
        quads.push(q(&commit_uri, "commitHash", string_literal(&hash)));
        quads.push(q(&commit_uri, "shortHash", string_literal(short_hash)));

        let author = commit.author();
        if let Some(name) = author.name() {
            quads.push(q(&commit_uri, "authorName", string_literal(name)));
        }
        if let Some(email) = author.email() {
            quads.push(q(&commit_uri, "authorEmail", string_literal(email)));
        }

        let committer = commit.committer();
        if let Some(name) = committer.name() {
            quads.push(q(&commit_uri, "committerName", string_literal(name)));
        }
        if let Some(email) = committer.email() {
            quads.push(q(&commit_uri, "committerEmail", string_literal(email)));
        }

        let date_str = format_git_time(commit.time());
        quads.push(q(&commit_uri, "commitDate", datetime_literal(&date_str)));

        if let Some(msg) = commit.message() {
            quads.push(q(&commit_uri, "message", string_literal(msg)));
        }

        // Parent links
        for parent_id in commit.parent_ids() {
            let parent_hash = parent_id.to_string();
            let parent_short = &parent_hash[..7.min(parent_hash.len())];
            let parent_uri = code_ns(&format!("commit/{parent_short}"));
            quads.push(q(&commit_uri, "parentCommit", Term::NamedNode(parent_uri)));
        }

        // Diff: compute file changes
        let commit_tree = commit
            .tree()
            .map_err(|e| GitLoadError::Diff(e.to_string()))?;

        let parent_tree = if commit.parent_count() > 0 {
            let parent = commit
                .parent(0)
                .map_err(|e| GitLoadError::Diff(e.to_string()))?;
            Some(
                parent
                    .tree()
                    .map_err(|e| GitLoadError::Diff(e.to_string()))?,
            )
        } else {
            None
        };

        let diff = repo
            .diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)
            .map_err(|e| GitLoadError::Diff(e.to_string()))?;

        // Detect renames
        let mut diff = diff;
        diff.find_similar(None)
            .map_err(|e| GitLoadError::Diff(e.to_string()))?;

        for delta_idx in 0..diff.deltas().len() {
            let delta = diff.get_delta(delta_idx).unwrap();
            let new_file = delta.new_file();
            let file_path = new_file
                .path()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            if file_path.is_empty() {
                continue;
            }

            let change_type = match delta.status() {
                git2::Delta::Added => "added",
                git2::Delta::Deleted => {
                    // For deleted files, use the old file path
                    let old_path = delta
                        .old_file()
                        .path()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let fc_uri = code_ns(&format!("commit/{short_hash}/{old_path}"));
                    quads.push(qt(&fc_uri, "FileChange"));
                    quads.push(q(&fc_uri, "changeType", string_literal("deleted")));
                    quads.push(q(&fc_uri, "filePath", string_literal(&old_path)));
                    quads.push(q(&commit_uri, "hasChange", Term::NamedNode(fc_uri)));
                    change_count += 1;
                    continue;
                }
                git2::Delta::Modified => "modified",
                git2::Delta::Renamed => "renamed",
                git2::Delta::Copied => "added",
                _ => continue,
            };

            let fc_uri = code_ns(&format!("commit/{short_hash}/{file_path}"));
            quads.push(qt(&fc_uri, "FileChange"));
            quads.push(q(&fc_uri, "changeType", string_literal(change_type)));
            quads.push(q(&fc_uri, "filePath", string_literal(&file_path)));

            if delta.status() == git2::Delta::Renamed {
                if let Some(old_path) = delta.old_file().path() {
                    quads.push(q(
                        &fc_uri,
                        "oldFilePath",
                        string_literal(&old_path.to_string_lossy()),
                    ));
                }
            }

            quads.push(q(&commit_uri, "hasChange", Term::NamedNode(fc_uri)));
            change_count += 1;
        }

        commit_count += 1;
    }

    Ok(GitHistoryResult {
        quads,
        commit_count,
        change_count,
    })
}

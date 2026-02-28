pub mod git;
pub mod python;
pub mod rust;
pub mod typescript;

use ignore::WalkBuilder;
use oxigraph::model::{GraphName, Literal, NamedNode, NamedOrBlankNode, Quad, Term};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

// --- Shared RDF helpers for code loaders ---

pub const CODE_NS: &str = "https://ds-labs.org/code#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// Percent-encode characters that are invalid in IRIs.
pub fn sanitize_iri_local(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '{' => out.push_str("%7B"),
            '}' => out.push_str("%7D"),
            ' ' => out.push_str("%20"),
            '"' => out.push_str("%22"),
            '|' => out.push_str("%7C"),
            '\\' => out.push_str("%5C"),
            '^' => out.push_str("%5E"),
            '`' => out.push_str("%60"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn code_ns(local: &str) -> NamedNode {
    NamedNode::new(format!("{CODE_NS}{}", sanitize_iri_local(local))).unwrap()
}

pub fn rdf_type() -> NamedNode {
    NamedNode::new(RDF_TYPE).unwrap()
}

pub fn string_literal(value: &str) -> Term {
    Term::Literal(Literal::new_simple_literal(value))
}

pub fn integer_literal(value: i64) -> Term {
    Term::Literal(Literal::new_typed_literal(
        value.to_string(),
        NamedNode::new("http://www.w3.org/2001/XMLSchema#integer").unwrap(),
    ))
}

pub fn quad(subject: &NamedNode, predicate: &str, object: Term, graph: GraphName) -> Quad {
    Quad::new(
        NamedOrBlankNode::NamedNode(subject.clone()),
        code_ns(predicate),
        object,
        graph,
    )
}

pub fn quad_type(subject: &NamedNode, class: &str, graph: GraphName) -> Quad {
    Quad::new(
        NamedOrBlankNode::NamedNode(subject.clone()),
        rdf_type(),
        Term::NamedNode(code_ns(class)),
        graph,
    )
}

/// Error type for code loading operations.
#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Parse {
        file: PathBuf,
        line: Option<usize>,
        message: String,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "I/O error: {e}"),
            LoadError::Parse {
                file,
                line,
                message,
            } => {
                write!(f, "Parse error in {}", file.display())?;
                if let Some(line) = line {
                    write!(f, " at line {line}")?;
                }
                write!(f, ": {message}")
            }
        }
    }
}

impl std::error::Error for LoadError {}

impl From<std::io::Error> for LoadError {
    fn from(e: std::io::Error) -> Self {
        LoadError::Io(e)
    }
}

/// Trait for language-specific code loaders that extract RDF quads from source code.
pub trait LanguageLoader: Send + Sync {
    /// Short identifier for the language (e.g. "rust", "python", "typescript").
    fn language_id(&self) -> &str;

    /// File extensions handled by this loader (without the dot).
    fn file_extensions(&self) -> &[&str];

    /// Parse a single source file and return RDF quads.
    fn load_file(&self, path: &Path, project_root: &Path) -> Result<Vec<Quad>, LoadError>;

    /// Parse project metadata (e.g. Cargo.toml, package.json) and return RDF quads.
    fn load_project_metadata(&self, project_root: &Path) -> Result<Vec<Quad>, LoadError>;

    /// Glob patterns to skip during file discovery (e.g. "target/").
    fn ignore_patterns(&self) -> &[&str] {
        &[]
    }

    /// Return the project URI for the given project root, if project metadata is available.
    fn project_uri(&self, _project_root: &Path) -> Option<NamedNode> {
        None
    }
}

/// Registry of available language loaders with auto-detection.
pub struct LoaderRegistry {
    loaders: HashMap<String, Box<dyn LanguageLoader>>,
}

impl LoaderRegistry {
    pub fn new() -> Self {
        Self {
            loaders: HashMap::new(),
        }
    }

    pub fn register(&mut self, loader: Box<dyn LanguageLoader>) {
        self.loaders
            .insert(loader.language_id().to_string(), loader);
    }

    /// Auto-detect language from a path (checks file extensions and marker files).
    pub fn detect_language(&self, path: &Path) -> Option<&str> {
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                for loader in self.loaders.values() {
                    if loader.file_extensions().contains(&ext) {
                        return Some(loader.language_id());
                    }
                }
            }
        } else if path.is_dir() {
            // Check for language-specific marker files
            if path.join("Cargo.toml").exists() {
                return Some("rust");
            }
            if path.join("pyproject.toml").exists() || path.join("setup.py").exists() {
                return Some("python");
            }
            if path.join("package.json").exists() || path.join("tsconfig.json").exists() {
                return Some("typescript");
            }
        }
        None
    }

    pub fn get_loader(&self, language: &str) -> Option<&dyn LanguageLoader> {
        self.loaders.get(language).map(|b| b.as_ref())
    }
}

impl Default for LoaderRegistry {
    fn default() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(python::PythonLoader));
        registry.register(Box::new(rust::RustLoader));
        registry.register(Box::new(typescript::TypeScriptLoader));
        registry
    }
}

/// Walk a directory tree and collect files matching the given extensions,
/// respecting `.gitignore` and skipping hardcoded ignore patterns as fallback.
pub fn discover_files(root: &Path, extensions: &[&str], ignore_patterns: &[&str]) -> Vec<PathBuf> {
    let mut builder = WalkBuilder::new(root);
    builder.hidden(true); // skip hidden files/dirs
    builder.git_ignore(true); // respect .gitignore
    builder.git_global(false);
    builder.git_exclude(true);

    let normalized: Vec<&str> = ignore_patterns
        .iter()
        .map(|p| p.trim_end_matches('/'))
        .collect();

    let mut files = Vec::new();
    for entry in builder.build() {
        let Ok(entry) = entry else { continue };
        let path = entry.path();

        // Additionally filter out hardcoded ignore patterns (for projects without .gitignore)
        if let Ok(rel) = path.strip_prefix(root) {
            if rel.components().any(|c| {
                let s = c.as_os_str().to_string_lossy();
                normalized.iter().any(|p| s == *p)
            }) {
                continue;
            }
        }

        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if extensions.contains(&ext) {
                    files.push(entry.into_path());
                }
            }
        }
    }
    files.sort();
    files
}

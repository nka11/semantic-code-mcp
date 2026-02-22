pub mod rust;

use oxigraph::model::Quad;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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
        self.loaders.insert(loader.language_id().to_string(), loader);
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
        registry.register(Box::new(rust::RustLoader));
        registry
    }
}

/// Walk a directory tree and collect files matching the given extensions, skipping ignore patterns.
pub fn discover_files(root: &Path, extensions: &[&str], ignore: &[&str]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_entry(|e| {
        let path = e.path();
        // Skip ignored directories
        if path.is_dir() {
            let rel = path.strip_prefix(root).unwrap_or(path);
            for pattern in ignore {
                let pattern = pattern.trim_end_matches('/');
                if rel.components().any(|c| c.as_os_str() == pattern) {
                    return false;
                }
            }
        }
        true
    }) {
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                if extensions.contains(&ext) {
                    files.push(entry.into_path());
                }
            }
        }
    }
    files.sort();
    files
}

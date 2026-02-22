use super::{LanguageLoader, LoadError};
use oxigraph::model::Quad;
use std::path::Path;

pub struct RustLoader;

impl LanguageLoader for RustLoader {
    fn language_id(&self) -> &str {
        "rust"
    }

    fn file_extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn ignore_patterns(&self) -> &[&str] {
        &["target/"]
    }

    fn load_file(&self, _path: &Path, _project_root: &Path) -> Result<Vec<Quad>, LoadError> {
        todo!()
    }

    fn load_project_metadata(&self, _project_root: &Path) -> Result<Vec<Quad>, LoadError> {
        todo!()
    }
}

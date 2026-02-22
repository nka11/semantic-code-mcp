use oxigraph::store::Store;
use std::path::PathBuf;

const DEFAULT_STORE_PATH: &str = "./oxigraph_data";

pub fn open_store() -> Result<Store, Box<dyn std::error::Error>> {
    let path = std::env::var("OXIGRAPH_STORE_PATH")
        .unwrap_or_else(|_| DEFAULT_STORE_PATH.to_string());
    let path = PathBuf::from(path);

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let store = Store::open(&path)?;
    tracing::info!("Oxigraph store opened at: {}", path.display());
    Ok(store)
}

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
) -> CallToolResult {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return tool_error(format!("Path does not exist: {}", path.display()));
    }

    let lang = match language {
        Some(l) => l.to_string(),
        None => match registry.detect_language(path) {
            Some(l) => l.to_string(),
            None => {
                return tool_error(
                    "Could not auto-detect language. Please specify the 'language' parameter.",
                )
            }
        },
    };

    let loader = match registry.get_loader(&lang) {
        Some(l) => l,
        None => return tool_error(format!("No loader registered for language: {lang}")),
    };

    let mut all_quads = Vec::new();
    let mut errors = Vec::new();
    let mut files_loaded = 0u32;

    let project_root = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

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

    let quad_count = all_quads.len();
    for quad in &all_quads {
        if let Err(e) = store.insert(quad) {
            return tool_error(format!("Store insert error: {e}"));
        }
    }

    // Build summary
    let mut summary = format!("Loaded {files_loaded} file(s), {quad_count} triples ({lang}).",);

    if !errors.is_empty() {
        summary.push_str(&format!(
            "\n\nWarnings ({} file(s) failed):\n{}",
            errors.len(),
            errors.join("\n")
        ));
    }

    CallToolResult::success(vec![rmcp::model::Content::text(summary)])
}

pub fn load_rust_code(store: &Store, registry: &LoaderRegistry, path: &str) -> CallToolResult {
    load_code(store, registry, path, Some("rust"))
}

pub fn load_ts_code(store: &Store, registry: &LoaderRegistry, path: &str) -> CallToolResult {
    load_code(store, registry, path, Some("typescript"))
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

    /// Helper: query from the default graph
    fn q(store: &Store, select: &str, body: &str) -> String {
        let sparql =
            format!("PREFIX code: <https://ds-labs.org/code#>\n{select} WHERE {{ {body} }}");
        query_results(store, &sparql)
    }

    #[test]
    fn test_cargo_toml_parsing() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[package]
name = "test-project"
version = "1.0.0"
edition = "2021"
description = "A test project"

[dependencies]
serde = "1.0"
tokio = { version = "1", features = ["full"] }
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "// empty").unwrap();

        let store = Store::new().unwrap();
        let registry = LoaderRegistry::default();
        let result = load_rust_code(&store, &registry, dir.path().to_str().unwrap());
        assert!(
            !is_error(&result),
            "load_rust_code failed: {}",
            result_text(&result)
        );

        let json = q(
            &store,
            "SELECT ?name ?version ?edition",
            "?p a code:Project ; code:name ?name ; code:version ?version ; code:edition ?edition .",
        );
        assert!(
            json.contains("test-project"),
            "Project name not found: {json}"
        );
        assert!(json.contains("1.0.0"), "Version not found: {json}");
        assert!(json.contains("2021"), "Edition not found: {json}");

        let json = q(
            &store,
            "SELECT ?name ?ver",
            "?d a code:Dependency ; code:name ?name ; code:version ?ver .",
        );
        assert!(json.contains("serde"), "serde dependency not found: {json}");
        assert!(json.contains("tokio"), "tokio dependency not found: {json}");
    }

    #[test]
    fn test_rs_ast_extraction() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"ast-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            r#"
/// A greeting function
pub fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}

pub struct Config {
    pub host: String,
    pub port: u16,
}

pub enum Status {
    Active,
    Inactive,
    Pending,
}

pub trait Handler {
    fn handle(&self);
    fn name(&self) -> &str;
}

impl Config {
    pub fn new(host: String, port: u16) -> Self {
        Self { host, port }
    }
}

use std::collections::HashMap;

mod utils;
"#,
        )
        .unwrap();

        let store = Store::new().unwrap();
        let registry = LoaderRegistry::default();
        let result = load_rust_code(&store, &registry, dir.path().to_str().unwrap());
        assert!(!is_error(&result), "Failed: {}", result_text(&result));

        // Function
        let json = q(&store, "SELECT ?name ?vis",
            "?f a code:Function ; code:name ?name ; code:visibility ?vis . FILTER NOT EXISTS { ?_ code:hasFunction ?f }");
        assert!(json.contains("greet"), "Function 'greet' not found: {json}");
        assert!(json.contains("pub"), "Visibility not found: {json}");

        // Struct (Class)
        let json = q(
            &store,
            "SELECT ?name",
            "?s a code:Class ; code:name ?name .",
        );
        assert!(json.contains("Config"), "Struct 'Config' not found: {json}");

        // Enum
        let json = q(&store, "SELECT ?name", "?e a code:Enum ; code:name ?name .");
        assert!(json.contains("Status"), "Enum 'Status' not found: {json}");

        // Trait
        let json = q(
            &store,
            "SELECT ?name",
            "?t a code:Trait ; code:name ?name .",
        );
        assert!(
            json.contains("Handler"),
            "Trait 'Handler' not found: {json}"
        );

        // Trait methods
        let json = q(
            &store,
            "SELECT ?method",
            "?t a code:Trait ; code:hasMethod ?method .",
        );
        assert!(
            json.contains("handle"),
            "Trait method 'handle' not found: {json}"
        );
        assert!(
            json.contains("name"),
            "Trait method 'name' not found: {json}"
        );

        // Impl methods (linked via hasFunction)
        let json = q(
            &store,
            "SELECT ?method",
            "?c a code:Class ; code:hasFunction ?f . ?f code:name ?method .",
        );
        assert!(json.contains("new"), "Impl method 'new' not found: {json}");

        // Import
        let json = q(
            &store,
            "SELECT ?path",
            "?i a code:Import ; code:importPath ?path .",
        );
        assert!(json.contains("HashMap"), "Import not found: {json}");

        // Module declaration
        let json = q(
            &store,
            "SELECT ?name",
            r#"?m a code:Module ; code:name ?name . FILTER(?name != "")"#,
        );
        assert!(json.contains("utils"), "Module 'utils' not found: {json}");

        // Docstring
        let json = q(
            &store,
            "SELECT ?doc",
            r#"?f a code:Function ; code:name "greet" ; code:docstring ?doc ."#,
        );
        assert!(json.contains("greeting"), "Docstring not found: {json}");
    }

    #[test]
    fn test_directory_loading_with_file_discovery() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"multi-file\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            "pub fn hello() -> &'static str { \"hi\" }",
        )
        .unwrap();

        // target/ dir should be ignored
        fs::create_dir_all(dir.path().join("target/debug")).unwrap();
        fs::write(dir.path().join("target/debug/build.rs"), "fn build() {}").unwrap();

        let store = Store::new().unwrap();
        let registry = LoaderRegistry::default();
        let result = load_rust_code(&store, &registry, dir.path().to_str().unwrap());
        let text = result_text(&result);
        assert!(!is_error(&result), "Failed: {text}");
        assert!(
            text.contains("2 file(s)"),
            "Expected 2 files loaded: {text}"
        );

        let json = q(
            &store,
            "SELECT ?path",
            "?m a code:Module ; code:relativePath ?path .",
        );
        assert!(
            !json.contains("target"),
            "target/ should be ignored: {json}"
        );
    }

    #[test]
    fn test_auto_detection() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"detect-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn foo() {}").unwrap();

        let store = Store::new().unwrap();
        let registry = LoaderRegistry::default();
        let result = load_code(&store, &registry, dir.path().to_str().unwrap(), None);
        assert!(
            !is_error(&result),
            "Auto-detect failed: {}",
            result_text(&result)
        );
        assert!(
            result_text(&result).contains("(rust)"),
            "Should detect rust: {}",
            result_text(&result)
        );
    }

    #[test]
    fn test_single_file_loading() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.rs");
        fs::write(&file, "pub fn single() -> i32 { 42 }").unwrap();

        let store = Store::new().unwrap();
        let registry = LoaderRegistry::default();
        let result = load_rust_code(&store, &registry, file.to_str().unwrap());
        assert!(!is_error(&result), "Failed: {}", result_text(&result));
        assert!(result_text(&result).contains("1 file(s)"));

        let json = q(
            &store,
            "SELECT ?name",
            "?f a code:Function ; code:name ?name .",
        );
        assert!(json.contains("single"), "Function not found: {json}");
    }

    #[test]
    fn test_nonexistent_path() {
        let store = Store::new().unwrap();
        let registry = LoaderRegistry::default();
        let result = load_rust_code(&store, &registry, "/nonexistent/path");
        assert!(is_error(&result));
        assert!(result_text(&result).contains("does not exist"));
    }

    // --- TypeScript loader tests ---

    #[test]
    fn test_package_json_parsing() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{
  "name": "my-app",
  "version": "2.0.0",
  "description": "A test app",
  "dependencies": {
    "express": "^4.18.0",
    "lodash": "^4.17.21"
  },
  "devDependencies": {
    "typescript": "^5.0.0"
  }
}"#,
        )
        .unwrap();
        fs::write(dir.path().join("index.ts"), "export function main() {}").unwrap();

        let store = Store::new().unwrap();
        let registry = LoaderRegistry::default();
        let result = load_ts_code(&store, &registry, dir.path().to_str().unwrap());
        assert!(
            !is_error(&result),
            "load_ts_code failed: {}",
            result_text(&result)
        );

        // Project metadata
        let json = q(
            &store,
            "SELECT ?name ?version",
            "?p a code:Project ; code:name ?name ; code:version ?version .",
        );
        assert!(json.contains("my-app"), "Project name not found: {json}");
        assert!(json.contains("2.0.0"), "Version not found: {json}");

        // Dependencies
        let json = q(
            &store,
            "SELECT ?name ?ver",
            "?d a code:Dependency ; code:name ?name ; code:version ?ver .",
        );
        assert!(json.contains("express"), "express dep not found: {json}");
        assert!(json.contains("lodash"), "lodash dep not found: {json}");
        assert!(
            json.contains("typescript"),
            "typescript devDep not found: {json}"
        );
    }

    #[test]
    fn test_ts_ast_extraction() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name": "ast-test", "version": "1.0.0"}"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/index.ts"),
            r#"
/** Greet someone */
export function greet(name: string): string {
    return `Hello, ${name}!`;
}

export class Config {
    host: string;
    port: number;

    constructor(host: string, port: number) {
        this.host = host;
        this.port = port;
    }

    getUrl(): string {
        return `${this.host}:${this.port}`;
    }
}

export interface Handler {
    handle(request: Request): Response;
    name: string;
}

export type UserId = string | number;

export enum Status {
    Active,
    Inactive,
    Pending,
}

import { Request, Response } from 'express';

const helper = (x: number): number => x * 2;
"#,
        )
        .unwrap();

        let store = Store::new().unwrap();
        let registry = LoaderRegistry::default();
        let result = load_ts_code(&store, &registry, dir.path().to_str().unwrap());
        assert!(!is_error(&result), "Failed: {}", result_text(&result));

        // Function
        let json = q(
            &store,
            "SELECT ?name ?vis",
            r#"?f a code:Function ; code:name "greet" ; code:visibility ?vis ; code:name ?name ."#,
        );
        assert!(json.contains("greet"), "Function 'greet' not found: {json}");
        assert!(
            json.contains("export"),
            "Export visibility not found: {json}"
        );

        // Class
        let json = q(
            &store,
            "SELECT ?name",
            "?s a code:Class ; code:name ?name . FILTER NOT EXISTS { ?s code:kind ?k }",
        );
        assert!(json.contains("Config"), "Class 'Config' not found: {json}");

        // Class fields
        let json = q(
            &store,
            "SELECT ?field",
            r#"?c a code:Class ; code:name "Config" ; code:hasField ?field ."#,
        );
        assert!(json.contains("host"), "Field 'host' not found: {json}");
        assert!(json.contains("port"), "Field 'port' not found: {json}");

        // Class methods
        let json = q(
            &store,
            "SELECT ?method",
            r#"?c a code:Class ; code:name "Config" ; code:hasFunction ?f . ?f code:name ?method ."#,
        );
        assert!(
            json.contains("constructor"),
            "Constructor not found: {json}"
        );
        assert!(json.contains("getUrl"), "Method 'getUrl' not found: {json}");

        // Interface (mapped to Trait)
        let json = q(
            &store,
            "SELECT ?name",
            "?t a code:Trait ; code:name ?name .",
        );
        assert!(
            json.contains("Handler"),
            "Interface 'Handler' not found: {json}"
        );

        // Interface methods
        let json = q(
            &store,
            "SELECT ?method",
            r#"?t a code:Trait ; code:name "Handler" ; code:hasMethod ?method ."#,
        );
        assert!(
            json.contains("handle"),
            "Interface method 'handle' not found: {json}"
        );

        // Interface fields
        let json = q(
            &store,
            "SELECT ?field",
            r#"?t a code:Trait ; code:name "Handler" ; code:hasField ?field ."#,
        );
        assert!(
            json.contains("name"),
            "Interface field 'name' not found: {json}"
        );

        // Type alias
        let json = q(
            &store,
            "SELECT ?name",
            r#"?c a code:Class ; code:kind "type_alias" ; code:name ?name ."#,
        );
        assert!(
            json.contains("UserId"),
            "Type alias 'UserId' not found: {json}"
        );

        // Enum
        let json = q(&store, "SELECT ?name", "?e a code:Enum ; code:name ?name .");
        assert!(json.contains("Status"), "Enum 'Status' not found: {json}");

        // Enum variants
        let json = q(
            &store,
            "SELECT ?variant",
            r#"?e a code:Enum ; code:name "Status" ; code:hasVariant ?variant ."#,
        );
        assert!(
            json.contains("Active"),
            "Variant 'Active' not found: {json}"
        );
        assert!(
            json.contains("Inactive"),
            "Variant 'Inactive' not found: {json}"
        );
        assert!(
            json.contains("Pending"),
            "Variant 'Pending' not found: {json}"
        );

        // Import
        let json = q(
            &store,
            "SELECT ?path",
            "?i a code:Import ; code:importPath ?path .",
        );
        assert!(json.contains("express"), "Import not found: {json}");

        // Arrow function
        let json = q(
            &store,
            "SELECT ?name",
            r#"?f a code:Function ; code:name "helper" ; code:name ?name ."#,
        );
        assert!(
            json.contains("helper"),
            "Arrow function 'helper' not found: {json}"
        );

        // Docstring
        let json = q(
            &store,
            "SELECT ?doc",
            r#"?f a code:Function ; code:name "greet" ; code:docstring ?doc ."#,
        );
        assert!(
            json.contains("Greet someone"),
            "Docstring not found: {json}"
        );

        // Return type
        let json = q(
            &store,
            "SELECT ?ret",
            r#"?f a code:Function ; code:name "greet" ; code:returnType ?ret ."#,
        );
        assert!(json.contains("string"), "Return type not found: {json}");

        // Parameters
        let json = q(
            &store,
            "SELECT ?param",
            r#"?f a code:Function ; code:name "greet" ; code:parameter ?param ."#,
        );
        assert!(json.contains("name"), "Parameter 'name' not found: {json}");
    }

    #[test]
    fn test_tsx_jsx_support() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Component.tsx"),
            r#"
import React from 'react';

interface Props {
    title: string;
}

export function MyComponent(props: Props) {
    return <div>{props.title}</div>;
}
"#,
        )
        .unwrap();

        let store = Store::new().unwrap();
        let registry = LoaderRegistry::default();
        let result = load_ts_code(
            &store,
            &registry,
            dir.path().join("Component.tsx").to_str().unwrap(),
        );
        assert!(
            !is_error(&result),
            "TSX parse failed: {}",
            result_text(&result)
        );

        let json = q(
            &store,
            "SELECT ?name",
            "?f a code:Function ; code:name ?name .",
        );
        assert!(
            json.contains("MyComponent"),
            "TSX function not found: {json}"
        );
    }

    #[test]
    fn test_node_modules_ignored() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name": "ignore-test", "version": "1.0.0"}"#,
        )
        .unwrap();
        fs::write(dir.path().join("index.ts"), "export function app() {}").unwrap();

        // node_modules should be ignored
        fs::create_dir_all(dir.path().join("node_modules/foo")).unwrap();
        fs::write(
            dir.path().join("node_modules/foo/index.ts"),
            "export function internal() {}",
        )
        .unwrap();

        let store = Store::new().unwrap();
        let registry = LoaderRegistry::default();
        let result = load_ts_code(&store, &registry, dir.path().to_str().unwrap());
        let text = result_text(&result);
        assert!(!is_error(&result), "Failed: {text}");
        assert!(
            text.contains("1 file(s)"),
            "Expected 1 file loaded (node_modules excluded): {text}"
        );

        let json = q(
            &store,
            "SELECT ?path",
            "?m a code:Module ; code:relativePath ?path .",
        );
        assert!(
            !json.contains("node_modules"),
            "node_modules should be ignored: {json}"
        );
    }

    #[test]
    fn test_auto_detection_typescript() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name": "detect-ts", "version": "1.0.0"}"#,
        )
        .unwrap();
        fs::write(dir.path().join("index.ts"), "export function foo() {}").unwrap();

        let store = Store::new().unwrap();
        let registry = LoaderRegistry::default();
        // Use load_code without specifying language — should auto-detect typescript
        let result = load_code(&store, &registry, dir.path().to_str().unwrap(), None);
        assert!(
            !is_error(&result),
            "Auto-detect failed: {}",
            result_text(&result)
        );
        assert!(
            result_text(&result).contains("(typescript)"),
            "Should detect typescript: {}",
            result_text(&result)
        );
    }

    #[test]
    fn test_ts_class_implements_extends() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("models.ts"),
            r#"
interface Serializable {
    serialize(): string;
}

interface Loggable {
    log(): void;
}

class Base {
    id: number;
}

export class User extends Base implements Serializable, Loggable {
    name: string;

    serialize(): string {
        return JSON.stringify(this);
    }

    log(): void {
        console.log(this.name);
    }
}
"#,
        )
        .unwrap();

        let store = Store::new().unwrap();
        let registry = LoaderRegistry::default();
        let result = load_ts_code(
            &store,
            &registry,
            dir.path().join("models.ts").to_str().unwrap(),
        );
        assert!(!is_error(&result), "Failed: {}", result_text(&result));

        // Check implements
        let json = q(
            &store,
            "SELECT ?iface",
            r#"?c a code:Class ; code:name "User" ; code:implements ?iface ."#,
        );
        assert!(
            json.contains("Serializable"),
            "implements Serializable not found: {json}"
        );
        assert!(
            json.contains("Loggable"),
            "implements Loggable not found: {json}"
        );

        // Check extends
        let json = q(
            &store,
            "SELECT ?parent",
            r#"?c a code:Class ; code:name "User" ; code:extends ?parent ."#,
        );
        assert!(json.contains("Base"), "extends Base not found: {json}");
    }
}

# M4 — TypeScript Loader Implementation Plan

## 1. Parser Crate Selection: `oxc_parser`

### Rationale

| Crate | Pros | Cons |
|---|---|---|
| **oxc_parser** | Fastest TS/JS parser in Rust; typed AST; full TS/TSX/JSX support; active development; arena allocator | Rapidly evolving API (0.x); arena-allocated AST uses lifetimes |
| **swc_ecma_parser** | Mature; broad ecosystem | Heavier dependency tree; larger compile time |
| **tree-sitter-typescript** | Error-tolerant; incremental | CST not AST — string-based node matching; C dependency |
| **Native TS tooling** (ts-morph, tsc) | Full type resolution | Requires Node.js runtime; subprocess IPC; 10-50x slower; breaks single-binary distribution |

**Decision: `oxc_parser`** — typed AST (unlike tree-sitter), fastest option, first-class TS/TSX/JSX support, zero runtime deps. Matches existing pattern where `RustLoader` uses `syn`. Type resolution is unnecessary for structural code extraction.

### Dependencies to Add

```toml
oxc_parser = "0.114"
oxc_ast = "0.114"
oxc_allocator = "0.114"
oxc_span = "0.114"
```

All `oxc_*` crates must use the same version for compatibility.

## 2. Architecture

```
TypeScriptLoader
  implements LanguageLoader
    language_id() -> "typescript"
    file_extensions() -> &["ts", "tsx", "js", "jsx"]
    ignore_patterns() -> &["node_modules/", "dist/", "build/", ".next/"]
    load_file(path, project_root) -> Vec<Quad>
    load_project_metadata(project_root) -> Vec<Quad>
```

All triples written to the default graph.

## 3. RDF Entities to Extract

### From `package.json`

| Entity | RDF Class | Properties |
|---|---|---|
| Project | `code:Project` | `name`, `version`, `description` |
| Dependency | `code:Dependency` | `name`, `version` (linked via `code:hasDependency`) |

### From `.ts/.tsx/.js/.jsx` files

| Source Construct | RDF Class | Key Properties |
|---|---|---|
| File/module | `code:Module` | `name`, `filePath`, `relativePath`, `language` |
| `function` declaration | `code:Function` | `name`, `visibility`, `parameter`, `returnType`, `startLine`, `endLine`, `definedIn`, `docstring` |
| Arrow function assigned to const/let | `code:Function` | Same (name from binding identifier) |
| `class` declaration | `code:Class` | `name`, `visibility`, `startLine`, `endLine`, `definedIn`, `docstring`, `hasField`, `hasFunction` |
| Class methods | `code:Function` | `name`, `visibility`, `parameter`, `returnType`, `startLine`, `endLine`, `definedIn` |
| `interface` declaration | `code:Trait` | `name`, `visibility`, `startLine`, `endLine`, `definedIn`, `hasMethod`, `docstring` |
| `type` alias | `code:Class` | `name`, `visibility`, `startLine`, `endLine`, `definedIn`, `docstring` + `code:kind "type_alias"` |
| `enum` declaration | `code:Enum` | `name`, `visibility`, `hasVariant`, `startLine`, `endLine`, `definedIn` |
| `import` statement | `code:Import` | `importPath` |
| `export` | Visibility flag | `visibility: "export"` or `"export default"` |
| `implements` clause | `code:implements` | string of interface name |

## 4. Implementation Steps

### Step 1: Add dependencies to `Cargo.toml`

Add `oxc_parser`, `oxc_ast`, `oxc_allocator`, `oxc_span`.

### Step 2: Refactor shared RDF helpers

Extract duplicated RDF helper functions (`code_ns`, `rdf_type`, `string_literal`, `integer_literal`, `quad`, `quad_type`, `sanitize_iri_local`) from `rust.rs` into `loaders/mod.rs` or a new `loaders/rdf_helpers.rs`. Update `rust.rs` to use shared helpers.

### Step 3: Create `rust/src/loaders/typescript.rs`

#### 3a: `parse_package_json(project_root) -> Result<Vec<Quad>, LoadError>`

- Read and parse `package.json` with `serde_json`
- Extract `name`, `version`, `description` → `code:Project` triples
- Extract `dependencies` and `devDependencies` → `code:Dependency` triples

#### 3b: `parse_ts_file(path, project_root) -> Result<Vec<Quad>, LoadError>`

Core flow:
1. Read file content
2. Create `oxc_allocator::Allocator`
3. Determine `SourceType` from file extension
4. Parse with `Parser::new(&allocator, &source_text, source_type).parse()`
5. Build line-offset table for span → line number conversion
6. Create Module triples
7. Iterate `program.body` statements, matching on declaration types
8. Collect and return `Vec<Quad>`

#### 3c: Statement extraction functions

- `extract_function_quads` — function/method declarations
- `extract_class_quads` — class with methods, properties, implements, extends
- `extract_interface_quads` — interface → `code:Trait`
- `extract_type_alias_quads` — type alias → `code:Class` with `code:kind "type_alias"`
- `extract_enum_quads` — enum with variants
- `extract_variable_declaration_quads` — arrow functions assigned to const/let
- `extract_import_quads` — import statements
- Export handling via visibility flags on exported declarations

#### 3d: JSDoc extraction

- Collect block comments (`/** ... */`) from `program.comments`
- For each declaration, find nearest preceding block comment
- Strip `/** */` delimiters and `*` line prefixes
- Store as `code:docstring`

#### 3e: Span → line number conversion

Build a precomputed line-start offset table, binary search for line number from byte offset.

### Step 4: Register TypeScript loader

- `loaders/mod.rs`: Add `pub mod typescript;`, register `TypeScriptLoader` in `LoaderRegistry::default()`
- `tools/code.rs`: Add `load_ts_code` wrapper function
- `main.rs`: Add `LoadTsCodeParams` struct and `load_ts_code` tool handler

### Step 5: Write tests

- `test_package_json_parsing` — project metadata and dependencies
- `test_ts_ast_extraction` — all entity types (functions, classes, interfaces, enums, type aliases, imports, exports)
- `test_tsx_jsx_support` — TSX/JSX files parse correctly
- `test_node_modules_ignored` — node_modules directory skipped
- `test_arrow_function_extraction` — const arrow functions extracted as named functions
- `test_auto_detection_typescript` — auto-detect from package.json/tsconfig.json

## 5. File Change Summary

| File | Action | Description |
|---|---|---|
| `rust/Cargo.toml` | Modify | Add oxc dependencies |
| `rust/src/loaders/mod.rs` | Modify | Extract shared helpers, add `pub mod typescript`, register loader |
| `rust/src/loaders/rust.rs` | Modify | Use shared RDF helpers |
| `rust/src/loaders/typescript.rs` | **Create** | TypeScript loader (~400-600 lines) |
| `rust/src/tools/code.rs` | Modify | Add `load_ts_code` wrapper |
| `rust/src/main.rs` | Modify | Add `LoadTsCodeParams`, `load_ts_code` handler |

## 6. Commit Sequence

1. Save plan to `plans/m4-typescript-loader.md`
2. Refactor RDF helpers into shared location
3. Add oxc deps, create `typescript.rs` skeleton with `LanguageLoader` impl and `parse_package_json`
4. Implement `parse_ts_file` with full AST extraction
5. Implement JSDoc extraction and export visibility
6. Wire up `load_ts_code` tool in `code.rs` and `main.rs`
7. Add unit tests
8. Manual testing and fixes

## 7. Risks and Mitigations

| Risk | Mitigation |
|---|---|
| oxc API breaking changes | Pin exact version |
| Arena allocator lifetime issues | Collect quads into owned `Vec<Quad>` before allocator drops |
| Incomplete JSDoc extraction | Best-effort heuristic, document limitations |
| oxc compile time | OXC optimizes for this; monitor |

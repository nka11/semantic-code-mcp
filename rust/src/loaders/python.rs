use super::{code_ns, integer_literal, quad, quad_type, string_literal, LanguageLoader, LoadError};
use oxigraph::model::{GraphName, NamedNode, Quad, Term};
use rustpython_parser::ast::{self, Expr, Stmt};
use rustpython_parser::Parse;
use std::path::Path;

pub struct PythonLoader;

fn default_graph() -> GraphName {
    GraphName::DefaultGraph
}

fn q(subject: &NamedNode, predicate: &str, object: Term) -> Quad {
    quad(subject, predicate, object, default_graph())
}

fn qt(subject: &NamedNode, class: &str) -> Quad {
    quad_type(subject, class, default_graph())
}

// --- Line number conversion (byte offset → 1-based line) ---

fn build_line_table(source: &str) -> Vec<u32> {
    let mut table = vec![0u32];
    for (i, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            table.push((i + 1) as u32);
        }
    }
    table
}

fn offset_to_line(line_table: &[u32], offset: u32) -> usize {
    match line_table.binary_search(&offset) {
        Ok(idx) => idx + 1,
        Err(idx) => idx,
    }
}

/// Convert a Python expression AST node to a human-readable string.
fn expr_to_string(expr: &Expr) -> String {
    match expr {
        Expr::Name(name) => name.id.to_string(),
        Expr::Attribute(attr) => {
            format!("{}.{}", expr_to_string(&attr.value), attr.attr)
        }
        Expr::Subscript(sub) => {
            format!(
                "{}[{}]",
                expr_to_string(&sub.value),
                expr_to_string(&sub.slice)
            )
        }
        Expr::Constant(c) => match &c.value {
            ast::Constant::Str(s) => format!("\"{s}\""),
            ast::Constant::Int(i) => i.to_string(),
            ast::Constant::Float(f) => f.to_string(),
            ast::Constant::Bool(b) => b.to_string(),
            ast::Constant::None => "None".to_string(),
            ast::Constant::Ellipsis => "...".to_string(),
            _ => "...".to_string(),
        },
        Expr::Tuple(t) => {
            let elts: Vec<String> = t.elts.iter().map(expr_to_string).collect();
            format!("({})", elts.join(", "))
        }
        Expr::List(l) => {
            let elts: Vec<String> = l.elts.iter().map(expr_to_string).collect();
            format!("[{}]", elts.join(", "))
        }
        Expr::BinOp(b) => {
            format!("{} | {}", expr_to_string(&b.left), expr_to_string(&b.right))
        }
        Expr::Call(call) => {
            let func = expr_to_string(&call.func);
            let args: Vec<String> = call.args.iter().map(expr_to_string).collect();
            if args.is_empty() {
                format!("{func}()")
            } else {
                format!("{func}({})", args.join(", "))
            }
        }
        Expr::Starred(s) => format!("*{}", expr_to_string(&s.value)),
        _ => "...".to_string(),
    }
}

/// Extract docstring from a function/class body (first statement if it's a string constant).
fn extract_docstring(body: &[Stmt]) -> Option<String> {
    if let Some(Stmt::Expr(expr_stmt)) = body.first() {
        if let Expr::Constant(c) = expr_stmt.value.as_ref() {
            if let ast::Constant::Str(s) = &c.value {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    None
}

/// Determine visibility from a Python name convention.
fn python_visibility(name: &str) -> &'static str {
    if name.starts_with('_') && !(name.starts_with("__") && name.ends_with("__")) {
        "private"
    } else {
        "public"
    }
}

// --- pyproject.toml parsing ---

fn parse_pyproject_toml(project_root: &Path) -> Result<Vec<Quad>, LoadError> {
    let toml_path = project_root.join("pyproject.toml");
    let content = std::fs::read_to_string(&toml_path)?;
    let doc: toml::Value = toml::from_str(&content).map_err(|e| LoadError::Parse {
        file: toml_path.clone(),
        line: None,
        message: e.to_string(),
    })?;

    let mut quads = Vec::new();

    // Try [project] table first, then [tool.poetry] for Poetry projects
    let project_table = doc.get("project").and_then(|v| v.as_table()).or_else(|| {
        doc.get("tool")
            .and_then(|t| t.get("poetry"))
            .and_then(|v| v.as_table())
    });

    let Some(proj) = project_table else {
        return Ok(quads);
    };

    let name = proj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let project_uri = code_ns(&format!("project/{name}"));

    quads.push(qt(&project_uri, "Project"));
    quads.push(q(&project_uri, "name", string_literal(name)));
    quads.push(q(&project_uri, "language", string_literal("python")));

    if let Some(version) = proj.get("version").and_then(|v| v.as_str()) {
        quads.push(q(&project_uri, "version", string_literal(version)));
    }

    if let Some(desc) = proj.get("description").and_then(|v| v.as_str()) {
        quads.push(q(&project_uri, "description", string_literal(desc)));
    }

    // Parse dependencies
    if let Some(deps) = proj.get("dependencies") {
        parse_python_dependencies(deps, &project_uri, &mut quads);
    }

    Ok(quads)
}

fn parse_python_dependencies(deps: &toml::Value, project_uri: &NamedNode, quads: &mut Vec<Quad>) {
    match deps {
        // Array of PEP 508 strings: ["requests>=2.28", "flask"]
        toml::Value::Array(arr) => {
            for dep in arr {
                if let Some(spec) = dep.as_str() {
                    let (name, version) = parse_pep508(spec);
                    emit_dependency(quads, project_uri, &name, version.as_deref());
                }
            }
        }
        // Table (Poetry style): { requests = "^2.28", flask = { version = "^3.0" } }
        toml::Value::Table(table) => {
            for (name, value) in table {
                let version = match value {
                    toml::Value::String(v) => Some(v.as_str().to_string()),
                    toml::Value::Table(t) => {
                        t.get("version").and_then(|v| v.as_str()).map(String::from)
                    }
                    _ => None,
                };
                emit_dependency(quads, project_uri, name, version.as_deref());
            }
        }
        _ => {}
    }
}

/// Parse a PEP 508 dependency string like "requests>=2.28.0" or "flask".
fn parse_pep508(spec: &str) -> (String, Option<String>) {
    let ops = [">=", "<=", "==", "!=", "~=", ">", "<"];
    for op in ops {
        if let Some(idx) = spec.find(op) {
            let name = spec[..idx].trim().to_string();
            let version = spec[idx + op.len()..].trim().to_string();
            let version = version
                .split(';')
                .next()
                .unwrap_or(&version)
                .trim()
                .to_string();
            let name = name.split('[').next().unwrap_or(&name).trim().to_string();
            return (name, Some(version));
        }
    }
    let name = spec
        .split('[')
        .next()
        .unwrap_or(spec)
        .split(';')
        .next()
        .unwrap_or(spec)
        .trim()
        .to_string();
    (name, None)
}

fn emit_dependency(
    quads: &mut Vec<Quad>,
    project_uri: &NamedNode,
    name: &str,
    version: Option<&str>,
) {
    let dep_uri = code_ns(&format!("dep/{name}"));
    quads.push(qt(&dep_uri, "Dependency"));
    quads.push(q(&dep_uri, "name", string_literal(name)));
    if let Some(ver) = version {
        quads.push(q(&dep_uri, "version", string_literal(ver)));
    }
    quads.push(q(project_uri, "hasDependency", Term::NamedNode(dep_uri)));
}

// --- requirements.txt parsing ---

fn parse_requirements_txt(project_root: &Path) -> Result<Vec<Quad>, LoadError> {
    let req_path = project_root.join("requirements.txt");
    let content = std::fs::read_to_string(&req_path)?;

    let mut quads = Vec::new();
    let project_uri = code_ns("project/unknown");
    quads.push(qt(&project_uri, "Project"));
    quads.push(q(&project_uri, "language", string_literal("python")));

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        let (name, version) = parse_pep508(line);
        if !name.is_empty() {
            emit_dependency(&mut quads, &project_uri, &name, version.as_deref());
        }
    }

    Ok(quads)
}

// --- Python AST extraction ---

struct ParseContext<'a> {
    rel_path: &'a str,
    module_uri: &'a NamedNode,
    line_table: &'a [u32],
}

/// Shared logic for extracting function/method quads from either sync or async function defs.
#[allow(clippy::too_many_arguments)]
fn emit_function_quads(
    name: &str,
    args: &ast::Arguments,
    returns: Option<&Expr>,
    decorator_list: &[Expr],
    body: &[Stmt],
    start_offset: u32,
    end_offset: u32,
    is_async: bool,
    ctx: &ParseContext,
    parent_class: Option<&str>,
) -> Vec<Quad> {
    let func_uri = match parent_class {
        Some(class_name) => code_ns(&format!("{}/{class_name}/{name}", ctx.rel_path)),
        None => code_ns(&format!("{}/{name}", ctx.rel_path)),
    };

    let mut quads = vec![
        qt(&func_uri, "Function"),
        q(&func_uri, "name", string_literal(name)),
        q(
            &func_uri,
            "definedIn",
            Term::NamedNode(ctx.module_uri.clone()),
        ),
        q(
            &func_uri,
            "visibility",
            string_literal(python_visibility(name)),
        ),
    ];

    // Link to module (not for methods — those are linked from the class)
    if parent_class.is_none() {
        quads.push(q(
            ctx.module_uri,
            "hasFunction",
            Term::NamedNode(func_uri.clone()),
        ));
    }

    // Parameters (skip 'self' and 'cls' for methods)
    for arg in &args.args {
        let param_name = arg.def.arg.as_str();
        if parent_class.is_some() && (param_name == "self" || param_name == "cls") {
            continue;
        }
        quads.push(q(&func_uri, "parameter", string_literal(param_name)));
    }

    // Return type annotation
    if let Some(ret) = returns {
        quads.push(q(
            &func_uri,
            "returnType",
            string_literal(&expr_to_string(ret)),
        ));
    }

    // Decorators
    for decorator in decorator_list {
        quads.push(q(
            &func_uri,
            "decorator",
            string_literal(&expr_to_string(decorator)),
        ));
    }

    // Docstring
    if let Some(doc) = extract_docstring(body) {
        quads.push(q(&func_uri, "docstring", string_literal(&doc)));
    }

    // Line numbers
    let start_line = offset_to_line(ctx.line_table, start_offset) as i64;
    let end_line = offset_to_line(ctx.line_table, end_offset) as i64;
    quads.push(q(&func_uri, "startLine", integer_literal(start_line)));
    quads.push(q(&func_uri, "endLine", integer_literal(end_line)));

    // Async flag
    if is_async {
        quads.push(q(&func_uri, "async", string_literal("true")));
    }

    quads
}

fn extract_function_quads(
    func: &ast::StmtFunctionDef,
    ctx: &ParseContext,
    parent_class: Option<&str>,
) -> Vec<Quad> {
    use rustpython_parser::ast::Ranged;
    emit_function_quads(
        func.name.as_str(),
        &func.args,
        func.returns.as_deref(),
        &func.decorator_list,
        &func.body,
        func.start().to_u32(),
        func.end().to_u32(),
        false,
        ctx,
        parent_class,
    )
}

fn extract_async_function_quads(
    func: &ast::StmtAsyncFunctionDef,
    ctx: &ParseContext,
    parent_class: Option<&str>,
) -> Vec<Quad> {
    use rustpython_parser::ast::Ranged;
    emit_function_quads(
        func.name.as_str(),
        &func.args,
        func.returns.as_deref(),
        &func.decorator_list,
        &func.body,
        func.start().to_u32(),
        func.end().to_u32(),
        true,
        ctx,
        parent_class,
    )
}

fn extract_class_quads(class: &ast::StmtClassDef, ctx: &ParseContext) -> Vec<Quad> {
    use rustpython_parser::ast::Ranged;

    let name = class.name.as_str();
    let class_uri = code_ns(&format!("{}/{name}", ctx.rel_path));

    let mut quads = vec![
        qt(&class_uri, "Class"),
        q(&class_uri, "name", string_literal(name)),
        q(
            &class_uri,
            "definedIn",
            Term::NamedNode(ctx.module_uri.clone()),
        ),
        q(
            &class_uri,
            "visibility",
            string_literal(python_visibility(name)),
        ),
    ];

    // Base classes (extends)
    for base in &class.bases {
        quads.push(q(
            &class_uri,
            "extends",
            string_literal(&expr_to_string(base)),
        ));
    }

    // Decorators
    for decorator in &class.decorator_list {
        quads.push(q(
            &class_uri,
            "decorator",
            string_literal(&expr_to_string(decorator)),
        ));
    }

    // Docstring
    if let Some(doc) = extract_docstring(&class.body) {
        quads.push(q(&class_uri, "docstring", string_literal(&doc)));
    }

    // Line numbers
    let start_line = offset_to_line(ctx.line_table, class.start().to_u32()) as i64;
    let end_line = offset_to_line(ctx.line_table, class.end().to_u32()) as i64;
    quads.push(q(&class_uri, "startLine", integer_literal(start_line)));
    quads.push(q(&class_uri, "endLine", integer_literal(end_line)));

    // Process class body
    for stmt in &class.body {
        match stmt {
            Stmt::FunctionDef(func) => {
                let method_name = func.name.as_str();
                let method_uri = code_ns(&format!("{}/{name}/{method_name}", ctx.rel_path));
                quads.push(q(&class_uri, "hasFunction", Term::NamedNode(method_uri)));
                quads.extend(extract_function_quads(func, ctx, Some(name)));

                // Extract fields from __init__ self-assignments
                if method_name == "__init__" {
                    quads.extend(extract_init_fields_from_func(
                        &func.args, &func.body, name, ctx,
                    ));
                }
            }
            Stmt::AsyncFunctionDef(func) => {
                let method_name = func.name.as_str();
                let method_uri = code_ns(&format!("{}/{name}/{method_name}", ctx.rel_path));
                quads.push(q(&class_uri, "hasFunction", Term::NamedNode(method_uri)));
                quads.extend(extract_async_function_quads(func, ctx, Some(name)));
            }
            // Class-level annotated assignments (e.g., `name: str = "default"`)
            Stmt::AnnAssign(ann) => {
                if let Expr::Name(target) = ann.target.as_ref() {
                    let field_name = target.id.as_str();
                    let field_uri = code_ns(&format!("{}/{name}/{field_name}", ctx.rel_path));
                    quads.push(qt(&field_uri, "Field"));
                    quads.push(q(&field_uri, "name", string_literal(field_name)));
                    quads.push(q(
                        &class_uri,
                        "hasField",
                        Term::NamedNode(field_uri.clone()),
                    ));
                    quads.push(q(
                        &field_uri,
                        "fieldType",
                        string_literal(&expr_to_string(&ann.annotation)),
                    ));
                    let f_start = offset_to_line(ctx.line_table, ann.start().to_u32()) as i64;
                    quads.push(q(&field_uri, "startLine", integer_literal(f_start)));
                }
            }
            _ => {}
        }
    }

    quads
}

/// Extract fields from `self.x = ...` assignments in `__init__`.
fn extract_init_fields_from_func(
    args: &ast::Arguments,
    body: &[Stmt],
    class_name: &str,
    ctx: &ParseContext,
) -> Vec<Quad> {
    use rustpython_parser::ast::Ranged;

    let mut quads = Vec::new();
    let class_uri = code_ns(&format!("{}/{class_name}", ctx.rel_path));

    // Collect type annotations from init parameters for field type hints
    let mut param_types: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for arg in &args.args {
        let pname = arg.def.arg.as_str();
        if let Some(ann) = &arg.def.annotation {
            param_types.insert(pname.to_string(), expr_to_string(ann));
        }
    }

    for stmt in body {
        // Look for self.x = ... assignments
        if let Stmt::Assign(assign) = stmt {
            for target in &assign.targets {
                if let Expr::Attribute(attr) = target {
                    if let Expr::Name(val_name) = attr.value.as_ref() {
                        if val_name.id.as_str() == "self" {
                            let field_name = attr.attr.as_str();
                            let field_uri =
                                code_ns(&format!("{}/{class_name}/{field_name}", ctx.rel_path));
                            quads.push(qt(&field_uri, "Field"));
                            quads.push(q(&field_uri, "name", string_literal(field_name)));
                            quads.push(q(
                                &class_uri,
                                "hasField",
                                Term::NamedNode(field_uri.clone()),
                            ));

                            // Try to get type from parameter annotation
                            if let Some(ftype) = param_types.get(field_name) {
                                quads.push(q(&field_uri, "fieldType", string_literal(ftype)));
                            }

                            let f_start =
                                offset_to_line(ctx.line_table, assign.start().to_u32()) as i64;
                            quads.push(q(&field_uri, "startLine", integer_literal(f_start)));
                        }
                    }
                }
            }
        }
        // Also handle annotated self assignments: self.x: Type = value
        if let Stmt::AnnAssign(ann) = stmt {
            if let Expr::Attribute(attr) = ann.target.as_ref() {
                if let Expr::Name(val_name) = attr.value.as_ref() {
                    if val_name.id.as_str() == "self" {
                        let field_name = attr.attr.as_str();
                        let field_uri =
                            code_ns(&format!("{}/{class_name}/{field_name}", ctx.rel_path));
                        quads.push(qt(&field_uri, "Field"));
                        quads.push(q(&field_uri, "name", string_literal(field_name)));
                        quads.push(q(
                            &class_uri,
                            "hasField",
                            Term::NamedNode(field_uri.clone()),
                        ));
                        quads.push(q(
                            &field_uri,
                            "fieldType",
                            string_literal(&expr_to_string(&ann.annotation)),
                        ));
                        let f_start = offset_to_line(ctx.line_table, ann.start().to_u32()) as i64;
                        quads.push(q(&field_uri, "startLine", integer_literal(f_start)));
                    }
                }
            }
        }
    }

    quads
}

fn extract_import_quads(stmt: &Stmt, ctx: &ParseContext) -> Vec<Quad> {
    let mut quads = Vec::new();

    match stmt {
        Stmt::Import(import) => {
            for alias in &import.names {
                let module_path = alias.name.as_str();
                let import_uri = code_ns(&format!(
                    "{}/import/{}",
                    ctx.rel_path,
                    module_path.replace('.', "_")
                ));
                quads.push(qt(&import_uri, "Import"));
                quads.push(q(&import_uri, "importPath", string_literal(module_path)));
                quads.push(q(
                    ctx.module_uri,
                    "hasImport",
                    Term::NamedNode(import_uri.clone()),
                ));
                quads.push(q(
                    &import_uri,
                    "importedSymbol",
                    string_literal(module_path),
                ));
            }
        }
        Stmt::ImportFrom(import_from) => {
            let module_path = import_from
                .module
                .as_ref()
                .map(|m| m.as_str())
                .unwrap_or(".");
            let import_uri = code_ns(&format!(
                "{}/import/{}",
                ctx.rel_path,
                module_path.replace('.', "_")
            ));
            quads.push(qt(&import_uri, "Import"));
            quads.push(q(&import_uri, "importPath", string_literal(module_path)));
            quads.push(q(
                ctx.module_uri,
                "hasImport",
                Term::NamedNode(import_uri.clone()),
            ));

            for alias in &import_from.names {
                quads.push(q(
                    &import_uri,
                    "importedSymbol",
                    string_literal(alias.name.as_str()),
                ));
            }
        }
        _ => {}
    }

    quads
}

// --- LanguageLoader implementation ---

impl LanguageLoader for PythonLoader {
    fn language_id(&self) -> &str {
        "python"
    }

    fn file_extensions(&self) -> &[&str] {
        &["py"]
    }

    fn ignore_patterns(&self) -> &[&str] {
        &[
            "__pycache__",
            ".venv",
            "venv",
            ".tox",
            "dist",
            "build",
            ".eggs",
            ".mypy_cache",
            ".pytest_cache",
        ]
    }

    fn project_uri(&self, project_root: &Path) -> Option<NamedNode> {
        let toml_path = project_root.join("pyproject.toml");
        if toml_path.exists() {
            let content = std::fs::read_to_string(&toml_path).ok()?;
            let doc: toml::Value = toml::from_str(&content).ok()?;
            let name = doc
                .get("project")
                .and_then(|v| v.get("name"))
                .or_else(|| {
                    doc.get("tool")
                        .and_then(|t| t.get("poetry"))
                        .and_then(|p| p.get("name"))
                })
                .and_then(|v| v.as_str())?;
            return Some(code_ns(&format!("project/{name}")));
        }
        None
    }

    fn load_project_metadata(&self, project_root: &Path) -> Result<Vec<Quad>, LoadError> {
        let toml_path = project_root.join("pyproject.toml");
        if toml_path.exists() {
            return parse_pyproject_toml(project_root);
        }

        let req_path = project_root.join("requirements.txt");
        if req_path.exists() {
            return parse_requirements_txt(project_root);
        }

        Ok(Vec::new())
    }

    fn load_file(&self, path: &Path, project_root: &Path) -> Result<Vec<Quad>, LoadError> {
        let source = std::fs::read_to_string(path)?;
        let rel_path = path
            .strip_prefix(project_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        let module_uri = code_ns(&rel_path);
        let line_table = build_line_table(&source);

        let mut quads = vec![
            qt(&module_uri, "Module"),
            q(
                &module_uri,
                "filePath",
                string_literal(&path.to_string_lossy()),
            ),
            q(&module_uri, "relativePath", string_literal(&rel_path)),
            q(&module_uri, "language", string_literal("python")),
        ];

        // Parse the Python source
        let parsed = ast::Suite::parse(&source, &rel_path).map_err(|e| LoadError::Parse {
            file: path.to_path_buf(),
            line: None,
            message: e.to_string(),
        })?;

        let ctx = ParseContext {
            rel_path: &rel_path,
            module_uri: &module_uri,
            line_table: &line_table,
        };

        for stmt in &parsed {
            match stmt {
                Stmt::FunctionDef(func) => {
                    quads.extend(extract_function_quads(func, &ctx, None));
                }
                Stmt::AsyncFunctionDef(func) => {
                    quads.extend(extract_async_function_quads(func, &ctx, None));
                }
                Stmt::ClassDef(class) => {
                    quads.extend(extract_class_quads(class, &ctx));
                }
                Stmt::Import(_) | Stmt::ImportFrom(_) => {
                    quads.extend(extract_import_quads(stmt, &ctx));
                }
                _ => {}
            }
        }

        Ok(quads)
    }
}

use super::{code_ns, integer_literal, quad, quad_type, string_literal, LanguageLoader, LoadError};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;
use oxigraph::model::{GraphName, NamedNode, Quad, Term};
use std::path::Path;

pub struct TypeScriptLoader;

fn default_graph() -> GraphName {
    GraphName::DefaultGraph
}

fn q(subject: &NamedNode, predicate: &str, object: Term) -> Quad {
    quad(subject, predicate, object, default_graph())
}

fn qt(subject: &NamedNode, class: &str) -> Quad {
    quad_type(subject, class, default_graph())
}

// --- Line number conversion ---

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

// --- JSDoc extraction ---

fn extract_jsdoc(comments: &[oxc_ast::Comment], decl_start: u32, source: &str) -> Option<String> {
    let mut best: Option<&oxc_ast::Comment> = None;
    for comment in comments {
        if matches!(comment.kind, oxc_ast::CommentKind::Line) {
            continue;
        }
        if comment.span.end >= decl_start {
            continue;
        }
        let between = &source[comment.span.end as usize..decl_start as usize];
        if !between.trim().is_empty() {
            continue;
        }
        let comment_src = &source[comment.span.start as usize..comment.span.end as usize];
        if !comment_src.starts_with("/**") {
            continue;
        }
        match best {
            None => best = Some(comment),
            Some(prev) if comment.span.end > prev.span.end => best = Some(comment),
            _ => {}
        }
    }

    best.map(|c| {
        let raw = &source[(c.span.start as usize + 3)..(c.span.end as usize - 2)];
        let cleaned: Vec<String> = raw
            .lines()
            .map(|line| {
                let trimmed = line.trim();
                trimmed
                    .strip_prefix("* ")
                    .or_else(|| trimmed.strip_prefix('*'))
                    .unwrap_or(trimmed)
                    .to_string()
            })
            .collect();
        let start = cleaned.iter().position(|l| !l.is_empty()).unwrap_or(0);
        let end = cleaned
            .iter()
            .rposition(|l| !l.is_empty())
            .map_or(start, |e| e + 1);
        cleaned[start..end].join("\n")
    })
}

// --- package.json parsing ---

fn parse_package_json(project_root: &Path) -> Result<Vec<Quad>, LoadError> {
    let pkg_path = project_root.join("package.json");
    let content = std::fs::read_to_string(&pkg_path)?;
    let doc: serde_json::Value = serde_json::from_str(&content).map_err(|e| LoadError::Parse {
        file: pkg_path.clone(),
        line: None,
        message: e.to_string(),
    })?;

    let mut quads = Vec::new();

    let name = doc["name"].as_str().unwrap_or("unknown");
    let project_uri = code_ns(&format!("project/{name}"));

    quads.push(qt(&project_uri, "Project"));
    quads.push(q(&project_uri, "name", string_literal(name)));
    quads.push(q(&project_uri, "language", string_literal("typescript")));

    if let Some(version) = doc["version"].as_str() {
        quads.push(q(&project_uri, "version", string_literal(version)));
    }
    if let Some(desc) = doc["description"].as_str() {
        quads.push(q(&project_uri, "description", string_literal(desc)));
    }

    for dep_key in &["dependencies", "devDependencies"] {
        if let Some(deps) = doc[dep_key].as_object() {
            for (dep_name, dep_ver) in deps {
                let dep_uri = code_ns(&format!("project/{name}/dep/{dep_name}"));
                quads.push(qt(&dep_uri, "Dependency"));
                quads.push(q(&dep_uri, "name", string_literal(dep_name)));
                quads.push(q(
                    &project_uri,
                    "hasDependency",
                    Term::NamedNode(dep_uri.clone()),
                ));
                if let Some(ver) = dep_ver.as_str() {
                    quads.push(q(&dep_uri, "version", string_literal(ver)));
                }
            }
        }
    }

    Ok(quads)
}

// --- Type formatting ---

fn ts_type_to_string(annotation: &TSTypeAnnotation) -> String {
    format_ts_type(&annotation.type_annotation)
}

fn format_ts_type(ty: &TSType) -> String {
    match ty {
        TSType::TSStringKeyword(_) => "string".to_string(),
        TSType::TSNumberKeyword(_) => "number".to_string(),
        TSType::TSBooleanKeyword(_) => "boolean".to_string(),
        TSType::TSVoidKeyword(_) => "void".to_string(),
        TSType::TSAnyKeyword(_) => "any".to_string(),
        TSType::TSNullKeyword(_) => "null".to_string(),
        TSType::TSUndefinedKeyword(_) => "undefined".to_string(),
        TSType::TSNeverKeyword(_) => "never".to_string(),
        TSType::TSUnknownKeyword(_) => "unknown".to_string(),
        TSType::TSObjectKeyword(_) => "object".to_string(),
        TSType::TSBigIntKeyword(_) => "bigint".to_string(),
        TSType::TSSymbolKeyword(_) => "symbol".to_string(),
        TSType::TSTypeReference(r) => {
            let name = r.type_name.to_string();
            if let Some(params) = &r.type_arguments {
                let args: Vec<String> = params.params.iter().map(|p| format_ts_type(p)).collect();
                format!("{name}<{}>", args.join(", "))
            } else {
                name
            }
        }
        TSType::TSArrayType(a) => format!("{}[]", format_ts_type(&a.element_type)),
        TSType::TSUnionType(u) => {
            let parts: Vec<String> = u.types.iter().map(|t| format_ts_type(t)).collect();
            parts.join(" | ")
        }
        TSType::TSIntersectionType(i) => {
            let parts: Vec<String> = i.types.iter().map(|t| format_ts_type(t)).collect();
            parts.join(" & ")
        }
        TSType::TSLiteralType(l) => match &l.literal {
            TSLiteral::StringLiteral(s) => format!("\"{}\"", s.value),
            TSLiteral::NumericLiteral(n) => n.value.to_string(),
            TSLiteral::BooleanLiteral(b) => b.value.to_string(),
            _ => "literal".to_string(),
        },
        TSType::TSFunctionType(_) => "Function".to_string(),
        _ => "unknown".to_string(),
    }
}

// --- Helpers ---

fn binding_pattern_name(pat: &BindingPattern) -> Option<String> {
    match pat {
        BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

fn property_key_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => Some(n.value.to_string()),
        _ => None,
    }
}

fn expr_to_name(expr: &Expression) -> String {
    match expr {
        Expression::Identifier(id) => id.name.to_string(),
        Expression::StaticMemberExpression(m) => {
            format!("{}.{}", expr_to_name(&m.object), m.property.name)
        }
        _ => "unknown".to_string(),
    }
}

/// Context passed through statement processing for JSDoc span tracking.
struct ExtractCtx<'a> {
    module_uri: &'a NamedNode,
    rel_path: &'a str,
    line_table: &'a [u32],
    comments: &'a [oxc_ast::Comment],
    source: &'a str,
}

// --- AST extraction ---

fn extract_function_quads(
    func: &Function,
    ctx: &ExtractCtx,
    is_export: bool,
    jsdoc_start: u32,
) -> Vec<Quad> {
    let Some(id) = &func.id else { return vec![] };
    let name = id.name.as_str();
    let fn_uri = code_ns(&format!("{}/{name}", ctx.rel_path));
    let mut quads = Vec::new();

    quads.push(qt(&fn_uri, "Function"));
    quads.push(q(&fn_uri, "name", string_literal(name)));
    quads.push(q(
        &fn_uri,
        "definedIn",
        Term::NamedNode(ctx.module_uri.clone()),
    ));
    quads.push(q(
        ctx.module_uri,
        "hasFunction",
        Term::NamedNode(fn_uri.clone()),
    ));

    let vis = if is_export { "export" } else { "private" };
    quads.push(q(&fn_uri, "visibility", string_literal(vis)));

    let start = offset_to_line(ctx.line_table, func.span.start);
    let end = offset_to_line(ctx.line_table, func.span.end);
    quads.push(q(&fn_uri, "startLine", integer_literal(start as i64)));
    quads.push(q(&fn_uri, "endLine", integer_literal(end as i64)));

    for param in &func.params.items {
        if let Some(pname) = binding_pattern_name(&param.pattern) {
            quads.push(q(&fn_uri, "parameter", string_literal(&pname)));
        }
    }

    if let Some(ret) = &func.return_type {
        quads.push(q(
            &fn_uri,
            "returnType",
            string_literal(&ts_type_to_string(ret)),
        ));
    }

    if let Some(doc) = extract_jsdoc(ctx.comments, jsdoc_start, ctx.source) {
        quads.push(q(&fn_uri, "docstring", string_literal(&doc)));
    }

    quads
}

fn extract_arrow_fn_quads(
    name: &str,
    arrow: &ArrowFunctionExpression,
    ctx: &ExtractCtx,
    is_export: bool,
    jsdoc_start: u32,
) -> Vec<Quad> {
    let fn_uri = code_ns(&format!("{}/{name}", ctx.rel_path));
    let mut quads = Vec::new();

    quads.push(qt(&fn_uri, "Function"));
    quads.push(q(&fn_uri, "name", string_literal(name)));
    quads.push(q(
        &fn_uri,
        "definedIn",
        Term::NamedNode(ctx.module_uri.clone()),
    ));
    quads.push(q(
        ctx.module_uri,
        "hasFunction",
        Term::NamedNode(fn_uri.clone()),
    ));

    let vis = if is_export { "export" } else { "private" };
    quads.push(q(&fn_uri, "visibility", string_literal(vis)));

    let start = offset_to_line(ctx.line_table, arrow.span.start);
    let end = offset_to_line(ctx.line_table, arrow.span.end);
    quads.push(q(&fn_uri, "startLine", integer_literal(start as i64)));
    quads.push(q(&fn_uri, "endLine", integer_literal(end as i64)));

    for param in &arrow.params.items {
        if let Some(pname) = binding_pattern_name(&param.pattern) {
            quads.push(q(&fn_uri, "parameter", string_literal(&pname)));
        }
    }

    if let Some(ret) = &arrow.return_type {
        quads.push(q(
            &fn_uri,
            "returnType",
            string_literal(&ts_type_to_string(ret)),
        ));
    }

    if let Some(doc) = extract_jsdoc(ctx.comments, jsdoc_start, ctx.source) {
        quads.push(q(&fn_uri, "docstring", string_literal(&doc)));
    }

    quads
}

fn extract_class_quads(
    class: &Class,
    ctx: &ExtractCtx,
    is_export: bool,
    jsdoc_start: u32,
) -> Vec<Quad> {
    let Some(id) = &class.id else { return vec![] };
    let name = id.name.as_str();
    let class_uri = code_ns(&format!("{}/{name}", ctx.rel_path));
    let mut quads = Vec::new();

    quads.push(qt(&class_uri, "Class"));
    quads.push(q(&class_uri, "name", string_literal(name)));
    quads.push(q(
        &class_uri,
        "definedIn",
        Term::NamedNode(ctx.module_uri.clone()),
    ));

    let vis = if is_export { "export" } else { "private" };
    quads.push(q(&class_uri, "visibility", string_literal(vis)));

    let start = offset_to_line(ctx.line_table, class.span.start);
    let end = offset_to_line(ctx.line_table, class.span.end);
    quads.push(q(&class_uri, "startLine", integer_literal(start as i64)));
    quads.push(q(&class_uri, "endLine", integer_literal(end as i64)));

    for imp in &class.implements {
        quads.push(q(
            &class_uri,
            "implements",
            string_literal(&imp.expression.to_string()),
        ));
    }

    if let Some(super_class) = &class.super_class {
        quads.push(q(
            &class_uri,
            "extends",
            string_literal(&expr_to_name(super_class)),
        ));
    }

    for element in &class.body.body {
        match element {
            ClassElement::MethodDefinition(method) => {
                if let Some(method_name) = property_key_name(&method.key) {
                    let fn_uri = code_ns(&format!("{}/{name}/{method_name}", ctx.rel_path));
                    quads.push(qt(&fn_uri, "Function"));
                    quads.push(q(&fn_uri, "name", string_literal(&method_name)));
                    quads.push(q(
                        &fn_uri,
                        "definedIn",
                        Term::NamedNode(ctx.module_uri.clone()),
                    ));
                    quads.push(q(
                        &class_uri,
                        "hasFunction",
                        Term::NamedNode(fn_uri.clone()),
                    ));

                    let m_start = offset_to_line(ctx.line_table, method.span.start);
                    let m_end = offset_to_line(ctx.line_table, method.span.end);
                    quads.push(q(&fn_uri, "startLine", integer_literal(m_start as i64)));
                    quads.push(q(&fn_uri, "endLine", integer_literal(m_end as i64)));

                    for param in &method.value.params.items {
                        if let Some(pname) = binding_pattern_name(&param.pattern) {
                            quads.push(q(&fn_uri, "parameter", string_literal(&pname)));
                        }
                    }

                    if let Some(ret) = &method.value.return_type {
                        quads.push(q(
                            &fn_uri,
                            "returnType",
                            string_literal(&ts_type_to_string(ret)),
                        ));
                    }
                }
            }
            ClassElement::PropertyDefinition(prop) => {
                if let Some(field_name) = property_key_name(&prop.key) {
                    let field_uri = code_ns(&format!("{}/{name}/{field_name}", ctx.rel_path));
                    quads.push(qt(&field_uri, "Field"));
                    quads.push(q(&field_uri, "name", string_literal(&field_name)));
                    quads.push(q(&class_uri, "hasField", Term::NamedNode(field_uri.clone())));
                    if let Some(ann) = &prop.type_annotation {
                        quads.push(q(&field_uri, "fieldType", string_literal(&ts_type_to_string(ann))));
                    }
                    quads.push(q(&field_uri, "optional", string_literal(if prop.optional { "true" } else { "false" })));
                    let f_start = offset_to_line(ctx.line_table, prop.span.start);
                    quads.push(q(&field_uri, "startLine", integer_literal(f_start as i64)));
                }
            }
            _ => {}
        }
    }

    if let Some(doc) = extract_jsdoc(ctx.comments, jsdoc_start, ctx.source) {
        quads.push(q(&class_uri, "docstring", string_literal(&doc)));
    }

    quads
}

fn extract_interface_quads(
    iface: &TSInterfaceDeclaration,
    ctx: &ExtractCtx,
    is_export: bool,
    jsdoc_start: u32,
) -> Vec<Quad> {
    let name = iface.id.name.as_str();
    let uri = code_ns(&format!("{}/{name}", ctx.rel_path));
    let mut quads = Vec::new();

    quads.push(qt(&uri, "Trait"));
    quads.push(q(&uri, "name", string_literal(name)));
    quads.push(q(
        &uri,
        "definedIn",
        Term::NamedNode(ctx.module_uri.clone()),
    ));

    let vis = if is_export { "export" } else { "private" };
    quads.push(q(&uri, "visibility", string_literal(vis)));

    let start = offset_to_line(ctx.line_table, iface.span.start);
    let end = offset_to_line(ctx.line_table, iface.span.end);
    quads.push(q(&uri, "startLine", integer_literal(start as i64)));
    quads.push(q(&uri, "endLine", integer_literal(end as i64)));

    for heritage in &iface.extends {
        quads.push(q(
            &uri,
            "extends",
            string_literal(&expr_to_name(&heritage.expression)),
        ));
    }

    for sig in &iface.body.body {
        match sig {
            TSSignature::TSMethodSignature(method) => {
                if let Some(method_name) = property_key_name(&method.key) {
                    quads.push(q(&uri, "hasMethod", string_literal(&method_name)));
                }
            }
            TSSignature::TSPropertySignature(prop) => {
                if let Some(field_name) = property_key_name(&prop.key) {
                    let field_uri = code_ns(&format!("{}/{name}/{field_name}", ctx.rel_path));
                    quads.push(qt(&field_uri, "Field"));
                    quads.push(q(&field_uri, "name", string_literal(&field_name)));
                    quads.push(q(&uri, "hasField", Term::NamedNode(field_uri.clone())));
                    if let Some(ann) = &prop.type_annotation {
                        quads.push(q(&field_uri, "fieldType", string_literal(&ts_type_to_string(ann))));
                    }
                    quads.push(q(&field_uri, "optional", string_literal(if prop.optional { "true" } else { "false" })));
                    let f_start = offset_to_line(ctx.line_table, prop.span.start);
                    quads.push(q(&field_uri, "startLine", integer_literal(f_start as i64)));
                }
            }
            _ => {}
        }
    }

    if let Some(doc) = extract_jsdoc(ctx.comments, jsdoc_start, ctx.source) {
        quads.push(q(&uri, "docstring", string_literal(&doc)));
    }

    quads
}

fn extract_type_alias_quads(
    alias: &TSTypeAliasDeclaration,
    ctx: &ExtractCtx,
    is_export: bool,
    jsdoc_start: u32,
) -> Vec<Quad> {
    let name = alias.id.name.as_str();
    let uri = code_ns(&format!("{}/{name}", ctx.rel_path));
    let mut quads = Vec::new();

    quads.push(qt(&uri, "Class"));
    quads.push(q(&uri, "name", string_literal(name)));
    quads.push(q(&uri, "kind", string_literal("type_alias")));
    quads.push(q(
        &uri,
        "definedIn",
        Term::NamedNode(ctx.module_uri.clone()),
    ));

    let vis = if is_export { "export" } else { "private" };
    quads.push(q(&uri, "visibility", string_literal(vis)));

    let start = offset_to_line(ctx.line_table, alias.span.start);
    let end = offset_to_line(ctx.line_table, alias.span.end);
    quads.push(q(&uri, "startLine", integer_literal(start as i64)));
    quads.push(q(&uri, "endLine", integer_literal(end as i64)));

    if let Some(doc) = extract_jsdoc(ctx.comments, jsdoc_start, ctx.source) {
        quads.push(q(&uri, "docstring", string_literal(&doc)));
    }

    quads
}

fn extract_enum_quads(
    ts_enum: &TSEnumDeclaration,
    ctx: &ExtractCtx,
    is_export: bool,
    jsdoc_start: u32,
) -> Vec<Quad> {
    let name = ts_enum.id.name.as_str();
    let uri = code_ns(&format!("{}/{name}", ctx.rel_path));
    let mut quads = Vec::new();

    quads.push(qt(&uri, "Enum"));
    quads.push(q(&uri, "name", string_literal(name)));
    quads.push(q(
        &uri,
        "definedIn",
        Term::NamedNode(ctx.module_uri.clone()),
    ));

    let vis = if is_export { "export" } else { "private" };
    quads.push(q(&uri, "visibility", string_literal(vis)));

    let start = offset_to_line(ctx.line_table, ts_enum.span.start);
    let end = offset_to_line(ctx.line_table, ts_enum.span.end);
    quads.push(q(&uri, "startLine", integer_literal(start as i64)));
    quads.push(q(&uri, "endLine", integer_literal(end as i64)));

    for member in &ts_enum.body.members {
        let variant_name = match &member.id {
            TSEnumMemberName::Identifier(id) => id.name.to_string(),
            TSEnumMemberName::String(s) => s.value.to_string(),
            _ => continue,
        };
        quads.push(q(&uri, "hasVariant", string_literal(&variant_name)));
    }

    if let Some(doc) = extract_jsdoc(ctx.comments, jsdoc_start, ctx.source) {
        quads.push(q(&uri, "docstring", string_literal(&doc)));
    }

    quads
}

fn extract_import_quads(import: &ImportDeclaration, module_uri: &NamedNode) -> Vec<Quad> {
    let path = import.source.value.as_str();
    let import_uri = code_ns(&format!("import/{}", path.replace('/', "_")));
    let mut quads = vec![
        qt(&import_uri, "Import"),
        q(&import_uri, "importPath", string_literal(path)),
        q(module_uri, "hasImport", Term::NamedNode(import_uri.clone())),
    ];

    // Extract named import specifiers
    if let Some(specifiers) = &import.specifiers {
        for spec in specifiers {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    quads.push(q(&import_uri, "importedSymbol", string_literal(&s.local.name)));
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    quads.push(q(&import_uri, "importedSymbol", string_literal(&s.local.name)));
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    quads.push(q(
                        &import_uri,
                        "importedSymbol",
                        string_literal(&format!("* as {}", s.local.name)),
                    ));
                }
            }
        }
    }

    quads
}

fn extract_var_decl_quads(
    decl: &VariableDeclaration,
    ctx: &ExtractCtx,
    is_export: bool,
    jsdoc_start: u32,
) -> Vec<Quad> {
    let mut quads = Vec::new();
    for declarator in &decl.declarations {
        let Some(name) = binding_pattern_name(&declarator.id) else {
            continue;
        };
        let Some(init) = &declarator.init else {
            continue;
        };
        match init.without_parentheses() {
            Expression::ArrowFunctionExpression(arrow) => {
                quads.extend(extract_arrow_fn_quads(
                    &name,
                    arrow,
                    ctx,
                    is_export,
                    jsdoc_start,
                ));
            }
            Expression::FunctionExpression(func) => {
                let fn_uri = code_ns(&format!("{}/{name}", ctx.rel_path));
                quads.push(qt(&fn_uri, "Function"));
                quads.push(q(&fn_uri, "name", string_literal(&name)));
                quads.push(q(
                    &fn_uri,
                    "definedIn",
                    Term::NamedNode(ctx.module_uri.clone()),
                ));
                quads.push(q(
                    ctx.module_uri,
                    "hasFunction",
                    Term::NamedNode(fn_uri.clone()),
                ));

                let vis = if is_export { "export" } else { "private" };
                quads.push(q(&fn_uri, "visibility", string_literal(vis)));

                let start = offset_to_line(ctx.line_table, func.span.start);
                let end = offset_to_line(ctx.line_table, func.span.end);
                quads.push(q(&fn_uri, "startLine", integer_literal(start as i64)));
                quads.push(q(&fn_uri, "endLine", integer_literal(end as i64)));

                for param in &func.params.items {
                    if let Some(pname) = binding_pattern_name(&param.pattern) {
                        quads.push(q(&fn_uri, "parameter", string_literal(&pname)));
                    }
                }
                if let Some(ret) = &func.return_type {
                    quads.push(q(
                        &fn_uri,
                        "returnType",
                        string_literal(&ts_type_to_string(ret)),
                    ));
                }
            }
            _ => {}
        }
    }
    quads
}

/// Process a top-level statement and extract RDF quads.
fn process_statement(stmt: &Statement, ctx: &ExtractCtx) -> Vec<Quad> {
    match stmt {
        Statement::FunctionDeclaration(func) => {
            extract_function_quads(func, ctx, false, func.span.start)
        }
        Statement::ClassDeclaration(class) => {
            extract_class_quads(class, ctx, false, class.span.start)
        }
        Statement::TSTypeAliasDeclaration(alias) => {
            extract_type_alias_quads(alias, ctx, false, alias.span.start)
        }
        Statement::TSInterfaceDeclaration(iface) => {
            extract_interface_quads(iface, ctx, false, iface.span.start)
        }
        Statement::TSEnumDeclaration(ts_enum) => {
            extract_enum_quads(ts_enum, ctx, false, ts_enum.span.start)
        }
        Statement::VariableDeclaration(decl) => {
            extract_var_decl_quads(decl, ctx, false, decl.span.start)
        }
        Statement::ImportDeclaration(import) => extract_import_quads(import, ctx.module_uri),
        Statement::ExportNamedDeclaration(export) => {
            if let Some(decl) = &export.declaration {
                // Pass export span start for JSDoc (comment is before `export`)
                process_export_declaration(decl, ctx, export.span.start)
            } else {
                vec![]
            }
        }
        Statement::ExportDefaultDeclaration(export) => {
            let js = export.span.start;
            match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                    extract_function_quads(func, ctx, true, js)
                }
                ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                    extract_class_quads(class, ctx, true, js)
                }
                _ => vec![],
            }
        }
        _ => vec![],
    }
}

fn process_export_declaration(decl: &Declaration, ctx: &ExtractCtx, jsdoc_start: u32) -> Vec<Quad> {
    match decl {
        Declaration::FunctionDeclaration(func) => {
            extract_function_quads(func, ctx, true, jsdoc_start)
        }
        Declaration::ClassDeclaration(class) => extract_class_quads(class, ctx, true, jsdoc_start),
        Declaration::TSTypeAliasDeclaration(alias) => {
            extract_type_alias_quads(alias, ctx, true, jsdoc_start)
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            extract_interface_quads(iface, ctx, true, jsdoc_start)
        }
        Declaration::TSEnumDeclaration(ts_enum) => {
            extract_enum_quads(ts_enum, ctx, true, jsdoc_start)
        }
        Declaration::VariableDeclaration(var_decl) => {
            extract_var_decl_quads(var_decl, ctx, true, jsdoc_start)
        }
        _ => vec![],
    }
}

// --- File parsing ---

fn parse_ts_file(path: &Path, project_root: &Path) -> Result<Vec<Quad>, LoadError> {
    let source = std::fs::read_to_string(path)?;
    let allocator = Allocator::default();

    let source_type = SourceType::from_path(path).map_err(|_| LoadError::Parse {
        file: path.to_path_buf(),
        line: None,
        message: format!("Unsupported file extension: {}", path.display()),
    })?;

    let ret = Parser::new(&allocator, &source, source_type).parse();

    if !ret.errors.is_empty() {
        let first = &ret.errors[0];
        let line = first
            .labels
            .as_ref()
            .and_then(|labels| labels.first())
            .map(|label| {
                let line_table = build_line_table(&source);
                offset_to_line(&line_table, label.offset() as u32)
            });
        return Err(LoadError::Parse {
            file: path.to_path_buf(),
            line,
            message: first.to_string(),
        });
    }

    let rel_path = path
        .strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let module_uri = code_ns(&rel_path);
    let line_table = build_line_table(&source);

    let ctx = ExtractCtx {
        module_uri: &module_uri,
        rel_path: &rel_path,
        line_table: &line_table,
        comments: &ret.program.comments,
        source: &source,
    };

    let mut quads = Vec::new();

    quads.push(qt(&module_uri, "Module"));
    quads.push(q(
        &module_uri,
        "filePath",
        string_literal(&path.to_string_lossy()),
    ));
    quads.push(q(&module_uri, "relativePath", string_literal(&rel_path)));
    quads.push(q(&module_uri, "language", string_literal("typescript")));

    for stmt in &ret.program.body {
        quads.extend(process_statement(stmt, &ctx));
    }

    Ok(quads)
}

// --- LanguageLoader implementation ---

impl LanguageLoader for TypeScriptLoader {
    fn language_id(&self) -> &str {
        "typescript"
    }

    fn file_extensions(&self) -> &[&str] {
        &["ts", "tsx", "js", "jsx"]
    }

    fn ignore_patterns(&self) -> &[&str] {
        &["node_modules", "dist", "build", ".next"]
    }

    fn load_file(&self, path: &Path, project_root: &Path) -> Result<Vec<Quad>, LoadError> {
        parse_ts_file(path, project_root)
    }

    fn load_project_metadata(&self, project_root: &Path) -> Result<Vec<Quad>, LoadError> {
        parse_package_json(project_root)
    }

    fn project_uri(&self, project_root: &Path) -> Option<NamedNode> {
        let pkg_path = project_root.join("package.json");
        let content = std::fs::read_to_string(&pkg_path).ok()?;
        let doc: serde_json::Value = serde_json::from_str(&content).ok()?;
        let name = doc["name"].as_str()?;
        Some(code_ns(&format!("project/{name}")))
    }
}

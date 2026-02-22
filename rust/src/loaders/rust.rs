use super::{LanguageLoader, LoadError};
use oxigraph::model::{GraphName, Literal, NamedNode, NamedOrBlankNode, Quad, Term};
use quote::ToTokens;
use std::path::Path;

const CODE_NS: &str = "https://oxigraph.org/code#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

pub struct RustLoader;

// --- RDF helper functions ---

fn code_ns(local: &str) -> NamedNode {
    NamedNode::new(format!("{CODE_NS}{local}")).unwrap()
}

fn rdf_type() -> NamedNode {
    NamedNode::new(RDF_TYPE).unwrap()
}

fn string_literal(value: &str) -> Term {
    Term::Literal(Literal::new_simple_literal(value))
}

fn integer_literal(value: i64) -> Term {
    Term::Literal(Literal::new_typed_literal(
        value.to_string(),
        NamedNode::new("http://www.w3.org/2001/XMLSchema#integer").unwrap(),
    ))
}

fn default_graph() -> GraphName {
    GraphName::NamedNode(code_ns("rust"))
}

fn quad(subject: &NamedNode, predicate: &str, object: Term) -> Quad {
    Quad::new(
        NamedOrBlankNode::NamedNode(subject.clone()),
        code_ns(predicate),
        object,
        default_graph(),
    )
}

fn quad_type(subject: &NamedNode, class: &str) -> Quad {
    Quad::new(
        NamedOrBlankNode::NamedNode(subject.clone()),
        rdf_type(),
        Term::NamedNode(code_ns(class)),
        default_graph(),
    )
}

// --- Cargo.toml parsing ---

fn parse_cargo_toml(project_root: &Path) -> Result<Vec<Quad>, LoadError> {
    let cargo_path = project_root.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_path)?;
    let doc: toml::Value = toml::from_str(&content).map_err(|e| LoadError::Parse {
        file: cargo_path.clone(),
        line: None,
        message: e.to_string(),
    })?;

    let mut quads = Vec::new();
    let empty = toml::map::Map::new();
    let table = doc.as_table().unwrap_or(&empty);

    if let Some(pkg) = table.get("package").and_then(|v| v.as_table()) {
        let name = pkg
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let project_uri = code_ns(&format!("project/{name}"));

        quads.push(quad_type(&project_uri, "Project"));
        quads.push(quad(&project_uri, "name", string_literal(name)));

        if let Some(version) = pkg.get("version").and_then(|v| v.as_str()) {
            quads.push(quad(&project_uri, "version", string_literal(version)));
        }
        if let Some(desc) = pkg.get("description").and_then(|v| v.as_str()) {
            quads.push(quad(&project_uri, "description", string_literal(desc)));
        }
        if let Some(edition) = pkg.get("edition").and_then(|v| v.as_str()) {
            quads.push(quad(&project_uri, "edition", string_literal(edition)));
        }

        // Dependencies
        if let Some(deps) = table.get("dependencies").and_then(|v| v.as_table()) {
            for (dep_name, dep_val) in deps {
                let dep_uri = code_ns(&format!("project/{name}/dep/{dep_name}"));
                quads.push(quad_type(&dep_uri, "Dependency"));
                quads.push(quad(&dep_uri, "name", string_literal(dep_name)));
                quads.push(quad(
                    &project_uri,
                    "hasDependency",
                    Term::NamedNode(dep_uri.clone()),
                ));

                let version_str = match dep_val {
                    toml::Value::String(v) => Some(v.clone()),
                    toml::Value::Table(t) => {
                        t.get("version").and_then(|v| v.as_str()).map(String::from)
                    }
                    _ => None,
                };
                if let Some(ver) = version_str {
                    quads.push(quad(&dep_uri, "version", string_literal(&ver)));
                }
            }
        }
    }

    Ok(quads)
}

// --- .rs AST extraction ---

fn extract_docstring(attrs: &[syn::Attribute]) -> Option<String> {
    let docs: Vec<String> = attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("doc") {
                if let syn::Meta::NameValue(nv) = &attr.meta {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = &nv.value
                    {
                        return Some(s.value().trim().to_string());
                    }
                }
            }
            None
        })
        .collect();

    if docs.is_empty() {
        None
    } else {
        Some(docs.join("\n"))
    }
}

fn visibility_str(vis: &syn::Visibility) -> &str {
    match vis {
        syn::Visibility::Public(_) => "pub",
        syn::Visibility::Restricted(_) => "restricted",
        syn::Visibility::Inherited => "private",
    }
}

fn type_to_string(ty: &syn::Type) -> String {
    ty.to_token_stream().to_string()
}

fn return_type_string(output: &syn::ReturnType) -> Option<String> {
    match output {
        syn::ReturnType::Default => None,
        syn::ReturnType::Type(_, ty) => Some(type_to_string(ty)),
    }
}

fn extract_fn_quads(
    func: &syn::ItemFn,
    module_uri: &NamedNode,
    rel_path: &str,
) -> Vec<Quad> {
    let name = func.sig.ident.to_string();
    let fn_uri = code_ns(&format!("{rel_path}/{name}"));
    let mut quads = Vec::new();

    quads.push(quad_type(&fn_uri, "Function"));
    quads.push(quad(&fn_uri, "name", string_literal(&name)));
    quads.push(quad(
        &fn_uri,
        "definedIn",
        Term::NamedNode(module_uri.clone()),
    ));
    quads.push(quad(
        &fn_uri,
        "visibility",
        string_literal(visibility_str(&func.vis)),
    ));

    let start = func.sig.ident.span().start().line;
    quads.push(quad(&fn_uri, "startLine", integer_literal(start as i64)));
    let end = func.block.brace_token.span.close().start().line;
    quads.push(quad(&fn_uri, "endLine", integer_literal(end as i64)));

    for param in &func.sig.inputs {
        match param {
            syn::FnArg::Receiver(_) => {
                quads.push(quad(&fn_uri, "parameter", string_literal("self")));
            }
            syn::FnArg::Typed(pat_type) => {
                let param_name = pat_type.pat.to_token_stream().to_string();
                quads.push(quad(&fn_uri, "parameter", string_literal(&param_name)));
            }
        }
    }

    if let Some(ret) = return_type_string(&func.sig.output) {
        quads.push(quad(&fn_uri, "returnType", string_literal(&ret)));
    }

    if let Some(doc) = extract_docstring(&func.attrs) {
        quads.push(quad(&fn_uri, "docstring", string_literal(&doc)));
    }

    quads
}

fn extract_struct_quads(
    item: &syn::ItemStruct,
    module_uri: &NamedNode,
    rel_path: &str,
) -> Vec<Quad> {
    let name = item.ident.to_string();
    let uri = code_ns(&format!("{rel_path}/{name}"));
    let mut quads = Vec::new();

    quads.push(quad_type(&uri, "Class"));
    quads.push(quad(&uri, "name", string_literal(&name)));
    quads.push(quad(
        &uri,
        "definedIn",
        Term::NamedNode(module_uri.clone()),
    ));
    quads.push(quad(
        &uri,
        "visibility",
        string_literal(visibility_str(&item.vis)),
    ));

    let start = item.ident.span().start().line;
    quads.push(quad(&uri, "startLine", integer_literal(start as i64)));

    if let syn::Fields::Named(fields) = &item.fields {
        if let Some(last) = fields.named.last() {
            let end = last.ident.as_ref().map_or(start, |i| i.span().start().line);
            quads.push(quad(&uri, "endLine", integer_literal(end as i64 + 1)));
        }
        for field in &fields.named {
            if let Some(ident) = &field.ident {
                quads.push(quad(&uri, "hasField", string_literal(&ident.to_string())));
            }
        }
    }

    if let Some(doc) = extract_docstring(&item.attrs) {
        quads.push(quad(&uri, "docstring", string_literal(&doc)));
    }

    quads
}

fn extract_enum_quads(
    item: &syn::ItemEnum,
    module_uri: &NamedNode,
    rel_path: &str,
) -> Vec<Quad> {
    let name = item.ident.to_string();
    let uri = code_ns(&format!("{rel_path}/{name}"));
    let mut quads = Vec::new();

    quads.push(quad_type(&uri, "Enum"));
    quads.push(quad(&uri, "name", string_literal(&name)));
    quads.push(quad(
        &uri,
        "definedIn",
        Term::NamedNode(module_uri.clone()),
    ));
    quads.push(quad(
        &uri,
        "visibility",
        string_literal(visibility_str(&item.vis)),
    ));

    let start = item.ident.span().start().line;
    quads.push(quad(&uri, "startLine", integer_literal(start as i64)));

    for variant in &item.variants {
        quads.push(quad(
            &uri,
            "hasVariant",
            string_literal(&variant.ident.to_string()),
        ));
    }

    if let Some(last) = item.variants.last() {
        let end = last.ident.span().start().line;
        quads.push(quad(&uri, "endLine", integer_literal(end as i64 + 1)));
    }

    if let Some(doc) = extract_docstring(&item.attrs) {
        quads.push(quad(&uri, "docstring", string_literal(&doc)));
    }

    quads
}

fn extract_trait_quads(
    item: &syn::ItemTrait,
    module_uri: &NamedNode,
    rel_path: &str,
) -> Vec<Quad> {
    let name = item.ident.to_string();
    let uri = code_ns(&format!("{rel_path}/{name}"));
    let mut quads = Vec::new();

    quads.push(quad_type(&uri, "Trait"));
    quads.push(quad(&uri, "name", string_literal(&name)));
    quads.push(quad(
        &uri,
        "definedIn",
        Term::NamedNode(module_uri.clone()),
    ));
    quads.push(quad(
        &uri,
        "visibility",
        string_literal(visibility_str(&item.vis)),
    ));

    let start = item.ident.span().start().line;
    quads.push(quad(&uri, "startLine", integer_literal(start as i64)));

    for trait_item in &item.items {
        if let syn::TraitItem::Fn(method) = trait_item {
            let method_name = method.sig.ident.to_string();
            quads.push(quad(&uri, "hasMethod", string_literal(&method_name)));
        }
    }

    if let Some(doc) = extract_docstring(&item.attrs) {
        quads.push(quad(&uri, "docstring", string_literal(&doc)));
    }

    quads
}

fn extract_impl_quads(
    item: &syn::ItemImpl,
    module_uri: &NamedNode,
    rel_path: &str,
) -> Vec<Quad> {
    let mut quads = Vec::new();

    let type_name = type_to_string(&item.self_ty);
    let type_local = type_name
        .split("::")
        .last()
        .unwrap_or(&type_name)
        .trim()
        .to_string();
    let type_uri = code_ns(&format!("{rel_path}/{type_local}"));

    if let Some((_, trait_path, _)) = &item.trait_ {
        let trait_name = trait_path.to_token_stream().to_string();
        quads.push(quad(&type_uri, "implements", string_literal(&trait_name)));
    }

    for impl_item in &item.items {
        if let syn::ImplItem::Fn(method) = impl_item {
            let method_name = method.sig.ident.to_string();
            let fn_uri = code_ns(&format!("{rel_path}/{type_local}/{method_name}"));

            quads.push(quad_type(&fn_uri, "Function"));
            quads.push(quad(&fn_uri, "name", string_literal(&method_name)));
            quads.push(quad(
                &fn_uri,
                "definedIn",
                Term::NamedNode(module_uri.clone()),
            ));
            quads.push(quad(
                &fn_uri,
                "visibility",
                string_literal(visibility_str(&method.vis)),
            ));
            quads.push(quad(
                &type_uri,
                "hasFunction",
                Term::NamedNode(fn_uri.clone()),
            ));

            let start = method.sig.ident.span().start().line;
            quads.push(quad(&fn_uri, "startLine", integer_literal(start as i64)));
            let end = method.block.brace_token.span.close().start().line;
            quads.push(quad(&fn_uri, "endLine", integer_literal(end as i64)));

            for param in &method.sig.inputs {
                match param {
                    syn::FnArg::Receiver(_) => {
                        quads.push(quad(&fn_uri, "parameter", string_literal("self")));
                    }
                    syn::FnArg::Typed(pat_type) => {
                        let param_name = pat_type.pat.to_token_stream().to_string();
                        quads.push(quad(&fn_uri, "parameter", string_literal(&param_name)));
                    }
                }
            }

            if let Some(ret) = return_type_string(&method.sig.output) {
                quads.push(quad(&fn_uri, "returnType", string_literal(&ret)));
            }

            if let Some(doc) = extract_docstring(&method.attrs) {
                quads.push(quad(&fn_uri, "docstring", string_literal(&doc)));
            }
        }
    }

    quads
}

fn extract_use_quads(item: &syn::ItemUse, module_uri: &NamedNode) -> Vec<Quad> {
    let import_path = item.tree.to_token_stream().to_string();
    let import_uri = code_ns(&format!(
        "import/{}",
        import_path.replace("::", "/").replace(' ', "")
    ));
    vec![
        quad_type(&import_uri, "Import"),
        quad(&import_uri, "importPath", string_literal(&import_path)),
        quad(
            module_uri,
            "hasImport",
            Term::NamedNode(import_uri),
        ),
    ]
}

fn extract_mod_quads(
    item: &syn::ItemMod,
    module_uri: &NamedNode,
    rel_path: &str,
) -> Vec<Quad> {
    let name = item.ident.to_string();
    if item.content.is_some() {
        return vec![];
    }
    let mod_uri = code_ns(&format!("{rel_path}/{name}"));
    vec![
        quad_type(&mod_uri, "Module"),
        quad(&mod_uri, "name", string_literal(&name)),
        quad(
            module_uri,
            "hasModule",
            Term::NamedNode(mod_uri),
        ),
    ]
}

fn parse_rs_file(path: &Path, project_root: &Path) -> Result<Vec<Quad>, LoadError> {
    let content = std::fs::read_to_string(path)?;
    let syntax = syn::parse_file(&content).map_err(|e| LoadError::Parse {
        file: path.to_path_buf(),
        line: Some(e.span().start().line),
        message: e.to_string(),
    })?;

    let rel_path = path
        .strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let module_uri = code_ns(&rel_path);
    let mut quads = Vec::new();

    quads.push(quad_type(&module_uri, "Module"));
    quads.push(quad(
        &module_uri,
        "filePath",
        string_literal(&path.to_string_lossy()),
    ));
    quads.push(quad(
        &module_uri,
        "relativePath",
        string_literal(&rel_path),
    ));
    quads.push(quad(&module_uri, "language", string_literal("rust")));

    for item in &syntax.items {
        match item {
            syn::Item::Fn(f) => quads.extend(extract_fn_quads(f, &module_uri, &rel_path)),
            syn::Item::Struct(s) => {
                quads.extend(extract_struct_quads(s, &module_uri, &rel_path))
            }
            syn::Item::Enum(e) => quads.extend(extract_enum_quads(e, &module_uri, &rel_path)),
            syn::Item::Trait(t) => {
                quads.extend(extract_trait_quads(t, &module_uri, &rel_path))
            }
            syn::Item::Impl(i) => quads.extend(extract_impl_quads(i, &module_uri, &rel_path)),
            syn::Item::Use(u) => quads.extend(extract_use_quads(u, &module_uri)),
            syn::Item::Mod(m) => quads.extend(extract_mod_quads(m, &module_uri, &rel_path)),
            _ => {}
        }
    }

    Ok(quads)
}

// --- LanguageLoader implementation ---

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

    fn load_file(&self, path: &Path, project_root: &Path) -> Result<Vec<Quad>, LoadError> {
        parse_rs_file(path, project_root)
    }

    fn load_project_metadata(&self, project_root: &Path) -> Result<Vec<Quad>, LoadError> {
        parse_cargo_toml(project_root)
    }
}

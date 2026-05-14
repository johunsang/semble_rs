use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use tree_sitter::{Language, Node, Parser};

fn get_language(name: &str) -> Option<Language> {
    let lang_fn = match name {
        "rust" => tree_sitter_rust::LANGUAGE,
        "python" => tree_sitter_python::LANGUAGE,
        "javascript" => tree_sitter_javascript::LANGUAGE,
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        "go" => tree_sitter_go::LANGUAGE,
        "java" => tree_sitter_java::LANGUAGE,
        "c" => tree_sitter_c::LANGUAGE,
        "cpp" => tree_sitter_cpp::LANGUAGE,
        "kotlin" => tree_sitter_kotlin_ng::LANGUAGE,
        _ => return None,
    };
    Some(Language::from(lang_fn))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub line: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileNode {
    pub symbols: Vec<Symbol>,
    pub raw_imports: Vec<String>,
    pub depends_on: Vec<String>,
    #[serde(skip)]
    pub package_name: Option<String>,
    #[serde(skip)]
    pub source: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OrphanFile {
    pub file_path: String,
    pub symbols: Vec<Symbol>,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UnusedSymbol {
    pub file_path: String,
    pub symbol: Symbol,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct DependencyGraph {
    pub files: HashMap<String, FileNode>,
    #[serde(skip)]
    reverse: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_file(&mut self, file_path: &str, source: &str, language: &str) {
        let ts_lang = match get_language(language) {
            Some(l) => l,
            None => return,
        };
        let mut parser = Parser::new();
        if parser.set_language(&ts_lang).is_err() {
            return;
        }
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return,
        };
        let root = tree.root_node();

        let symbols = extract_symbols(source, language, &root);
        let raw_imports = extract_imports(source, language, &root);
        let package_name = extract_package(source, language, &root);

        self.files.insert(
            file_path.to_string(),
            FileNode {
                symbols,
                raw_imports,
                depends_on: Vec::new(),
                package_name,
                source: source.to_string(),
            },
        );
    }

    pub fn resolve_dependencies(&mut self) {
        let all_paths: Vec<String> = self.files.keys().cloned().collect();
        let file_stems: HashMap<String, Vec<String>> = build_stem_index(&all_paths);
        let file_packages: HashMap<String, Option<String>> = build_package_index(&self.files);
        let symbol_index = build_symbol_index(&self.files);

        let resolutions: Vec<(String, Vec<String>)> = self
            .files
            .iter()
            .map(|(fp, node)| {
                let deps = node
                    .raw_imports
                    .iter()
                    .filter_map(|imp| {
                        resolve_import(
                            imp,
                            fp,
                            &file_stems,
                            &file_packages,
                            &symbol_index,
                            &all_paths,
                        )
                    })
                    .filter(|dep| dep != fp)
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect();
                (fp.clone(), deps)
            })
            .collect();

        for (fp, deps) in resolutions {
            if let Some(node) = self.files.get_mut(&fp) {
                node.depends_on = deps;
            }
        }

        self.build_reverse_index();
    }

    fn build_reverse_index(&mut self) {
        self.reverse.clear();
        for (fp, node) in &self.files {
            for dep in &node.depends_on {
                self.reverse
                    .entry(dep.clone())
                    .or_default()
                    .push(fp.clone());
            }
        }
    }

    pub fn deps(&self, file_path: &str) -> Option<&FileNode> {
        self.files.get(file_path)
    }

    pub fn dependents(&self, file_path: &str) -> Vec<&str> {
        self.reverse
            .get(file_path)
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    pub fn impact(&self, file_path: &str) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(file_path.to_string());
        visited.insert(file_path.to_string());

        while let Some(current) = queue.pop_front() {
            if let Some(deps) = self.reverse.get(&current) {
                for dep in deps {
                    if visited.insert(dep.clone()) {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }

        visited.remove(file_path);
        let mut result: Vec<String> = visited.into_iter().collect();
        result.sort();
        result
    }

    pub fn orphans(&self) -> Vec<OrphanFile> {
        let mut results = Vec::new();
        for (fp, node) in &self.files {
            if is_entry_point(fp) {
                continue;
            }
            let dep_count = self.reverse.get(fp).map(|v| v.len()).unwrap_or(0);
            if dep_count == 0 {
                results.push(OrphanFile {
                    file_path: fp.clone(),
                    symbols: node.symbols.clone(),
                    depends_on: node.depends_on.clone(),
                });
            }
        }
        results.sort_by(|a, b| a.file_path.cmp(&b.file_path));
        results
    }

    pub fn unused_symbols(&self) -> Vec<UnusedSymbol> {
        let mut defined: HashMap<String, Vec<(String, Symbol)>> = HashMap::new();
        for (fp, node) in &self.files {
            for sym in &node.symbols {
                defined
                    .entry(sym.name.clone())
                    .or_default()
                    .push((fp.clone(), sym.clone()));
            }
        }

        let mut imported_names: HashSet<String> = HashSet::new();
        for node in self.files.values() {
            for imp in &node.raw_imports {
                if let Some(last) = imp.rsplit(&[':', '.', '/', '\\']).next() {
                    imported_names.insert(last.to_string());
                    let lower = last.to_lowercase();
                    if lower != *last {
                        imported_names.insert(lower);
                    }
                }
                imported_names.insert(imp.clone());
            }
        }

        let mut results = Vec::new();
        for (name, locations) in &defined {
            if name == "main" || name == "new" || name == "default" || name == "lib" {
                continue;
            }
            let referenced =
                imported_names.contains(name) || imported_names.contains(&name.to_lowercase());
            if !referenced && locations.len() == 1 {
                let (fp, sym) = &locations[0];
                if is_entry_point(fp) {
                    continue;
                }
                if let Some(node) = self.files.get(fp) {
                    if symbol_used_in_source(&node.source, name, sym.line) {
                        continue;
                    }
                }
                let dep_count = self.reverse.get(fp).map(|v| v.len()).unwrap_or(0);
                if dep_count == 0 {
                    results.push(UnusedSymbol {
                        file_path: fp.clone(),
                        symbol: sym.clone(),
                    });
                }
            }
        }
        results.sort_by(|a, b| {
            a.file_path
                .cmp(&b.file_path)
                .then(a.symbol.line.cmp(&b.symbol.line))
        });
        results
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn edge_count(&self) -> usize {
        self.files.values().map(|n| n.depends_on.len()).sum()
    }
}

fn extract_symbols(source: &str, language: &str, root: &Node) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        if let Some(sym) = extract_symbol_from_node(source, language, &child) {
            symbols.push(sym);
        }
    }

    symbols
}

fn extract_symbol_from_node(source: &str, language: &str, node: &Node) -> Option<Symbol> {
    let kind = node.kind();
    let (sym_kind, name) = match language {
        "rust" => match kind {
            "function_item" => ("function", find_child_text(source, node, "name")?),
            "struct_item" => ("struct", find_child_text(source, node, "name")?),
            "enum_item" => ("enum", find_child_text(source, node, "name")?),
            "trait_item" => ("trait", find_child_text(source, node, "name")?),
            "impl_item" => ("impl", find_child_text(source, node, "type")?),
            "mod_item" => ("module", find_child_text(source, node, "name")?),
            "const_item" => ("const", find_child_text(source, node, "name")?),
            "static_item" => ("static", find_child_text(source, node, "name")?),
            "type_item" => ("type_alias", find_child_text(source, node, "name")?),
            "macro_definition" => ("macro", find_child_text(source, node, "name")?),
            _ => return None,
        },
        "python" => match kind {
            "function_definition" => ("function", find_child_text(source, node, "name")?),
            "class_definition" => ("class", find_child_text(source, node, "name")?),
            "decorated_definition" => {
                let inner = node.child_by_field_name("definition")?;
                return extract_symbol_from_node(source, language, &inner);
            }
            _ => return None,
        },
        "javascript" | "typescript" => match kind {
            "function_declaration" => ("function", find_child_text(source, node, "name")?),
            "class_declaration" => ("class", find_child_text(source, node, "name")?),
            "interface_declaration" => ("interface", find_child_text(source, node, "name")?),
            "type_alias_declaration" => ("type_alias", find_child_text(source, node, "name")?),
            "enum_declaration" => ("enum", find_child_text(source, node, "name")?),
            "export_statement" => {
                let mut c = node.walk();
                for child in node.children(&mut c) {
                    match child.kind() {
                        "function_declaration"
                        | "class_declaration"
                        | "interface_declaration"
                        | "type_alias_declaration"
                        | "enum_declaration"
                        | "lexical_declaration" => {
                            return extract_symbol_from_node(source, language, &child);
                        }
                        _ => {}
                    }
                }
                return None;
            }
            "lexical_declaration" | "variable_declaration" => {
                let name = find_variable_name(source, node)?;
                ("const", name)
            }
            _ => return None,
        },
        "go" => match kind {
            "function_declaration" => ("function", find_child_text(source, node, "name")?),
            "method_declaration" => ("method", find_child_text(source, node, "name")?),
            "type_declaration" => {
                let mut c = node.walk();
                for child in node.children(&mut c) {
                    if child.kind() == "type_spec" {
                        if let Some(name) = find_child_text(source, &child, "name") {
                            return Some(Symbol {
                                name,
                                kind: "type".to_string(),
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
                return None;
            }
            _ => return None,
        },
        "java" => match kind {
            "class_declaration" => ("class", find_child_text(source, node, "name")?),
            "interface_declaration" => ("interface", find_child_text(source, node, "name")?),
            "enum_declaration" => ("enum", find_child_text(source, node, "name")?),
            "method_declaration" => ("method", find_child_text(source, node, "name")?),
            "record_declaration" => ("record", find_child_text(source, node, "name")?),
            _ => return None,
        },
        "c" => match kind {
            "function_definition" => (
                "function",
                find_declarator_name(source, node)
                    .unwrap_or_else(|| node_text(source, node).chars().take(40).collect()),
            ),
            _ => return None,
        },
        "cpp" => match kind {
            "function_definition" => (
                "function",
                find_declarator_name(source, node)
                    .unwrap_or_else(|| node_text(source, node).chars().take(40).collect()),
            ),
            "class_specifier" => ("class", find_child_text(source, node, "name")?),
            "struct_specifier" => ("struct", find_child_text(source, node, "name")?),
            "namespace_definition" => ("namespace", find_child_text(source, node, "name")?),
            _ => return None,
        },
        "kotlin" => match kind {
            "class_declaration" => ("class", find_child_text(source, node, "name")?),
            "object_declaration" => ("object", find_child_text(source, node, "name")?),
            "function_declaration" => ("function", find_child_text(source, node, "name")?),
            "property_declaration" => ("property", find_kotlin_property_name(source, node)?),
            "type_alias" => (
                "type_alias",
                find_child_by_kind(source, node, "identifier")?,
            ),
            _ => return None,
        },
        _ => return None,
    };

    Some(Symbol {
        name,
        kind: sym_kind.to_string(),
        line: node.start_position().row + 1,
    })
}

fn extract_imports(source: &str, language: &str, root: &Node) -> Vec<String> {
    let mut imports = Vec::new();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        match language {
            "rust" => match child.kind() {
                "use_declaration" => {
                    if let Some(text) = extract_rust_use_path(source, &child) {
                        imports.push(text);
                    }
                }
                "mod_item" => {
                    if child.child_by_field_name("body").is_none() {
                        if let Some(name) = find_child_text(source, &child, "name") {
                            imports.push(format!("mod:{name}"));
                        }
                    }
                }
                _ => {}
            },
            "python" => match child.kind() {
                "import_statement" => {
                    if let Some(name) = find_child_by_kind(source, &child, "dotted_name") {
                        imports.push(name);
                    }
                }
                "import_from_statement" => {
                    if let Some(name) = child.child_by_field_name("module_name") {
                        imports.push(node_text(source, &name));
                    }
                }
                _ => {}
            },
            "javascript" | "typescript" => {
                if child.kind() == "import_statement" {
                    if let Some(src) = child.child_by_field_name("source") {
                        let text = node_text(source, &src);
                        let cleaned = text.trim_matches(|c| c == '\'' || c == '"');
                        imports.push(cleaned.to_string());
                    }
                }
            }
            "go" => {
                if child.kind() == "import_declaration" {
                    extract_go_imports(source, &child, &mut imports);
                }
            }
            "java" => {
                if child.kind() == "import_declaration" {
                    let text = node_text(source, &child);
                    let cleaned = text
                        .trim_start_matches("import ")
                        .trim_start_matches("static ")
                        .trim_end_matches(';')
                        .trim();
                    imports.push(cleaned.to_string());
                }
            }
            "c" | "cpp" => {
                if child.kind() == "preproc_include" {
                    if let Some(path) = child.child_by_field_name("path") {
                        let text = node_text(source, &path);
                        let cleaned = text.trim_matches(|c| c == '"' || c == '<' || c == '>');
                        imports.push(cleaned.to_string());
                    }
                }
            }
            "kotlin" => {
                if child.kind() == "import" {
                    if let Some(name) = find_child_by_kind(source, &child, "qualified_identifier")
                        .or_else(|| find_child_by_kind(source, &child, "identifier"))
                    {
                        imports.push(name);
                    }
                }
            }
            _ => {}
        }
    }

    imports
}

fn extract_package(source: &str, language: &str, root: &Node) -> Option<String> {
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        match language {
            "kotlin" => {
                if child.kind() == "package_header" {
                    return find_child_by_kind(source, &child, "qualified_identifier")
                        .or_else(|| find_child_by_kind(source, &child, "identifier"));
                }
            }
            "java" => {
                if child.kind() == "package_declaration" {
                    let text = node_text(source, &child);
                    return Some(
                        text.trim_start_matches("package ")
                            .trim_end_matches(';')
                            .trim()
                            .to_string(),
                    );
                }
            }
            _ => return None,
        }
    }

    None
}

fn extract_rust_use_path(source: &str, node: &Node) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "use_tree"
            || child.kind() == "scoped_identifier"
            || child.kind() == "identifier"
        {
            return Some(node_text(source, &child));
        }
    }
    let text = node_text(source, node);
    let trimmed = text.trim_start_matches("use ").trim_end_matches(';').trim();
    Some(trimmed.to_string())
}

fn extract_go_imports(source: &str, node: &Node, imports: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "import_spec_list" {
            let mut inner_cursor = child.walk();
            for spec in child.children(&mut inner_cursor) {
                if spec.kind() == "import_spec" {
                    if let Some(path) = spec.child_by_field_name("path") {
                        let text = node_text(source, &path);
                        let cleaned = text.trim_matches('"');
                        imports.push(cleaned.to_string());
                    }
                }
            }
        } else if child.kind() == "import_spec" {
            if let Some(path) = child.child_by_field_name("path") {
                let text = node_text(source, &path);
                let cleaned = text.trim_matches('"');
                imports.push(cleaned.to_string());
            }
        }
    }
}

fn find_variable_name(source: &str, node: &Node) -> Option<String> {
    if let Some(d) = node.child_by_field_name("declarator") {
        return find_child_text(source, &d, "name");
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if child.kind() == "variable_declarator" {
            return find_child_text(source, &child, "name");
        }
    }
    None
}

fn find_kotlin_property_name(source: &str, node: &Node) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declaration" {
            return find_child_by_kind(source, &child, "identifier");
        }
    }
    None
}

fn find_child_text(source: &str, node: &Node, field: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|n| node_text(source, &n))
}

fn find_child_by_kind(source: &str, node: &Node, kind: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return Some(node_text(source, &child));
        }
    }
    None
}

fn find_declarator_name(source: &str, node: &Node) -> Option<String> {
    let declarator = node.child_by_field_name("declarator")?;
    if let Some(name) = declarator.child_by_field_name("declarator") {
        return Some(node_text(source, &name));
    }
    Some(node_text(source, &declarator))
}

fn node_text(source: &str, node: &Node) -> String {
    source[node.byte_range()].to_string()
}

const ENTRY_POINT_NAMES: &[&str] = &[
    "main.rs",
    "lib.rs",
    "mod.rs",
    "main.ts",
    "main.tsx",
    "main.js",
    "main.jsx",
    "index.ts",
    "index.tsx",
    "index.js",
    "index.jsx",
    "App.tsx",
    "App.ts",
    "App.js",
    "App.jsx",
    "app.tsx",
    "app.ts",
    "app.js",
    "app.jsx",
    "main.go",
    "main.py",
    "main.java",
    "__init__.py",
    "main.c",
    "main.cpp",
];

fn symbol_used_in_source(source: &str, name: &str, def_line: usize) -> bool {
    for (i, line) in source.lines().enumerate() {
        let line_num = i + 1;
        if line_num == def_line {
            continue;
        }
        if line.contains(name) {
            return true;
        }
    }
    false
}

fn is_entry_point(file_path: &str) -> bool {
    let name = Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    ENTRY_POINT_NAMES.contains(&name)
}

fn build_stem_index(paths: &[String]) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for path in paths {
        let p = Path::new(path);
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
            index
                .entry(stem.to_lowercase())
                .or_default()
                .push(path.clone());
        }
    }
    index
}

fn build_package_index(files: &HashMap<String, FileNode>) -> HashMap<String, Option<String>> {
    files
        .iter()
        .map(|(path, node)| (path.clone(), node.package_name.clone()))
        .collect()
}

#[derive(Debug, Clone)]
struct SymbolCandidate {
    file_path: String,
    package_name: Option<String>,
}

fn build_symbol_index(files: &HashMap<String, FileNode>) -> HashMap<String, Vec<SymbolCandidate>> {
    let mut index: HashMap<String, Vec<SymbolCandidate>> = HashMap::new();
    for (path, node) in files {
        for symbol in &node.symbols {
            index
                .entry(symbol.name.to_lowercase())
                .or_default()
                .push(SymbolCandidate {
                    file_path: path.clone(),
                    package_name: node.package_name.clone(),
                });
        }
    }
    index
}

fn resolve_import(
    raw_import: &str,
    source_file: &str,
    stem_index: &HashMap<String, Vec<String>>,
    file_packages: &HashMap<String, Option<String>>,
    symbol_index: &HashMap<String, Vec<SymbolCandidate>>,
    all_paths: &[String],
) -> Option<String> {
    // Rust mod declaration: mod:foo → foo.rs or foo/mod.rs in same directory
    if let Some(mod_name) = raw_import.strip_prefix("mod:") {
        let source_dir = Path::new(source_file).parent().unwrap_or(Path::new(""));
        let candidate1 = source_dir
            .join(format!("{mod_name}.rs"))
            .to_string_lossy()
            .to_string();
        let candidate2 = source_dir
            .join(mod_name)
            .join("mod.rs")
            .to_string_lossy()
            .to_string();
        for path in all_paths {
            if *path == candidate1 || *path == candidate2 {
                return Some(path.clone());
            }
        }
        if let Some(candidates) = stem_index.get(&mod_name.to_lowercase()) {
            let source_dir_path = Path::new(source_file).parent().unwrap_or(Path::new(""));
            return find_closest(candidates, source_dir_path);
        }
        return None;
    }

    // Rust: crate::module::item → module
    if raw_import.starts_with("crate::") || raw_import.starts_with("super::") {
        let parts: Vec<&str> = raw_import.split("::").collect();
        // Try to find the module file from path segments
        for i in (1..parts.len()).rev() {
            let stem = parts[i].to_lowercase();
            if let Some(candidates) = stem_index.get(&stem) {
                let source_dir = Path::new(source_file).parent().unwrap_or(Path::new(""));
                if let Some(best) = find_closest(candidates, source_dir) {
                    return Some(best);
                }
            }
        }
        return None;
    }

    // Alias paths: @/lib/templates → src/lib/templates
    if raw_import.starts_with("@/") {
        let without_alias = raw_import.strip_prefix("@/").unwrap();
        let last_segment = Path::new(without_alias)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(without_alias);
        for path in all_paths {
            let without_ext = Path::new(path).with_extension("");
            let path_str = without_ext.to_string_lossy();
            if path_str.ends_with(without_alias) {
                return Some(path.clone());
            }
        }
        if let Some(candidates) = stem_index.get(&last_segment.to_lowercase()) {
            let source_dir = Path::new(source_file).parent().unwrap_or(Path::new(""));
            return find_closest_with_stem(candidates, source_dir, last_segment);
        }
        return None;
    }

    // Relative paths: ./foo, ../foo
    if raw_import.starts_with('.') {
        let source_dir = Path::new(source_file).parent().unwrap_or(Path::new(""));
        let normalized = raw_import.trim_start_matches("./").replace('.', "/");
        let candidate_base = source_dir.join(&normalized);
        let base_str = candidate_base.to_string_lossy();

        for path in all_paths {
            let without_ext = Path::new(path).with_extension("");
            if without_ext.to_string_lossy() == *base_str {
                return Some(path.clone());
            }
            // index.js/index.ts
            if path.starts_with(&*base_str)
                && Path::new(path)
                    .file_stem()
                    .map(|s| s == "index")
                    .unwrap_or(false)
            {
                return Some(path.clone());
            }
        }
        return None;
    }

    // Python dotted imports: os.path → os/path.py
    if raw_import.contains('.') && !raw_import.contains('/') {
        let as_path = raw_import.replace('.', "/");
        let last_part = raw_import.rsplit('.').next().unwrap_or(raw_import);
        if let Some(candidates) = stem_index.get(&last_part.to_lowercase()) {
            for c in candidates {
                if c.replace('/', ".").contains(raw_import) || c.contains(&as_path) {
                    return Some(c.clone());
                }
            }
            if let Some(candidate) = resolve_package_stem_import(
                raw_import,
                last_part,
                candidates,
                file_packages,
                source_file,
            ) {
                return Some(candidate);
            }
        }
        if let Some(candidates) = symbol_index.get(&last_part.to_lowercase()) {
            return resolve_package_symbol_import(raw_import, last_part, candidates, source_file);
        }
        return None;
    }

    // Simple name match
    let stem = Path::new(raw_import)
        .file_stem()
        .unwrap_or(raw_import.as_ref())
        .to_string_lossy()
        .to_lowercase();
    if let Some(candidates) = stem_index.get(&stem) {
        return find_same_directory(candidates, source_file);
    }

    None
}

fn resolve_package_stem_import(
    raw_import: &str,
    imported_stem: &str,
    candidates: &[String],
    file_packages: &HashMap<String, Option<String>>,
    source_file: &str,
) -> Option<String> {
    let package_prefix = raw_import.strip_suffix(imported_stem)?.strip_suffix('.')?;
    let matching_paths: Vec<String> = candidates
        .iter()
        .filter(|path| {
            file_packages
                .get(*path)
                .and_then(|package| package.as_deref())
                == Some(package_prefix)
        })
        .cloned()
        .collect();
    if matching_paths.is_empty() {
        return None;
    }

    let source_dir = Path::new(source_file).parent().unwrap_or(Path::new(""));
    find_closest(&matching_paths, source_dir)
}

fn resolve_package_symbol_import(
    raw_import: &str,
    imported_symbol: &str,
    candidates: &[SymbolCandidate],
    source_file: &str,
) -> Option<String> {
    let package_prefix = raw_import
        .strip_suffix(imported_symbol)?
        .strip_suffix('.')?;
    let matching_paths: Vec<String> = candidates
        .iter()
        .filter(|candidate| candidate.package_name.as_deref() == Some(package_prefix))
        .map(|candidate| candidate.file_path.clone())
        .collect();
    if matching_paths.is_empty() {
        return None;
    }

    let source_dir = Path::new(source_file).parent().unwrap_or(Path::new(""));
    find_closest(&matching_paths, source_dir)
}

fn find_same_directory(candidates: &[String], source_file: &str) -> Option<String> {
    let source_dir = Path::new(source_file).parent().unwrap_or(Path::new(""));
    let matching_paths: Vec<String> = candidates
        .iter()
        .filter(|path| Path::new(path.as_str()).parent().unwrap_or(Path::new("")) == source_dir)
        .cloned()
        .collect();
    find_closest(&matching_paths, source_dir)
}

fn find_closest(candidates: &[String], source_dir: &Path) -> Option<String> {
    if candidates.len() == 1 {
        return Some(candidates[0].clone());
    }
    let source_str = source_dir.to_string_lossy();
    candidates
        .iter()
        .max_by(|a, b| {
            let a_prefix = common_prefix_len(&source_str, a);
            let b_prefix = common_prefix_len(&source_str, b);
            a_prefix.cmp(&b_prefix)
        })
        .cloned()
}

fn find_closest_with_stem(
    candidates: &[String],
    source_dir: &Path,
    exact_stem: &str,
) -> Option<String> {
    let exact_matches: Vec<&String> = candidates
        .iter()
        .filter(|c| {
            Path::new(c.as_str())
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s == exact_stem)
                .unwrap_or(false)
        })
        .collect();

    if exact_matches.len() == 1 {
        return Some(exact_matches[0].clone());
    }
    if !exact_matches.is_empty() {
        let source_str = source_dir.to_string_lossy();
        return exact_matches
            .iter()
            .max_by_key(|c| common_prefix_len(&source_str, c))
            .map(|c| (*c).clone());
    }
    find_closest(candidates, source_dir)
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_symbols_and_imports() {
        let source = r#"
use crate::types::Chunk;
use std::collections::HashMap;

pub fn search(query: &str) -> Vec<Chunk> {
    Vec::new()
}

struct Index {
    chunks: Vec<Chunk>,
}
"#;
        let mut graph = DependencyGraph::new();
        graph.add_file("src/search.rs", source, "rust");
        let node = graph.files.get("src/search.rs").unwrap();
        assert_eq!(node.symbols.len(), 2);
        assert_eq!(node.symbols[0].name, "search");
        assert_eq!(node.symbols[0].kind, "function");
        assert_eq!(node.symbols[1].name, "Index");
        assert_eq!(node.symbols[1].kind, "struct");
        assert!(node.raw_imports.len() >= 2);
    }

    #[test]
    fn test_python_symbols_and_imports() {
        let source = r#"
import os
from pathlib import Path

class FileWalker:
    def walk(self):
        pass

def main():
    pass
"#;
        let mut graph = DependencyGraph::new();
        graph.add_file("walker.py", source, "python");
        let node = graph.files.get("walker.py").unwrap();
        assert_eq!(node.symbols.len(), 2);
        assert_eq!(node.symbols[0].name, "FileWalker");
        assert_eq!(node.symbols[1].name, "main");
        assert!(node.raw_imports.len() >= 2);
    }

    #[test]
    fn test_impact_analysis() {
        let mut graph = DependencyGraph::new();
        graph.files.insert(
            "a.rs".to_string(),
            FileNode {
                symbols: vec![],
                raw_imports: vec![],
                depends_on: vec!["b.rs".to_string()],
                package_name: None,
                source: String::new(),
            },
        );
        graph.files.insert(
            "b.rs".to_string(),
            FileNode {
                symbols: vec![],
                raw_imports: vec![],
                depends_on: vec!["c.rs".to_string()],
                package_name: None,
                source: String::new(),
            },
        );
        graph.files.insert(
            "c.rs".to_string(),
            FileNode {
                symbols: vec![],
                raw_imports: vec![],
                depends_on: vec![],
                package_name: None,
                source: String::new(),
            },
        );
        graph.build_reverse_index();

        let impact = graph.impact("c.rs");
        assert!(impact.contains(&"b.rs".to_string()));
        assert!(impact.contains(&"a.rs".to_string()));
    }

    #[test]
    fn test_javascript_imports() {
        let source = r#"
import { useState } from 'react';
import utils from './utils';

function App() {
    return null;
}
"#;
        let mut graph = DependencyGraph::new();
        graph.add_file("src/App.js", source, "javascript");
        let node = graph.files.get("src/App.js").unwrap();
        assert!(node.raw_imports.contains(&"react".to_string()));
        assert!(node.raw_imports.contains(&"./utils".to_string()));
    }

    #[test]
    fn test_typescript_export_symbols() {
        let source = r#"
import { db } from './firebase';

export async function getUser(uid: string) {
    return null;
}

export const createPage = async (data: any) => {
    return null;
};

export type PageData = {
    slug: string;
};

export interface UserProfile {
    name: string;
}

const internal = () => {};

export default function MainComponent() {
    return null;
}
"#;
        let mut graph = DependencyGraph::new();
        graph.add_file("src/lib/firestore.ts", source, "typescript");
        let node = graph.files.get("src/lib/firestore.ts").unwrap();
        let names: Vec<&str> = node.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"getUser"),
            "missing getUser, got: {names:?}"
        );
        assert!(
            names.contains(&"createPage"),
            "missing createPage, got: {names:?}"
        );
        assert!(
            names.contains(&"PageData"),
            "missing PageData, got: {names:?}"
        );
        assert!(
            names.contains(&"UserProfile"),
            "missing UserProfile, got: {names:?}"
        );
        assert!(
            names.contains(&"MainComponent"),
            "missing MainComponent, got: {names:?}"
        );
        assert!(
            names.contains(&"internal"),
            "missing internal, got: {names:?}"
        );
    }

    #[test]
    fn test_kotlin_symbols_and_imports() {
        let source = r#"
package com.example

import com.example.data.UserRepository
import kotlin.collections.List

class UserService {
    fun load() = Unit
}

object UserRoutes

fun createUser(): String = "ok"

val defaultUserName: String = "guest"

typealias UserName = String
"#;
        let mut graph = DependencyGraph::new();
        graph.add_file(
            "src/main/kotlin/com/example/UserService.kt",
            source,
            "kotlin",
        );
        let node = graph
            .files
            .get("src/main/kotlin/com/example/UserService.kt")
            .unwrap();
        let names: Vec<&str> = node.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"UserService"),
            "missing UserService, got: {names:?}"
        );
        assert!(
            names.contains(&"UserRoutes"),
            "missing UserRoutes, got: {names:?}"
        );
        assert!(
            names.contains(&"createUser"),
            "missing createUser, got: {names:?}"
        );
        assert!(
            names.contains(&"defaultUserName"),
            "missing defaultUserName, got: {names:?}"
        );
        assert!(
            names.contains(&"UserName"),
            "missing UserName, got: {names:?}"
        );
        assert!(node
            .raw_imports
            .contains(&"com.example.data.UserRepository".to_string()));
        assert!(node
            .raw_imports
            .contains(&"kotlin.collections.List".to_string()));
    }

    #[test]
    fn test_kotlin_dependency_resolution() {
        let mut graph = DependencyGraph::new();
        graph.add_file(
            "src/main/kotlin/com/example/UserService.kt",
            "package com.example\n\nimport com.example.data.UserRepository\n\nclass UserService\n",
            "kotlin",
        );
        graph.add_file(
            "src/main/kotlin/com/example/data/UserRepository.kt",
            "package com.example.data\n\nclass UserRepository\n",
            "kotlin",
        );
        graph.resolve_dependencies();

        let node = graph
            .files
            .get("src/main/kotlin/com/example/UserService.kt")
            .unwrap();
        assert_eq!(
            node.depends_on,
            vec!["src/main/kotlin/com/example/data/UserRepository.kt".to_string()]
        );

        let impact = graph.impact("src/main/kotlin/com/example/data/UserRepository.kt");
        assert_eq!(
            impact,
            vec!["src/main/kotlin/com/example/UserService.kt".to_string()]
        );
    }

    #[test]
    fn test_kotlin_top_level_symbol_import_resolution() {
        let mut graph = DependencyGraph::new();
        graph.add_file(
            "src/main/kotlin/com/example/UserService.kt",
            "package com.example\n\nimport com.example.util.formatUserName\n\nclass UserService\n",
            "kotlin",
        );
        graph.add_file(
            "src/main/kotlin/com/example/util/DateUtils.kt",
            "package com.example.util\n\nfun formatUserName(): String = \"ok\"\n",
            "kotlin",
        );
        graph.resolve_dependencies();

        let node = graph
            .files
            .get("src/main/kotlin/com/example/UserService.kt")
            .unwrap();
        assert_eq!(
            node.depends_on,
            vec!["src/main/kotlin/com/example/util/DateUtils.kt".to_string()]
        );
    }

    #[test]
    fn test_kotlin_external_import_does_not_match_local_symbol() {
        let mut graph = DependencyGraph::new();
        graph.add_file(
            "parser/src/main/kotlin/io/clroot/excel/parser/ExcelParser.kt",
            "package io.clroot.excel.parser\n\nimport org.apache.poi.ss.usermodel.Sheet\n\nclass ExcelParser\n",
            "kotlin",
        );
        graph.add_file(
            "core/src/main/kotlin/io/clroot/excel/core/model/ExcelDocument.kt",
            "package io.clroot.excel.core.model\n\nclass Sheet\n",
            "kotlin",
        );
        graph.resolve_dependencies();

        let node = graph
            .files
            .get("parser/src/main/kotlin/io/clroot/excel/parser/ExcelParser.kt")
            .unwrap();
        assert!(
            node.depends_on.is_empty(),
            "external library import should not resolve to local Sheet: {:?}",
            node.depends_on
        );
    }

    #[test]
    fn test_kotlin_external_import_with_matching_local_stem() {
        let mut graph = DependencyGraph::new();
        graph.add_file(
            "src/main/kotlin/com/example/App.kt",
            "package com.example\n\nimport org.junit.jupiter.api.Test\n\nclass App\n",
            "kotlin",
        );
        graph.add_file(
            "src/main/kotlin/com/foo/Test.kt",
            "package com.foo\n\nclass Test\n",
            "kotlin",
        );
        graph.resolve_dependencies();

        let node = graph
            .files
            .get("src/main/kotlin/com/example/App.kt")
            .unwrap();
        assert!(
            node.depends_on.is_empty(),
            "external JUnit import should not resolve to local Test.kt: {:?}",
            node.depends_on
        );
    }

    #[test]
    fn test_java_external_import_with_matching_local_stem() {
        let mut graph = DependencyGraph::new();
        graph.add_file(
            "src/main/java/com/example/App.java",
            "package com.example;\n\nimport org.junit.jupiter.api.Test;\n\nclass App {}\n",
            "java",
        );
        graph.add_file(
            "src/main/java/com/foo/Test.java",
            "package com.foo;\n\nclass Test {}\n",
            "java",
        );
        graph.resolve_dependencies();

        let node = graph
            .files
            .get("src/main/java/com/example/App.java")
            .unwrap();
        assert!(
            node.depends_on.is_empty(),
            "external JUnit import should not resolve to local Test.java: {:?}",
            node.depends_on
        );
    }

    #[test]
    fn test_python_external_import_with_matching_local_module() {
        let mut graph = DependencyGraph::new();
        graph.add_file(
            "app/main.py",
            "import requests\n\ndef main():\n    pass\n",
            "python",
        );
        graph.add_file("pkg/requests.py", "def helper():\n    pass\n", "python");
        graph.resolve_dependencies();

        let node = graph.files.get("app/main.py").unwrap();
        assert!(
            node.depends_on.is_empty(),
            "external requests import should not resolve to unrelated local module: {:?}",
            node.depends_on
        );
    }

    #[test]
    fn test_python_same_directory_import_resolution() {
        let mut graph = DependencyGraph::new();
        graph.add_file(
            "app/main.py",
            "import helpers\n\ndef main():\n    pass\n",
            "python",
        );
        graph.add_file("app/helpers.py", "def helper():\n    pass\n", "python");
        graph.resolve_dependencies();

        let node = graph.files.get("app/main.py").unwrap();
        assert_eq!(node.depends_on, vec!["app/helpers.py".to_string()]);
    }
}

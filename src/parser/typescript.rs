use indexmap::IndexMap;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use strum_macros;
use tree_sitter;
use tree_sitter::StreamingIterator;
use tree_sitter_typescript;

use super::common;
use super::common::PendingImport;
use crate::util;
use crate::Database;
use crate::{Edge, EdgeType, Language, Node, NodeType};
use crate::{File, FuncParamType};

/// The tree-sitter definition query source for TypeScript.
pub const TYPESCRIPT_DEFINITIONS_QUERY_SOURCE: &str =
    include_str!("queries/typescript-definitions.scm");

/// Tree-sitter query patterns.
///
/// Note that the order of these variants must match the order of the patterns in the query source file.
#[derive(Debug, Clone, PartialEq, Eq, strum_macros::FromRepr)]
enum QueryPattern {
    Import,
    Interface,
    Class,
    Function,
    Method,
    Enum,
    TypeAlias,
}

pub struct Parser {
    repo_path: PathBuf,
}

impl Parser {
    pub fn new(repo_path: PathBuf) -> Self {
        Self {
            repo_path: repo_path.clone(),
        }
    }

    pub fn parse(
        &self,
        file_node: &Node,
        file: &File,
    ) -> Result<
        (
            IndexMap<String, Node>,
            Vec<Edge>,
            Vec<PendingImport>,
            Option<HashMap<String, Vec<FuncParamType>>>,
        ),
        Box<dyn std::error::Error>,
    > {
        let query_source = TYPESCRIPT_DEFINITIONS_QUERY_SOURCE.to_string();
        let mut nodes: IndexMap<String, Node> = IndexMap::new();
        let mut edges: Vec<Edge> = Vec::new();
        let mut pending_imports: Vec<PendingImport> = Vec::new();
        let mut func_param_types: HashMap<String, Vec<FuncParamType>> = HashMap::new();

        let mut import_name_to_source_path: HashMap<String, String> = HashMap::new(); // Maps import names to their corresponding source paths

        let source_code = file.content;

        let mut parser = tree_sitter::Parser::new();
        let language = &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        parser
            .set_language(language)
            .expect("Error loading language parser");

        let tree = parser.parse(source_code, None).unwrap();
        let root_node = tree.root_node();

        let mut cursor = tree_sitter::QueryCursor::new();
        let query = tree_sitter::Query::new(language, &query_source).unwrap();
        let mut matches = cursor.matches(&query, root_node, source_code);

        while let Some(mat) = matches.next() {
            if let Some(pattern) = QueryPattern::from_repr(mat.pattern_index) {
                match pattern {
                    QueryPattern::Import => {
                        let mut import = PendingImport {
                            language: Language::TypeScript,
                            source_path: "".to_string(),
                            symbol: None,
                            alias: None,
                        };

                        for capture in mat.captures {
                            let capture_name = query.capture_names()[capture.index as usize];
                            let capture_node_text: String = capture
                                .node
                                .utf8_text(&source_code)
                                .unwrap_or("")
                                .to_string();
                            common::log_capture(&capture, capture_name, &capture_node_text);

                            match capture_name {
                                "reference.namespace_import.alias" => {
                                    // import * as X from 'Y' => X
                                    import.alias = Some(capture_node_text);
                                }
                                "reference.named_import.name" => {
                                    // import { X } from 'Y' => X
                                    // import { X as x } from 'Y' => X
                                    import.symbol = Some(capture_node_text);
                                }
                                "reference.named_import.alias" => {
                                    // import { X as x } from 'Y' => x
                                    import.alias = Some(capture_node_text);
                                }
                                "reference.default_import.alias" => {
                                    // import X from 'Y' => X
                                    import.symbol = Some("export default".to_string()); // a special symbol to represent the default export
                                    import.alias = Some(capture_node_text);
                                }
                                "reference.import.source" => {
                                    // import X from 'Y' => Y
                                    // import { X } from 'Y' => Y
                                    // import * as X from 'Y' => Y

                                    // Only handle relative imports for now.
                                    if capture_node_text.starts_with("./")
                                        || capture_node_text.starts_with("../")
                                    {
                                        // Get the absolute path of the imported file.
                                        let current_file_dir = file.path.parent().unwrap();
                                        let import_path = Path::new(&capture_node_text);
                                        let mut import_file_path =
                                            current_file_dir.join(import_path);

                                        // If the import path is a directory, append 'index.d.ts', 'index.ts' or 'index.js' to it
                                        if import_file_path.is_dir() {
                                            let index_d_ts = import_file_path.join("index.d.ts");
                                            let index_ts = import_file_path.join("index.ts");
                                            let index_js = import_file_path.join("index.js");
                                            if index_d_ts.exists() {
                                                import_file_path = index_d_ts;
                                            } else if index_ts.exists() {
                                                import_file_path = index_ts;
                                            } else if index_js.exists() {
                                                import_file_path = index_js;
                                            }
                                        } else {
                                            let file_ts = import_file_path.with_extension("ts");
                                            let file_js = import_file_path.with_extension("js");
                                            if file_ts.exists() {
                                                import_file_path = file_ts;
                                            } else if file_js.exists() {
                                                import_file_path = file_js;
                                            }
                                        }

                                        // Remove ./ or ../ from the import path
                                        let canonical_file_path = import_file_path
                                            .canonicalize()
                                            .unwrap_or(import_file_path.clone());
                                        import_file_path = canonical_file_path
                                            .strip_prefix(&self.repo_path)
                                            .unwrap_or_else(|_| &canonical_file_path)
                                            .to_path_buf();

                                        import.source_path =
                                            import_file_path.to_string_lossy().to_string();
                                    }
                                }
                                _ => {}
                            }
                        }

                        if !import.source_path.is_empty() {
                            pending_imports.push(import.clone());

                            import_name_to_source_path
                                .insert(import.import_name(), import.source_path.clone());
                        }
                    }

                    QueryPattern::Interface => {
                        let current_node = common::parse_simple_interface(
                            &query,
                            &mat,
                            &self.repo_path,
                            file_node,
                            &file.path,
                            &source_code,
                        );
                        if let Some(curr_node) = current_node {
                            nodes.insert(curr_node.name.clone(), curr_node.clone());
                            edges.push(Edge {
                                r#type: EdgeType::Contains,
                                from: file_node.clone(),
                                to: curr_node.clone(),
                                import: None,
                                alias: None,
                            });
                        }
                    }

                    QueryPattern::Class => {
                        let mut current_node: Option<Node> = None;
                        let mut current_tree_sitter_main_node: Option<tree_sitter::Node> = None;

                        for capture in mat.captures {
                            let capture_name = query.capture_names()[capture.index as usize];
                            let capture_node_text: String = capture
                                .node
                                .utf8_text(&source_code)
                                .unwrap_or("")
                                .to_string();
                            common::log_capture(&capture, capture_name, &capture_node_text);

                            match capture_name {
                                "definition.class" => {
                                    current_node = Some(Node {
                                        name: "".to_string(), // fill in later
                                        r#type: NodeType::Class,
                                        language: file_node.language.clone(),
                                        start_line: capture.node.start_position().row,
                                        end_line: capture.node.end_position().row,
                                        code: capture_node_text,
                                        skeleton_code: String::new(),
                                    });
                                    current_tree_sitter_main_node = Some(capture.node);
                                }
                                "definition.class.name" => {
                                    if let Some(curr_node) = &mut current_node {
                                        curr_node.name = format!(
                                            "{}:{}",
                                            file_node.name.clone(),
                                            capture_node_text
                                        );
                                    }
                                }
                                "definition.class.body" => {
                                    if let Some(current_tree_sitter_main_node) =
                                        current_tree_sitter_main_node
                                    {
                                        let start_byte = current_tree_sitter_main_node.start_byte();
                                        let body_start_byte = capture.node.start_byte();
                                        if let Some(curr_node) = &mut current_node {
                                            // Skip the body and keep only the signature.
                                            curr_node.skeleton_code = String::from_utf8_lossy(
                                                &source_code[start_byte..body_start_byte],
                                            )
                                            .to_string()
                                                + "{ ... }";
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }

                        if let Some(curr_node) = current_node {
                            nodes.insert(curr_node.name.clone(), curr_node.clone());
                            edges.push(Edge {
                                r#type: EdgeType::Contains,
                                from: file_node.clone(),
                                to: curr_node.clone(),
                                import: None,
                                alias: None,
                            });
                        }
                    }

                    QueryPattern::Function => {
                        let mut current_node: Option<Node> = None;
                        let mut current_tree_sitter_main_node: Option<tree_sitter::Node> = None;
                        let mut param_type_names: Vec<String> = Vec::new();

                        for capture in mat.captures {
                            let capture_name = query.capture_names()[capture.index as usize];
                            let capture_node_text: String = capture
                                .node
                                .utf8_text(&source_code)
                                .unwrap_or("")
                                .to_string();
                            common::log_capture(&capture, capture_name, &capture_node_text);

                            match capture_name {
                                "definition.function" => {
                                    current_node = Some(Node {
                                        name: "".to_string(), // fill in later
                                        r#type: NodeType::Function,
                                        language: file_node.language.clone(),
                                        start_line: capture.node.start_position().row,
                                        end_line: capture.node.end_position().row,
                                        code: capture_node_text,
                                        skeleton_code: String::new(),
                                    });
                                    current_tree_sitter_main_node = Some(capture.node);
                                }
                                "definition.function.name" => {
                                    if let Some(curr_node) = &mut current_node {
                                        curr_node.name = format!(
                                            "{}:{}",
                                            file_node.name.clone(),
                                            capture_node_text
                                        );
                                    }
                                }
                                "definition.function.param_type" => {
                                    param_type_names.push(capture_node_text);
                                }
                                "definition.function.body" => {
                                    if let Some(current_tree_sitter_main_node) =
                                        current_tree_sitter_main_node
                                    {
                                        let start_byte = current_tree_sitter_main_node.start_byte();
                                        let body_start_byte = capture.node.start_byte();
                                        if let Some(curr_node) = &mut current_node {
                                            // Skip the body and keep only the signature.
                                            curr_node.skeleton_code = String::from_utf8_lossy(
                                                &source_code[start_byte..body_start_byte],
                                            )
                                            .to_string()
                                                + "{ ... }";
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }

                        if let Some(curr_node) = &mut current_node {
                            // Parse the parameter types of the current function.
                            for param_type_name in param_type_names {
                                let param_types = Self::parse_func_param_types(
                                    &curr_node.name,
                                    &param_type_name,
                                    &import_name_to_source_path,
                                );
                                for param_type in param_types {
                                    func_param_types
                                        .entry(curr_node.name.clone())
                                        .or_insert_with(Vec::new)
                                        .push(param_type);
                                }
                            }

                            // There might be multiple parameter types for a function, in which case tree-sitter will
                            // emit multiple matches for the same function.
                            //
                            // We only need to keep one node and one edge for the same function.
                            if !nodes.contains_key(&curr_node.name) {
                                nodes.insert(curr_node.name.clone(), curr_node.clone());
                                edges.push(Edge {
                                    r#type: EdgeType::Contains,
                                    from: file_node.clone(),
                                    to: curr_node.clone(),
                                    import: None,
                                    alias: None,
                                });
                            }
                        }
                    }

                    QueryPattern::Method => {
                        let mut current_node: Option<Node> = None;
                        let mut method_name: Option<String> = None;
                        let mut parent_class_name: Option<String> = None;
                        let mut current_tree_sitter_main_node: Option<tree_sitter::Node> = None;
                        let mut param_type_names: Vec<String> = Vec::new();

                        for capture in mat.captures {
                            let capture_name = query.capture_names()[capture.index as usize];
                            let capture_node_text: String = capture
                                .node
                                .utf8_text(&source_code)
                                .unwrap_or("")
                                .to_string();
                            common::log_capture(&capture, capture_name, &capture_node_text);

                            match capture_name {
                                "definition.class.name" => {
                                    // Due to the behavior of tree-sitter, the class name will be captured multiple times
                                    // if there are multiple methods in the class.
                                    parent_class_name = Some(capture_node_text);
                                }
                                "definition.method" => {
                                    current_node = Some(Node {
                                        name: "".to_string(), // fill in later
                                        r#type: NodeType::Function,
                                        language: file_node.language.clone(),
                                        start_line: capture.node.start_position().row,
                                        end_line: capture.node.end_position().row,
                                        code: capture_node_text,
                                        skeleton_code: String::new(),
                                    });
                                    current_tree_sitter_main_node = Some(capture.node);
                                }
                                "definition.method.name" => {
                                    method_name = Some(capture_node_text);
                                }
                                "definition.method.param_type" => {
                                    param_type_names.push(capture_node_text);
                                }
                                "definition.method.body" => {
                                    if let Some(current_tree_sitter_main_node) =
                                        current_tree_sitter_main_node
                                    {
                                        let start_byte = current_tree_sitter_main_node.start_byte();
                                        let body_start_byte = capture.node.start_byte();
                                        if let Some(curr_node) = &mut current_node {
                                            // Skip the body and keep only the signature.
                                            curr_node.skeleton_code = String::from_utf8_lossy(
                                                &source_code[start_byte..body_start_byte],
                                            )
                                            .to_string()
                                                + "{ ... }";
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }

                        if let (Some(curr_node), Some(parent_class_name), Some(method_name)) =
                            (&mut current_node, parent_class_name, method_name)
                        {
                            let parent_class_node_name = format!(
                                "{}:{}",
                                file_node.name.clone(),
                                parent_class_name.clone(),
                            );
                            curr_node.name = format!(
                                "{}.{}",
                                parent_class_node_name.clone(),
                                method_name.clone(),
                            );

                            // Parse the parameter types of the current method.
                            for param_type_name in param_type_names {
                                let param_types = Self::parse_func_param_types(
                                    &curr_node.name,
                                    &param_type_name,
                                    &import_name_to_source_path,
                                );
                                for param_type in param_types {
                                    func_param_types
                                        .entry(curr_node.name.clone())
                                        .or_insert_with(Vec::new)
                                        .push(param_type);
                                }
                            }

                            // There might be multiple parameter types for a method, in which case tree-sitter will
                            // emit multiple matches for the same method.
                            //
                            // We only need to keep one node and one edge for the same method.
                            if !nodes.contains_key(&curr_node.name) {
                                nodes.insert(curr_node.name.clone(), curr_node.clone());

                                // Find the parent class node.
                                // Here we assume that the parent class has been parsed and added into nodes.
                                let parent_class_node = nodes.get(&parent_class_node_name);
                                if let Some(parent_class_node) = parent_class_node {
                                    edges.push(Edge {
                                        r#type: EdgeType::Contains,
                                        from: parent_class_node.clone(),
                                        to: curr_node.clone(),
                                        import: None,
                                        alias: None,
                                    });
                                }
                            }
                        }
                    }

                    QueryPattern::Enum => {
                        let current_node = common::parse_simple_enum(
                            &query,
                            &mat,
                            &self.repo_path,
                            file_node,
                            &file.path,
                            &source_code,
                        );
                        if let Some(curr_node) = current_node {
                            nodes.insert(curr_node.name.clone(), curr_node.clone());
                            edges.push(Edge {
                                r#type: EdgeType::Contains,
                                from: file_node.clone(),
                                to: curr_node.clone(),
                                import: None,
                                alias: None,
                            });
                        }
                    }

                    QueryPattern::TypeAlias => {
                        let current_node = common::parse_simple_type_alias(
                            &query,
                            &mat,
                            &self.repo_path,
                            file_node,
                            &file.path,
                            &source_code,
                        );
                        if let Some(curr_node) = current_node {
                            nodes.insert(curr_node.name.clone(), curr_node.clone());
                            edges.push(Edge {
                                r#type: EdgeType::Contains,
                                from: file_node.clone(),
                                to: curr_node.clone(),
                                import: None,
                                alias: None,
                            });
                        }
                    }
                }
            }
        }

        Ok((nodes, edges, pending_imports, Some(func_param_types)))
    }

    pub fn resolve_pending_imports(
        &self,
        nodes: &IndexMap<String, Node>,
        pending_imports: &HashMap<String, Vec<PendingImport>>,
    ) -> Result<Vec<Edge>, Box<dyn std::error::Error>> {
        let mut edges: Vec<Edge> = Vec::new();

        for (file_node_name, pending_imports) in pending_imports {
            for imp in pending_imports {
                log::trace!(
                    "{file_node_name} => {}, {:?}, {:?}",
                    imp.source_path,
                    imp.symbol,
                    imp.alias
                );

                let mut imported_node_name = imp.source_path.clone();
                if let Some(imp_symbol) = &imp.symbol {
                    imported_node_name = format!("{}:{}", imp.source_path, imp_symbol);
                }
                let file_node = nodes.get(file_node_name);
                let imported_node = nodes.get(&imported_node_name);
                if let (Some(file_node), Some(imported_node)) = (file_node, imported_node) {
                    edges.push(Edge {
                        r#type: EdgeType::Imports,
                        from: file_node.clone(),
                        to: imported_node.clone(),
                        import: imp.symbol.clone(),
                        alias: imp.alias.clone(),
                    })
                }
            }
        }

        Ok(edges)
    }

    // Mainly used when indexing all the repo (for performance reasons).
    pub fn resolve_func_param_type_edges(
        &self,
        nodes: &IndexMap<String, Node>,
        func_param_types: &HashMap<String, Vec<FuncParamType>>,
        db: &mut Database,
    ) -> Result<Vec<Edge>, Box<dyn std::error::Error>> {
        let mut edges: Vec<Edge> = Vec::new();

        for (func_node_name, param_types) in func_param_types {
            let func_node = nodes.get(func_node_name);
            if let Some(func_node) = func_node {
                for param_type in param_types {
                    if let Some(file_node_name) = &param_type.package_name {
                        let type_node_name = format!("{}:{}", file_node_name, param_type.type_name);
                        let param_type_node = nodes.get(type_node_name.as_str());
                        log::trace!(
                            "type_node_name: {type_node_name}, param_type_node: {:?}",
                            param_type_node
                        );
                        if let Some(param_type_node) = param_type_node {
                            edges.push(Edge {
                                r#type: EdgeType::References,
                                from: func_node.clone(),
                                to: param_type_node.clone(),
                                import: None,
                                alias: None,
                            });
                        }
                    }
                }
            }
        }

        Ok(edges)
    }

    // Mainly used when indexing a single file, where type nodes are not available in the current file.
    pub fn resolve_func_param_type_edges_from_db(
        &self,
        nodes: &IndexMap<String, Node>,
        func_param_types: &HashMap<String, Vec<FuncParamType>>,
        db: &mut Database,
    ) -> Result<Vec<Edge>, Box<dyn std::error::Error>> {
        let mut edges: Vec<Edge> = Vec::new();

        let mut file_types: IndexMap<String, HashSet<String>> = IndexMap::new();
        for (func_node_name, param_types) in func_param_types {
            for param_type in param_types {
                if let Some(file_node_name) = &param_type.package_name {
                    file_types
                        .entry(file_node_name.clone())
                        .or_insert_with(HashSet::new)
                        .insert(param_type.type_name.clone());
                };
            }
        }

        let mut filetype_to_node = IndexMap::new(); // "{file_node_name}:{type_name}" => type_node
        for (file_node_name, type_names) in file_types {
            let quoted_type_names: Vec<String> = type_names
                .iter()
                .map(|s| format!("\"{}\"", s.to_lowercase()))
                .collect();
            let type_names_str = format!("[{}]", quoted_type_names.join(", "));
            let stmt = format!(
                r#"
MATCH (file {{ name: "{}" }})
MATCH (file)-[:CONTAINS]->(typ)
WHERE typ.short_name IN {}
RETURN typ;
                "#,
                file_node_name, type_names_str,
            );
            log::trace!("Query Stmt: {:}", stmt);
            let type_nodes = db.query_nodes(stmt.as_str())?;

            for node in &type_nodes {
                filetype_to_node.insert(
                    format!("{}:{}", file_node_name, node.short_name()),
                    node.clone(),
                );
            }
        }

        for (func_node_name, param_types) in func_param_types {
            let func_node = nodes.get(func_node_name);

            for param_type in param_types {
                if let Some(file_node_name) = &param_type.package_name {
                    let mut param_type_node = filetype_to_node.get(&format!(
                        "{}:{}",
                        file_node_name,
                        param_type.type_name.to_lowercase()
                    ));
                    if let (Some(func_node), Some(param_type_node)) = (func_node, param_type_node) {
                        edges.push(Edge {
                            r#type: EdgeType::References,
                            from: func_node.clone(),
                            to: param_type_node.clone(),
                            import: None,
                            alias: None,
                        });
                    }
                }
            }
        }

        Ok(edges)
    }

    fn parse_func_param_types(
        from_node_name: &String,
        param_type_name: &String,
        import_name_to_source_path: &HashMap<String, String>,
    ) -> Vec<FuncParamType> {
        let mut param_types: Vec<FuncParamType> = Vec::new();

        for (import_name, source_path) in import_name_to_source_path {
            log::trace!(
                "imported_name: {import_name}, source_path: {:?}",
                source_path
            );
        }

        let param_type_names = extract_ts_types(param_type_name.as_str(), true);
        for param_type_name in param_type_names {
            let type_parts: Vec<&str> = param_type_name.splitn(2, '.').collect();
            let (module_name, type_name) = match type_parts.len() {
                // no pacakge
                1 => (None, type_parts[0].to_string()),
                // package and type
                2 => (Some(type_parts[0].to_string()), type_parts[1].to_string()),
                _ => unreachable!(),
            };

            let mut source_node_name: Option<String> = None;
            if let Some(module_name) = &module_name {
                // Find the target module name that the type belongs to.
                // Set it to be None if not found.
                if let Some(source_path) = import_name_to_source_path.get(module_name) {
                    source_node_name = Some(source_path.clone());
                }
            } else {
                // Otherwise, the type might has been directly imported.
                if let Some(source_path) = import_name_to_source_path.get(&type_name) {
                    source_node_name = Some(source_path.clone());
                } else {
                    // Finally, the type might be defined in the same file.
                    if let Some(from_file_node_name) = from_node_name.splitn(2, ":").next() {
                        source_node_name = Some(from_file_node_name.into());
                    }
                }
            }

            log::trace!(
                "module_name: {:?}, type_name: {type_name}, source_node_name: {:?}",
                module_name,
                source_node_name
            );

            // Save the types referenced by the currrent function/method.
            param_types.push(FuncParamType {
                type_name,
                package_name: source_node_name,
            });
        }

        param_types
    }
}

/// Extract types from TypeScript type string
///
/// # Arguments
/// * `type_str` - TypeScript type expression string
/// * `exclude_builtin` - Whether to exclude builtin types like string, number, etc.
///
/// # Returns
/// * Array of extracted type strings
pub fn extract_ts_types(type_str: &str, exclude_builtin: bool) -> Vec<String> {
    // Builtin types list
    let builtin_types: HashSet<&str> = [
        // Primitive types
        "string",
        "number",
        "boolean",
        "any",
        "void",
        "null",
        "undefined",
        "unknown",
        "never",
        "object",
        "bigint",
        "symbol",
        "function",
        // Composite types
        "Map",
        "Promise",
        "Array",
        "Record",
        "Partial",
    ]
    .iter()
    .cloned()
    .collect();

    // Compile regex pattern
    let re = Regex::new(r"(^|[<,\s])([A-Za-z_][A-Za-z0-9_]*)(?:\[\])*(>|,|\s|$|&|\|)?")
        .expect("Invalid regex pattern");

    let mut result = Vec::new();
    let mut found_types = HashSet::new();

    for cap in re.captures_iter(type_str) {
        if let Some(matched) = cap.get(2) {
            let type_name = matched.as_str();

            // Handle type name filtering logic
            if (!exclude_builtin || !builtin_types.contains(type_name))
                && !found_types.contains(type_name)
            {
                result.push(type_name.to_string());
                found_types.insert(type_name);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_ts_types() {
        let test_cases = vec![
            "X",
            "X[]",
            "X[][]",
            "Map<string, X>",
            "Promise<X>",
            "Array<X>",
            "Record<string, X>",
            "Promise<Map<string, X>>",
            "Partial<X>",
            "X | Y",                        // 联合类型
            "X & Y",                        // 交叉类型
            "Person extends Human ? X : Y", // 条件类型
        ];

        for case in test_cases {
            println!("类型字符串: {}", case);

            // 提取所有类型
            let all_types = extract_ts_types(case, false);
            println!("所有类型: {:?}", all_types);

            // 排除内置类型
            let custom_types = extract_ts_types(case, true);
            println!("自定义类型: {:?}", custom_types);

            println!();
        }
    }
}

use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tree_sitter;
use tree_sitter::StreamingIterator;
use tree_sitter_go;

use super::common;
use super::common::QueryPattern;
use crate::util;
use crate::Database;
use crate::FuncParamType;
use crate::{Edge, EdgeType, Language, Node, NodeType};

/// The tree-sitter definition query source for Go.
pub const GO_DEFINITIONS_QUERY_SOURCE: &str = include_str!("queries/go-definitions.scm");

pub struct Parser {
    repo_path: PathBuf,
    go_module_path: Option<String>,
}

impl Parser {
    pub fn new(repo_path: PathBuf) -> Self {
        Self {
            repo_path: repo_path.clone(),
            go_module_path: util::get_go_repo_module_path(&repo_path),
        }
    }

    pub fn parse(
        &self,
        file_node: &Node,
        file_path: &PathBuf,
    ) -> Result<
        (
            IndexMap<String, Node>,
            Vec<Edge>,
            Option<HashMap<String, Vec<FuncParamType>>>,
        ),
        Box<dyn std::error::Error>,
    > {
        let query_source = GO_DEFINITIONS_QUERY_SOURCE.to_string();
        let mut nodes: IndexMap<String, Node> = IndexMap::new();
        let mut edges: Vec<Edge> = Vec::new();
        let mut func_param_types: HashMap<String, Vec<FuncParamType>> = HashMap::new();

        let source_code = fs::read(&file_path).expect("Should have been able to read the file");

        let mut parser = tree_sitter::Parser::new();
        let language = &tree_sitter_go::LANGUAGE.into();
        parser
            .set_language(language)
            .expect("Error loading language parser");

        let tree = parser.parse(source_code.clone(), None).unwrap();
        let root_node = tree.root_node();

        let mut cursor = tree_sitter::QueryCursor::new();
        let query = tree_sitter::Query::new(language, &query_source).unwrap();
        let mut matches = cursor.matches(&query, root_node, source_code.as_slice());

        while let Some(mat) = matches.next() {
            if let Some(pattern) = QueryPattern::from_repr(mat.pattern_index) {
                match pattern {
                    QueryPattern::Import => {
                        for capture in mat.captures {
                            let start = capture.node.start_position();
                            let end = capture.node.end_position();
                            let capture_name = query.capture_names()[capture.index as usize];
                            let capture_node_text: String = capture
                                .node
                                .utf8_text(&source_code)
                                .unwrap_or("")
                                .to_string();
                            log::trace!(
                                "[CAPTURE]\nname: {capture_name}, start: {start}, end: {end}, text: {:?}, capture: {:?}",
                                capture_node_text,
                                capture.node.to_sexp()
                            );

                            match capture_name {
                                "reference.import.path" => {
                                    let path_name: String = capture
                                        .node
                                        .utf8_text(&source_code)
                                        .unwrap_or("")
                                        .to_string();

                                    let parts: Vec<&str> = path_name.splitn(2, ' ').collect();
                                    let (alias, mod_import_path) = match parts.len() {
                                        // no alias
                                        1 => (None, parts[0].trim_matches('"').to_string()),
                                        // alias and path
                                        2 => (
                                            Some(parts[0].to_string()),
                                            parts[1].trim_matches('"').to_string(),
                                        ),
                                        _ => unreachable!(),
                                    };

                                    if let Some(go_module_path) = self.go_module_path.clone() {
                                        let mod_file_path = util::get_repo_module_file_path(
                                            &PathBuf::from(""),
                                            &go_module_path,
                                            &mod_import_path,
                                        );

                                        if let Some(mod_file_path) = mod_file_path {
                                            let parts: Vec<&str> =
                                                mod_import_path.rsplitn(2, '/').collect();
                                            let mod_name = parts.first().unwrap_or(&""); // get module name

                                            let edge = Edge {
                                                r#type: EdgeType::Imports,
                                                from: Node::from_type_and_name(
                                                    file_node.r#type.clone(),
                                                    file_node.name.clone(),
                                                ),
                                                to: Node::from_type_and_name(
                                                    NodeType::Directory,
                                                    mod_file_path.to_string_lossy().to_string(),
                                                ),
                                                import: Some(mod_name.to_string()),
                                                alias: alias,
                                            };
                                            edges.push(edge);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    QueryPattern::Interface => {
                        let current_node = common::parse_simple_interface(
                            &query,
                            &mat,
                            &self.repo_path,
                            file_node,
                            file_path,
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
                        let current_node = common::parse_simple_class(
                            &query,
                            &mat,
                            &self.repo_path,
                            file_node,
                            file_path,
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

                    QueryPattern::Function => {
                        let mut current_node: Option<Node> = None;
                        let mut current_tree_sitter_main_node: Option<tree_sitter::Node> = None;
                        let mut parent_struct_name: Option<String> = None;
                        let mut param_type_names: Vec<String> = Vec::new();

                        for capture in mat.captures {
                            let start = capture.node.start_position();
                            let end = capture.node.end_position();
                            let capture_name = query.capture_names()[capture.index as usize];
                            let capture_node_text: String = capture
                                .node
                                .utf8_text(&source_code)
                                .unwrap_or("")
                                .to_string();
                            log::trace!(
                                "[CAPTURE]\nname: {capture_name}, start: {start}, end: {end}, text: {:?}, capture: {:?}",
                                capture_node_text,
                                capture.node.to_sexp()
                            );

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
                                            Path::new(file_path)
                                                .strip_prefix(&self.repo_path)
                                                .unwrap_or_else(|_| Path::new(file_path))
                                                .to_string_lossy(),
                                            capture_node_text
                                        );
                                    }
                                }
                                "definition.function.first_return_type" => {
                                    // The current function is a struct constructor
                                    let struct_node_name = format!(
                                        "{}:{}",
                                        Path::new(file_path)
                                            .strip_prefix(&self.repo_path)
                                            .unwrap_or_else(|_| Path::new(file_path))
                                            .to_string_lossy(),
                                        capture_node_text,
                                    );
                                    // Assume that the struct node is defined early in the current file.
                                    if nodes.contains_key(&struct_node_name) {
                                        parent_struct_name = Some(capture_node_text);
                                    }
                                }
                                "definition.function.param_type" => {
                                    let param_type_name: String = capture
                                        .node
                                        .utf8_text(&source_code)
                                        .unwrap_or("")
                                        .to_string();
                                    param_type_names.push(param_type_name);
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
                                                + "{\n...\n}";
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }

                        if let Some(curr_node) = &mut current_node {
                            // Change the name of the current node to include the parent struct name, if any.
                            if let Some(parent_struct_name) = &parent_struct_name {
                                let node_name = curr_node.name.rsplit(':').next().unwrap_or("");
                                curr_node.name = format!(
                                    "{}:{}.{}",
                                    Path::new(file_path)
                                        .strip_prefix(&self.repo_path)
                                        .unwrap_or_else(|_| Path::new(file_path))
                                        .to_string_lossy(),
                                    parent_struct_name.clone(),
                                    node_name
                                );
                            }

                            // Parse the parameter types of the current function.
                            for param_type_name in param_type_names {
                                let param_type = Self::parse_func_param_type(
                                    &curr_node.name,
                                    &param_type_name,
                                    &edges,
                                );
                                if let Some(param_type) = param_type {
                                    func_param_types
                                        .entry(curr_node.name.clone())
                                        .or_insert_with(Vec::new)
                                        .push(param_type);
                                }
                            }

                            // There might be multiple parameter types for a method, in which case tree-sitter will
                            // emit multiple matches for the same function.
                            //
                            // We only need to keep one node and one edge for the same method.
                            if !nodes.contains_key(&curr_node.name) {
                                nodes.insert(curr_node.name.clone(), curr_node.clone());

                                let edge = if let Some(parent_struct_name) = &parent_struct_name {
                                    let parent_node_name = curr_node
                                        .name
                                        .rsplit_once('.')
                                        .map(|(prefix, _)| prefix)
                                        .unwrap();
                                    // Assume that the parent struct node is defined early in the current file.
                                    let parent_node = nodes.get(parent_node_name).unwrap();
                                    Edge {
                                        r#type: EdgeType::Contains,
                                        from: parent_node.clone(),
                                        to: curr_node.clone(),
                                        import: None,
                                        alias: None,
                                    }
                                } else {
                                    Edge {
                                        r#type: EdgeType::Contains,
                                        from: file_node.clone(),
                                        to: curr_node.clone(),
                                        import: None,
                                        alias: None,
                                    }
                                };
                                edges.push(edge);
                            }
                        }
                    }

                    QueryPattern::Method => {
                        let mut current_node: Option<Node> = None;
                        let mut current_tree_sitter_main_node: Option<tree_sitter::Node> = None;
                        let mut parent_struct_name: Option<String> = None;
                        let mut param_type_names: Vec<String> = Vec::new();

                        for capture in mat.captures {
                            let start = capture.node.start_position();
                            let end = capture.node.end_position();
                            let capture_name = query.capture_names()[capture.index as usize];
                            let capture_node_text: String = capture
                                .node
                                .utf8_text(&source_code)
                                .unwrap_or("")
                                .to_string();
                            log::trace!(
                                "[CAPTURE]\nname: {capture_name}, start: {start}, end: {end}, text: {:?}, capture: {:?}",
                                capture_node_text,
                                capture.node.to_sexp()
                            );

                            match capture_name {
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
                                    if let Some(curr_node) = &mut current_node {
                                        curr_node.name = format!(
                                            "{}:{}",
                                            Path::new(file_path)
                                                .strip_prefix(&self.repo_path)
                                                .unwrap_or_else(|_| Path::new(file_path))
                                                .to_string_lossy(),
                                            capture_node_text
                                        );
                                    }
                                }
                                "definition.method.receiver_type" => {
                                    // Try to find the parent struct of the current method.
                                    let struct_node_name = format!(
                                        "{}:{}",
                                        Path::new(file_path)
                                            .strip_prefix(&self.repo_path)
                                            .unwrap_or_else(|_| Path::new(file_path))
                                            .to_string_lossy(),
                                        capture_node_text,
                                    );
                                    // Assume that the struct node is defined early in the current file.
                                    if nodes.contains_key(&struct_node_name) {
                                        parent_struct_name = Some(capture_node_text);
                                    }
                                }
                                "definition.method.param_type" => {
                                    let param_type_name: String = capture
                                        .node
                                        .utf8_text(&source_code)
                                        .unwrap_or("")
                                        .to_string();
                                    param_type_names.push(param_type_name);
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
                                                + "{\n...\n}";
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }

                        if let Some(curr_node) = &mut current_node {
                            // Change the name of the current node to include the parent struct name, if any.
                            if let Some(parent_struct_name) = &parent_struct_name {
                                let node_name = curr_node.name.rsplit(':').next().unwrap_or("");
                                curr_node.name = format!(
                                    "{}:{}.{}",
                                    Path::new(file_path)
                                        .strip_prefix(&self.repo_path)
                                        .unwrap_or_else(|_| Path::new(file_path))
                                        .to_string_lossy(),
                                    parent_struct_name.clone(),
                                    node_name
                                );
                            }

                            // Parse the parameter types of the current function.
                            for param_type_name in param_type_names {
                                let param_type = Self::parse_func_param_type(
                                    &curr_node.name,
                                    &param_type_name,
                                    &edges,
                                );
                                if let Some(param_type) = param_type {
                                    func_param_types
                                        .entry(curr_node.name.clone())
                                        .or_insert_with(Vec::new)
                                        .push(param_type);
                                }
                            }

                            // There might be multiple parameter types for a method, in which case tree-sitter will
                            // emit multiple matches for the same function.
                            //
                            // We only need to keep one node and one edge for the same method.
                            if !nodes.contains_key(&curr_node.name) {
                                nodes.insert(curr_node.name.clone(), curr_node.clone());

                                let edge = if let Some(parent_struct_name) = &parent_struct_name {
                                    let parent_node_name = curr_node
                                        .name
                                        .rsplit_once('.')
                                        .map(|(prefix, _)| prefix)
                                        .unwrap();
                                    // Assume that the parent struct node is defined early in the current file.
                                    let parent_node = nodes.get(parent_node_name).unwrap();
                                    Edge {
                                        r#type: EdgeType::Contains,
                                        from: parent_node.clone(),
                                        to: curr_node.clone(),
                                        import: None,
                                        alias: None,
                                    }
                                } else {
                                    Edge {
                                        r#type: EdgeType::Contains,
                                        from: file_node.clone(),
                                        to: curr_node.clone(),
                                        import: None,
                                        alias: None,
                                    }
                                };
                                edges.push(edge);
                            }
                        }
                    }
                }
            }
        }

        Ok((nodes, edges, Some(func_param_types)))
    }

    pub fn resolve_func_param_type_edges(
        &self,
        nodes: &IndexMap<String, Node>,
        func_param_types: &HashMap<String, Vec<FuncParamType>>,
        db: &mut Database,
    ) -> Result<Vec<Edge>, Box<dyn std::error::Error>> {
        let mut edges: Vec<Edge> = Vec::new();

        let mut pkg_types: IndexMap<String, HashSet<String>> = IndexMap::new();
        for (func_name, param_types) in func_param_types {
            for param_type in param_types {
                if let Some(package_name) = &param_type.package_name {
                    pkg_types
                        .entry(package_name.clone())
                        .or_insert_with(HashSet::new)
                        .insert(param_type.type_name.clone());
                };
            }
        }

        let mut pkgtype_to_node = IndexMap::new(); // "{pkg_name}:{type_name}" => type_node
        for (pkg_name, type_names) in pkg_types {
            let quoted_type_names: Vec<String> = type_names
                .iter()
                .map(|s| format!("\"{}\"", s.to_lowercase()))
                .collect();
            let type_names_str = format!("[{}]", quoted_type_names.join(", "));
            let stmt = format!(
                r#"
MATCH (pkg {{ name: "{}" }})
MATCH (pkg)-[:CONTAINS*2]->(typ)
WHERE typ.short_name IN {}
RETURN typ;
                "#,
                pkg_name, type_names_str,
            );
            log::trace!("Query Stmt: {:}", stmt);
            let nodes = db.query_nodes(stmt.as_str())?;

            for node in &nodes {
                pkgtype_to_node.insert(format!("{}:{}", pkg_name, node.short_name()), node.clone());
            }
        }

        for (func_name, param_types) in func_param_types {
            let func_node = nodes.get(func_name);

            for param_type in param_types {
                if let Some(package_name) = &param_type.package_name {
                    let mut type_node = pkgtype_to_node.get(&format!(
                        "{}:{}",
                        package_name,
                        param_type.type_name.to_lowercase()
                    ));
                    if let (Some(func_node), Some(type_node)) = (func_node, type_node) {
                        let rel = Edge {
                            r#type: EdgeType::References,
                            from: func_node.clone(),
                            to: type_node.clone(),
                            import: None,
                            alias: None,
                        };
                        edges.push(rel);
                    }
                }
            }
        }

        Ok(edges)
    }

    fn parse_func_param_type(
        from_node_name: &String,
        param_type_name: &String,
        import_edges: &Vec<Edge>,
    ) -> Option<FuncParamType> {
        // Skip the inline type definitions
        // `f func (...) ...`
        // `s struct { ... }`
        // `iface interface { ... }`
        if param_type_name.starts_with("func")
            || param_type_name.starts_with("struct")
            || param_type_name.starts_with("interface")
        {
            return None;
        }

        // Do conversion:
        // foo.Foo = > foo.Foo
        // Foo => Foo
        // *Foo => Foo
        // []*Foo => Foo
        // map[string]Foo => Foo
        let parts: Vec<&str> = param_type_name
            .rsplitn(2, |c| c == '*' || c == ']')
            .collect();
        let param_type = parts.first().unwrap_or(&"").trim();

        let type_parts: Vec<&str> = param_type.splitn(2, '.').collect();
        let (package_name, type_name) = match type_parts.len() {
            // no pacakge
            1 => (None, type_parts[0].to_string()),
            // package and type
            2 => (Some(type_parts[0].to_string()), type_parts[1].to_string()),
            _ => unreachable!(),
        };

        let mut real_package_name: Option<String> = None;
        // Find the target package name that the type belongs to.
        if let Some(package_name) = &package_name {
            for rel in import_edges {
                if let Some(import) = &rel.import {
                    if import == package_name {
                        real_package_name = Some(rel.to.name.clone());
                        break;
                    }
                }
                if let Some(alias) = &rel.alias {
                    if alias == package_name {
                        real_package_name = Some(rel.to.name.clone());
                        break;
                    }
                }
            }

            // If the package name is not found, leave it as None.
        } else {
            // Otherwise, the type is defined in the same package as the current file.
            let mut parent_dir_path = from_node_name.rsplitn(2, '/').nth(1).unwrap_or("");
            if parent_dir_path.is_empty() {
                parent_dir_path = ".";
            }
            real_package_name = Some(parent_dir_path.to_string());
        }

        if util::is_go_builtin_type(&type_name) {
            return None;
        }

        // Save the types referenced by the currrent function/method.
        return Some(FuncParamType {
            type_name,
            package_name: real_package_name,
        });
    }
}

/*
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse() {
        // Create test file
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let dir_path = PathBuf::from(manifest_dir)
            .join("examples")
            .join("go")
            .join("demo");

        let mut parser = Parser::new(dir_path.clone());
        let result = parser.parse(dir_path);
        match result {
            Ok((nodes, edges)) => {
                let mut node_strings: Vec<_> = nodes.into_iter().map(|n| n.name).collect();
                let mut rel_strings: Vec<_> = edges
                    .into_iter()
                    .map(|r| format!("{}-[{}]->{}", r.from.name, r.r#type, r.to.name))
                    .collect();

                node_strings.sort();
                rel_strings.sort();

                assert_eq!(
                    node_strings,
                    [
                        "",
                        "main.go",
                        "main.go:User",
                        "main.go:User.DisplayInfo",
                        "main.go:User.NewUser",
                        "main.go:User.SetAddress",
                        "main.go:User.UpdateEmail",
                        "main.go:main",
                        "types.go",
                        "types.go:Address",
                        "types.go:Hobby"
                    ]
                );
                assert_eq!(
                    rel_strings,
                    [
                        "main.go-[contains]->main.go:User",
                        "main.go-[contains]->main.go:main",
                        "main.go:User-[contains]->main.go:User.DisplayInfo",
                        "main.go:User-[contains]->main.go:User.NewUser",
                        "main.go:User-[contains]->main.go:User.SetAddress",
                        "main.go:User-[contains]->main.go:User.UpdateEmail",
                        "types.go-[contains]->types.go:Address",
                        "types.go-[contains]->types.go:Hobby"
                    ],
                );
            }
            Err(e) => {
                println!("Failed to parse: {:?}", e);
            }
        }
    }
}
*/

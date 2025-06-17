use glob::Pattern;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use tree_sitter;
use tree_sitter::StreamingIterator;
use tree_sitter_go;
use tree_sitter_python;
use walkdir::WalkDir;

use crate::util;
use crate::Database;
use crate::{Edge, EdgeType, Language, Node, NodeType, Relationship};

/// The tree-sitter definition query source for different languages.
pub const PYTHON_DEFINITIONS_QUERY_SOURCE: &str = include_str!("python/definitions.scm");
pub const GO_DEFINITIONS_QUERY_SOURCE: &str = include_str!("go/definitions.scm");
pub const GO_FUNC_PARAMS_QUERY_SOURCE: &str = include_str!("go/function_parameters.scm");

#[derive(Clone, Debug)]
/// Configuration options for the parser.
pub struct ParserConfig {
    /// Whether to recursively traverse subdirectories (default is true)
    pub recursive: bool,
    /// Whether to follow symbolic links (default is false)
    pub follow_links: bool,
    /// Maximum recursion depth, None means no limit (default is None)
    pub max_depth: usize,
    /// Whether to continue traversal when encountering errors (default is false)
    pub continue_on_error: bool,
    /// Ignore patterns following gitignore syntax (default is empty)
    /// Each pattern follows gitignore rules:
    /// - Pattern starting with '!' negates the pattern
    /// - Pattern ending with '/' matches directories only
    /// - Pattern starting with '/' is anchored to root
    /// - '*' matches any sequence of characters except '/'
    /// - '**' matches any sequence of characters including '/'
    /// - '?' matches any single character
    /// - '[abc]' matches any character in brackets
    pub ignore_patterns: Vec<String>,
    /// Whether to use .gitignore files found in directories (default is true)
    pub use_gitignore_files: bool,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            recursive: true,
            follow_links: false,
            max_depth: 0,
            continue_on_error: false,
            ignore_patterns: Vec::new(),
            use_gitignore_files: true,
        }
    }
}

impl ParserConfig {
    pub fn recursive(mut self, recursive: bool) -> Self {
        self.recursive = recursive;
        self
    }
    pub fn follow_links(mut self, follow_links: bool) -> Self {
        self.follow_links = follow_links;
        self
    }
    pub fn max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }
    pub fn continue_on_error(mut self, continue_on_error: bool) -> Self {
        self.continue_on_error = continue_on_error;
        self
    }
    pub fn ignore_patterns(mut self, ignore_patterns: Vec<String>) -> Self {
        self.ignore_patterns = ignore_patterns;
        self
    }
    pub fn use_gitignore_files(mut self, use_gitignore_files: bool) -> Self {
        self.use_gitignore_files = use_gitignore_files;
        self
    }
}

pub struct FuncParamType {
    type_name: String,
    package_name: Option<String>,
}

pub struct Parser {
    repo_path: PathBuf,
    config: ParserConfig,
    pub nodes: IndexMap<String, Node>,
    relationships: Vec<Relationship>,
    pub func_param_types: HashMap<String, Vec<FuncParamType>>, // function name -> parameter types
    // language-specific properties
    go_module_path: Option<String>,
}

impl Parser {
    pub fn new(repo_path: PathBuf, config: ParserConfig) -> Self {
        Parser {
            repo_path: repo_path,
            config: config,
            nodes: IndexMap::new(),
            relationships: Vec::new(),
            func_param_types: HashMap::new(),
            go_module_path: None,
        }
    }

    /// Parses the directory and returns references to parsed nodes and relationships
    ///
    /// # Arguments
    /// * `dir_path` - Path to the directory to parse
    ///
    /// # Returns
    /// Tuple of references to parsed nodes and relationships vectors
    /// Will write JSON files to configured output directory if specified
    pub fn parse(
        &mut self,
        dir_path: PathBuf,
    ) -> Result<(Vec<Node>, Vec<Relationship>), Box<dyn std::error::Error>> {
        self.go_module_path = util::get_go_repo_module_path(&self.repo_path);

        self.traverse_directory(&dir_path)?;
        let nodes: Vec<Node> = self.nodes.values().cloned().collect();

        // Return references to parsed nodes and relationships
        Ok((nodes, self.relationships.clone()))
    }

    pub fn resolve_func_param_type_relationships(
        &self,
        nodes: &IndexMap<String, Node>,
        func_param_types: &HashMap<String, Vec<FuncParamType>>,
        db: &mut Database,
    ) -> Result<Vec<Relationship>, Box<dyn std::error::Error>> {
        let mut relationships: Vec<Relationship> = Vec::new();

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
                        let rel = Relationship {
                            r#type: EdgeType::References,
                            from: func_node.clone(),
                            to: type_node.clone(),
                            import: None,
                            alias: None,
                        };
                        relationships.push(rel);
                    }
                }
            }
        }

        Ok(relationships)
    }

    /// Traverses all files and directories in the specified directory, creates Node and Relationship objects
    /// This method processes files by calling self.parse_file directly when encountering supported file types
    ///
    /// # Arguments
    /// - `dir_path`: The directory path to traverse
    ///
    /// # Returns
    /// - A tuple containing (nodes, relationships) where:
    ///   - nodes: Vector of Node objects representing directories, files, and parsed code elements
    ///   - relationships: Vector of Relationship objects representing Contains relationships
    pub fn traverse_directory(
        &mut self,
        dir_path: &PathBuf,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Check if directory exists
        if !dir_path.exists() {
            return Err(format!("Directory does not exist: {}", dir_path.display()).into());
        }

        let mut processed_paths: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::new();

        // Create WalkDir instance and apply configuration options
        let mut walkdir = WalkDir::new(dir_path);

        // Configure whether to follow symbolic links
        walkdir = walkdir.follow_links(self.config.follow_links);

        // Configure maximum recursion depth
        if self.config.max_depth > 0 {
            walkdir = walkdir.max_depth(self.config.max_depth);
        }

        // If not recursive, set depth to 1 (only traverse current directory)
        if !self.config.recursive {
            walkdir = walkdir.max_depth(1);
        }

        // Compile ignore patterns
        let mut ignore_patterns: Vec<Pattern> = self
            .config
            .ignore_patterns
            .iter()
            .filter_map(|p| Pattern::new(p).ok())
            .collect();

        // Add patterns from .gitignore files if enabled
        if self.config.use_gitignore_files {
            if let Ok(gitignore_path) = dir_path.join(".gitignore").canonicalize() {
                if let Ok(content) = std::fs::read_to_string(&gitignore_path) {
                    for line in content.lines() {
                        let line = line.trim();
                        // Skip comments and empty lines
                        if line.is_empty() || line.starts_with('#') {
                            continue;
                        }
                        if let Ok(pattern) = Pattern::new(line) {
                            ignore_patterns.push(pattern);
                        }
                    }
                }
            }
        }

        // Create root directory node
        let root_node = Node {
            // kuzu CSV does not support empty string as node name, so use "." for root directory
            //name: dir_path
            //    .strip_prefix(dir_path)
            //    .unwrap_or(dir_path)
            //    .to_string_lossy()
            //    .to_string(),
            name: String::from(""),
            r#type: NodeType::Directory,
            language: Language::Text,
            start_line: 0,
            end_line: 0,
            code: String::new(),
            skeleton_code: String::from(""),
        };
        self.add_node(&root_node)?;
        processed_paths.insert(dir_path.clone());

        // Traverse directory
        for entry in walkdir {
            match entry {
                Ok(entry) => {
                    let entry_path = entry.path();

                    // Skip if already processed
                    if processed_paths.contains(entry_path) {
                        continue;
                    }

                    // Get relative path from the root directory for ignore pattern checking
                    let rel_path = entry_path
                        .strip_prefix(dir_path)
                        .unwrap_or(entry_path)
                        .to_string_lossy();

                    // Check if path matches any ignore pattern
                    let mut should_skip = false;
                    let mut has_negation = false;

                    // First check positive patterns
                    for pattern in &ignore_patterns {
                        if !pattern.as_str().starts_with('!') && pattern.matches(&rel_path) {
                            should_skip = true;
                            break;
                        }
                    }

                    // Then check negation patterns
                    if should_skip {
                        for pattern in &ignore_patterns {
                            if pattern.as_str().starts_with('!') {
                                let negated_pattern = &pattern.as_str()[1..];
                                if let Ok(p) = Pattern::new(negated_pattern) {
                                    if p.matches(&rel_path) {
                                        has_negation = true;
                                        break;
                                    }
                                }
                            }
                        }
                        should_skip = !has_negation;
                    }

                    if should_skip {
                        continue;
                    }

                    // Create node for current entry
                    let current_node = if entry.file_type().is_dir() {
                        Node {
                            name: entry_path
                                .strip_prefix(dir_path)
                                .unwrap_or(entry_path)
                                .to_string_lossy()
                                .to_string(),
                            r#type: NodeType::Directory,
                            language: Language::Text,
                            start_line: 0,
                            end_line: 0,
                            code: String::new(),
                            skeleton_code: String::from(""),
                        }
                    } else {
                        let (file_node, nodes, rels, func_param_types) =
                            self.parse_file(&entry_path)?;
                        for (n_name, n) in nodes {
                            self.nodes.insert(n_name, n);
                        }
                        for r in rels {
                            self.relationships.push(r);
                        }
                        if let Some(func_param_types) = func_param_types {
                            self.func_param_types.extend(func_param_types);
                        }

                        // Sleep for a short duration to avoid high CPU usage during traversal.
                        thread::sleep(Duration::from_millis(1));

                        file_node
                    };

                    self.add_node(&current_node)?;
                    processed_paths.insert(entry_path.to_path_buf());

                    // Find parent directory and create Contains relationship
                    if let Some(parent_path) = entry_path.parent() {
                        // Find parent node in the nodes vector
                        let parent_path_str = parent_path
                            .strip_prefix(dir_path)
                            .unwrap_or(entry_path)
                            .to_string_lossy()
                            .to_string();

                        // Ensure parent directory node exists
                        if !processed_paths.contains(parent_path)
                            && parent_path != Path::new(dir_path)
                        {
                            let parent_node = Node {
                                name: parent_path_str.clone(),
                                r#type: NodeType::Directory,
                                language: Language::Text,
                                start_line: 0,
                                end_line: 0,
                                code: String::new(),
                                skeleton_code: String::from(""),
                            };
                            self.add_node(&parent_node)?;
                            processed_paths.insert(parent_path.to_path_buf());
                        }

                        // Find the actual parent node from nodes vector
                        if let Some(parent_node) = self.nodes.get(&parent_path_str) {
                            let relationship = Relationship {
                                r#type: EdgeType::Contains,
                                from: parent_node.clone(),
                                to: current_node.clone(),
                                import: None,
                                alias: None,
                            };
                            self.relationships.push(relationship);
                        }
                    }
                }
                Err(err) => {
                    // Decide whether to continue on error based on configuration
                    if self.config.continue_on_error {
                        eprintln!("Error encountered during traversal, continuing: {}", err);
                        continue;
                    } else {
                        return Err(err.into());
                    }
                }
            }
        }

        Ok(())
    }

    fn add_node(&mut self, node: &Node) -> Result<(), Box<dyn std::error::Error>> {
        self.nodes.insert(node.name.clone(), node.clone());

        Ok(())
    }

    pub fn parse_file(
        &self,
        file_path: &Path,
    ) -> Result<
        (
            Node,
            IndexMap<String, Node>,
            Vec<Relationship>,
            Option<HashMap<String, Vec<FuncParamType>>>,
        ),
        Box<dyn std::error::Error>,
    > {
        let file_language = Language::from_path(file_path.to_path_buf().to_str().unwrap());
        let file_node = Node {
            name: file_path
                .strip_prefix(&self.repo_path)
                .unwrap_or(file_path)
                .to_string_lossy()
                .to_string(),
            r#type: NodeType::File,
            language: file_language,
            start_line: 0,
            end_line: 0,                     // TODO: add end line number
            code: String::new(),             // TODO: add file code
            skeleton_code: String::from(""), // TODO: add file skeleton code
        };
        // Parse the file and add parsed nodes to the collection
        match file_node.language {
            Language::Python => {
                let (nodes, rels) = self.parse_python_file(
                    &file_node,
                    &self.repo_path,
                    &file_path.to_path_buf(),
                    "",
                )?;
                return Ok((file_node, nodes, rels, None));
            }
            Language::Go => {
                let (nodes, rels, func_param_types) =
                    self.parse_go_file(&file_node, &self.repo_path, &file_path.to_path_buf(), "")?;
                return Ok((file_node, nodes, rels, func_param_types));
            }
            Language::Text => {
                return Ok((file_node, IndexMap::new(), vec![], None));
            }
        }
    }

    fn parse_python_file(
        &self,
        file_node: &Node,
        dir_path: &Path,
        file_path: &PathBuf,
        query_path: &str,
    ) -> Result<(IndexMap<String, Node>, Vec<Relationship>), Box<dyn std::error::Error>> {
        let query_source = if query_path.is_empty() {
            PYTHON_DEFINITIONS_QUERY_SOURCE.to_string()
        } else {
            let query_path = PathBuf::from(query_path);
            fs::read_to_string(query_path).expect("Should have been able to read the query file")
        };

        if query_source == "" {
            return Ok((IndexMap::new(), vec![]));
        }

        let mut nodes: IndexMap<String, Node> = IndexMap::new();
        let mut relationships: Vec<Relationship> = Vec::new();

        let source_code = fs::read(&file_path).expect("Should have been able to read the file");

        //println!("[SOURCE]\n\n{}\n", String::from_utf8_lossy(&source_code));
        //println!("[QUERY]\n\n{}\n", query_source);

        let mut parser = tree_sitter::Parser::new();
        let language = &tree_sitter_python::LANGUAGE.into();
        parser
            .set_language(language)
            .expect("Error loading language parser");

        let tree = parser.parse(source_code.clone(), None).unwrap();
        let root_node = tree.root_node();

        let mut cursor = tree_sitter::QueryCursor::new();
        let query = tree_sitter::Query::new(language, &query_source).unwrap();
        let mut captures = cursor.captures(&query, root_node, source_code.as_slice());

        let mut cur_class_node: Option<tree_sitter::Node> = None;
        // 使用 streaming iterator 的正确方式来迭代QueryCaptures
        while let Some((mat, capture_index)) = captures.next() {
            let capture = mat.captures[*capture_index];
            let capture_name = query.capture_names()[capture.index as usize];
            let pos_start = capture.node.start_position();
            let pos_end = capture.node.end_position();
            log::trace!(
                "[CAPTURE]\nname: {capture_name}, start: {}, end: {}, text: {:?}, capture: {:?}",
                pos_start,
                pos_end,
                capture.node.utf8_text(&source_code).unwrap_or(""),
                capture.node.to_sexp()
            );

            match capture_name {
                "definition.class.name" => {
                    let class_name: String = capture
                        .node
                        .utf8_text(&source_code)
                        .unwrap_or("")
                        .to_string();
                    if let Some(class_node) = cur_class_node {
                        let node = Node {
                            name: format!(
                                "{}:{}",
                                Path::new(file_path)
                                    .strip_prefix(dir_path)
                                    .unwrap_or_else(|_| Path::new(file_path))
                                    .to_string_lossy(),
                                class_name
                            ),
                            r#type: NodeType::Class,
                            language: file_node.language.clone(),
                            start_line: class_node.start_position().row + 1,
                            end_line: class_node.end_position().row + 1,
                            code: class_node.utf8_text(&source_code).unwrap_or("").to_string(),
                            skeleton_code: "".to_string(),
                        };
                        nodes.insert(node.name.clone(), node.clone());

                        let relationship = Relationship {
                            r#type: EdgeType::Contains,
                            from: file_node.clone(),
                            to: node.clone(),
                            import: None,
                            alias: None,
                        };
                        relationships.push(relationship);
                    }
                }
                "definition.class" => {
                    cur_class_node = Some(capture.node);
                }
                _ => {}
            }
        }
        Ok((nodes, relationships))
    }

    fn parse_go_file(
        &self,
        file_node: &Node,
        dir_path: &Path,
        file_path: &PathBuf,
        query_path: &str,
    ) -> Result<
        (
            IndexMap<String, Node>,
            Vec<Relationship>,
            Option<HashMap<String, Vec<FuncParamType>>>,
        ),
        Box<dyn std::error::Error>,
    > {
        let query_source = if query_path.is_empty() {
            GO_DEFINITIONS_QUERY_SOURCE.to_string()
        } else {
            let query_path = PathBuf::from(query_path);
            fs::read_to_string(query_path).expect("Should have been able to read the query file")
        };

        if query_source == "" {
            return Ok((IndexMap::new(), vec![], None));
        }

        let mut nodes: IndexMap<String, Node> = IndexMap::new();
        let mut relationships: Vec<Relationship> = Vec::new();
        let mut func_param_types: HashMap<String, Vec<FuncParamType>> = HashMap::new();

        let source_code = fs::read(&file_path).expect("Should have been able to read the file");

        //println!("[SOURCE]\n\n{}\n", String::from_utf8_lossy(&source_code));
        //println!("[QUERY]\n\n{}\n", query_source);

        let mut parser = tree_sitter::Parser::new();
        let language = &tree_sitter_go::LANGUAGE.into();
        parser
            .set_language(language)
            .expect("Error loading language parser");

        let tree = parser.parse(source_code.clone(), None).unwrap();
        let root_node = tree.root_node();

        let mut cursor = tree_sitter::QueryCursor::new();
        let query = tree_sitter::Query::new(language, &query_source).unwrap();
        let mut captures = cursor.captures(&query, root_node, source_code.as_slice());

        let mut current_tree_sitter_main_node: Option<tree_sitter::Node> = None;
        let mut current_node: Option<Node> = None;
        let mut parent_struct_name: Option<String> = None;

        // 使用 streaming iterator 的正确方式来迭代QueryCaptures
        while let Some((mat, capture_index)) = captures.next() {
            let capture = mat.captures[*capture_index];
            let capture_name = query.capture_names()[capture.index as usize];
            let pos_start = capture.node.start_position();
            let pos_end = capture.node.end_position();
            log::trace!(
                "[CAPTURE]\nname: {capture_name}, start: {}, end: {}, text: {:?}, capture: {:?}",
                pos_start,
                pos_end,
                capture.node.utf8_text(&source_code).unwrap_or(""),
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
                            let parts: Vec<&str> = mod_import_path.rsplitn(2, '/').collect();
                            let mod_name = parts.first().unwrap_or(&""); // get module name

                            let relationship = Relationship {
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
                            relationships.push(relationship);
                        }
                    }
                }
                "definition.interface"
                | "definition.class"
                | "definition.function"
                | "definition.method" => {
                    if let Some(ref mut prev_node) = current_node.take() {
                        if let Some(parent_struct_name) = &parent_struct_name {
                            let node_name = prev_node.name.rsplit(':').next().unwrap_or("");
                            prev_node.name = format!(
                                "{}:{}.{}",
                                Path::new(file_path)
                                    .strip_prefix(dir_path)
                                    .unwrap_or_else(|_| Path::new(file_path))
                                    .to_string_lossy(),
                                parent_struct_name,
                                node_name
                            );
                        }
                        nodes.insert(prev_node.name.clone(), prev_node.clone());

                        if prev_node.r#type == NodeType::Function {
                            let param_types = self.parse_go_func_params(
                                &prev_node.name,
                                current_tree_sitter_main_node
                                    .unwrap()
                                    .utf8_text(&source_code)
                                    .unwrap_or_default()
                                    .to_string()
                                    .as_bytes(),
                                &relationships,
                            )?;
                            for (k, v) in param_types {
                                func_param_types.insert(k, v);
                            }
                        }

                        let relationship =
                            if let Some(parent_struct_name) = parent_struct_name.take() {
                                let parent_node_name = prev_node
                                    .name
                                    .rsplit_once('.')
                                    .map(|(prefix, _)| prefix)
                                    .unwrap();
                                // Assume that the parent struct node is defined early in the current file.
                                let parent_node = nodes.get(parent_node_name).unwrap();
                                Relationship {
                                    r#type: EdgeType::Contains,
                                    from: parent_node.clone(),
                                    to: prev_node.clone(),
                                    import: None,
                                    alias: None,
                                }
                            } else {
                                Relationship {
                                    r#type: EdgeType::Contains,
                                    from: file_node.clone(),
                                    to: prev_node.clone(),
                                    import: None,
                                    alias: None,
                                }
                            };
                        relationships.push(relationship);
                    }

                    let node_type = match capture_name {
                        "definition.interface" => NodeType::Interface,
                        "definition.class" => NodeType::Class,
                        "definition.function" => NodeType::Function,
                        "definition.method" => NodeType::Function,
                        _ => NodeType::Unparsed,
                    };
                    current_node = Some(Node {
                        name: "".to_string(), // fill in later
                        r#type: node_type,
                        language: file_node.language.clone(),
                        start_line: capture.node.start_position().row,
                        end_line: capture.node.end_position().row,
                        code: capture
                            .node
                            .utf8_text(&source_code)
                            .unwrap_or("")
                            .to_string(),
                        skeleton_code: String::new(),
                    });
                    current_tree_sitter_main_node = Some(capture.node);
                    //println!("Create a new node: {:?}", current_node);
                }
                "definition.interface.name"
                | "definition.class.name"
                | "definition.function.name"
                | "definition.method.name" => {
                    let node_name: String = capture
                        .node
                        .utf8_text(&source_code)
                        .unwrap_or("")
                        .to_string();
                    if let Some(curr_node) = &mut current_node {
                        curr_node.name = format!(
                            "{}:{}",
                            Path::new(file_path)
                                .strip_prefix(dir_path)
                                .unwrap_or_else(|_| Path::new(file_path))
                                .to_string_lossy(),
                            node_name
                        );
                    }
                }
                "definition.method.receiver_type" | "definition.function.first_return_type" => {
                    // struct constructor or method
                    let node_name: String = capture
                        .node
                        .utf8_text(&source_code)
                        .unwrap_or("")
                        .to_string();
                    let struct_node_name = format!(
                        "{}:{}",
                        Path::new(file_path)
                            .strip_prefix(dir_path)
                            .unwrap_or_else(|_| Path::new(file_path))
                            .to_string_lossy(),
                        node_name,
                    );
                    // Assume that the struct node is defined early in the current file.
                    if nodes.contains_key(&struct_node_name) {
                        parent_struct_name = Some(node_name);
                    }
                }
                "definition.function.body" | "definition.method.body" => {
                    if let Some(current_tree_sitter_main_node) = current_tree_sitter_main_node {
                        let start_byte = current_tree_sitter_main_node.start_byte();
                        let body_start_byte = capture.node.start_byte();
                        if let Some(curr_node) = &mut current_node {
                            // Skip the body and keep only the signature.
                            curr_node.skeleton_code =
                                String::from_utf8_lossy(&source_code[start_byte..body_start_byte])
                                    .to_string()
                                    + "{\n...\n}";
                        }
                    }
                }
                _ => {}
            }
        }

        //println!("What??, current_node: {:?}, parent_struct_name: {:?}", current_node, parent_struct_name);

        // Add the last node, if any.
        if let Some(ref mut prev_node) = current_node.take() {
            if let Some(parent_struct_name) = &parent_struct_name {
                let node_name = prev_node.name.rsplit(':').next().unwrap_or("");
                prev_node.name = format!(
                    "{}:{}.{}",
                    Path::new(file_path)
                        .strip_prefix(dir_path)
                        .unwrap_or_else(|_| Path::new(file_path))
                        .to_string_lossy(),
                    parent_struct_name,
                    node_name
                );
            }
            nodes.insert(prev_node.name.clone(), prev_node.clone());

            let relationship = if let Some(parent_struct_name) = parent_struct_name.take() {
                let parent_node_name = prev_node
                    .name
                    .rsplit_once('.')
                    .map(|(prefix, _)| prefix)
                    .unwrap();
                // Assume that the parent struct node is defined early in the current file.
                let parent_node = nodes.get(parent_node_name).unwrap();
                Relationship {
                    r#type: EdgeType::Contains,
                    from: parent_node.clone(),
                    to: prev_node.clone(),
                    import: None,
                    alias: None,
                }
            } else {
                Relationship {
                    r#type: EdgeType::Contains,
                    from: file_node.clone(),
                    to: prev_node.clone(),
                    import: None,
                    alias: None,
                }
            };
            relationships.push(relationship);

            if prev_node.r#type == NodeType::Function {
                let param_types = self.parse_go_func_params(
                    &prev_node.name,
                    current_tree_sitter_main_node
                        .unwrap()
                        .utf8_text(&source_code)
                        .unwrap_or_default()
                        .to_string()
                        .as_bytes(),
                    &relationships,
                )?;
                for (k, v) in param_types {
                    func_param_types.insert(k, v);
                }
            }
        }

        Ok((nodes, relationships, Some(func_param_types)))
    }

    fn parse_go_func_params(
        &self,
        from_node_name: &String,
        source_code: &[u8],
        import_relationships: &Vec<Relationship>,
    ) -> Result<HashMap<String, Vec<FuncParamType>>, Box<dyn std::error::Error>> {
        let mut func_param_types: HashMap<String, Vec<FuncParamType>> = HashMap::new(); // function name -> parameter types

        let query_source = GO_FUNC_PARAMS_QUERY_SOURCE.to_string();
        let mut parser = tree_sitter::Parser::new();
        let language = &tree_sitter_go::LANGUAGE.into();
        parser
            .set_language(language)
            .expect("Error loading language parser");

        let tree = parser.parse(source_code, None).unwrap();
        let root_node = tree.root_node();

        let mut cursor = tree_sitter::QueryCursor::new();
        let query = tree_sitter::Query::new(language, &query_source).unwrap();
        let mut captures = cursor.captures(&query, root_node, source_code);

        // 使用 streaming iterator 的正确方式来迭代QueryCaptures
        while let Some((mat, capture_index)) = captures.next() {
            let capture = mat.captures[*capture_index];
            let capture_name = query.capture_names()[capture.index as usize];
            let pos_start = capture.node.start_position();
            let pos_end = capture.node.end_position();
            log::trace!(
                "[CAPTURE]\nname: {capture_name}, start: {}, end: {}, text: {:?}, capture: {:?}",
                pos_start,
                pos_end,
                capture.node.utf8_text(&source_code).unwrap_or_default(),
                capture.node.to_sexp()
            );

            if capture_name == "param_type" {
                let node_name: String = capture
                    .node
                    .utf8_text(&source_code)
                    .unwrap_or("")
                    .to_string();

                // Skip the inline type definitions
                // `f func (...) ...`
                // `s struct { ... }`
                // `iface interface { ... }`
                if node_name.starts_with("func")
                    || node_name.starts_with("struct")
                    || node_name.starts_with("interface")
                {
                    continue;
                }

                // Do conversion:
                // foo.Foo = > foo.Foo
                // Foo => Foo
                // *Foo => Foo
                // []*Foo => Foo
                // map[string]Foo => Foo
                let parts: Vec<&str> = node_name.rsplitn(2, |c| c == '*' || c == ']').collect();
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
                    for rel in import_relationships {
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

                if !util::is_go_builtin_type(&type_name) {
                    // Save the types referenced by the currrent function/method.
                    func_param_types
                        .entry(from_node_name.clone())
                        .or_insert_with(Vec::new)
                        .push(FuncParamType {
                            type_name,
                            package_name: real_package_name,
                        });
                }
            }
        }

        Ok(func_param_types)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_python() {
        // Create test file
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let dir_path = PathBuf::from(manifest_dir).join("examples").join("python");

        let config =
            ParserConfig::default().ignore_patterns(vec!["*".to_string(), "!d.py".to_string()]);
        let mut parser = Parser::new(dir_path.clone(), config);
        let result = parser.parse(dir_path);
        match result {
            Ok((nodes, relationships)) => {
                //for node in nodes {
                //    println!("Node: {:?}", node);
                //}
                //for rel in relationships {
                //    println!("Relationship: {:?}", rel);
                //}
            }
            Err(e) => {
                println!("Failed to parse: {:?}", e);
            }
        }
    }

    #[test]
    fn test_parse_go() {
        // Create test file
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let dir_path = PathBuf::from(manifest_dir)
            .join("examples")
            .join("go")
            .join("demo");

        let config = ParserConfig::default().ignore_patterns(vec![
            "*".to_string(),
            "!main.go".to_string(),
            "!types.go".to_string(),
        ]);

        let mut parser = Parser::new(dir_path.clone(), config);
        let result = parser.parse(dir_path);
        match result {
            Ok((nodes, relationships)) => {
                let mut node_strings: Vec<_> = nodes.into_iter().map(|n| n.name).collect();
                let mut rel_strings: Vec<_> = relationships
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

    /*
    #[test]
    fn test_traverse_directory_with_gitignore() {
        // 创建测试目录结构
        let test_dir = "test_gitignore_dir";
        fs::create_dir_all(format!("{}/subdir", test_dir)).unwrap();

        // 创建测试文件
        fs::write(format!("{}/file1.py", test_dir), "content1").unwrap();
        fs::write(format!("{}/file2.py", test_dir), "content2").unwrap();
        fs::write(format!("{}/subdir/file3.py", test_dir), "content3").unwrap();
        fs::write(format!("{}/.gitignore", test_dir), "file2.py\nsubdir/\n!subdir/file3.py").unwrap();

        // 用于收集处理过的文件路径
        let processed_files = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
        let processed_files_clone = Arc::clone(&processed_files);

        // 遍历目录并启用.gitignore
        let mut options = TraverseOptions::default();
        options.ignore_patterns = vec!["file1.py".to_string()];
        options.use_gitignore_files = true;

        let result = traverse_directory(test_dir, options, |path| {
            processed_files_clone.lock().unwrap().push(path.to_path_buf());
        });

        // 验证结果
        assert!(result.is_ok());

        let files = processed_files.lock().unwrap();
        assert_eq!(files.len(), 1); // 只有file3.py应该被处理

        // 验证file3.py被处理(由于否定规则)
        let file_names: Vec<String> = files.iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(file_names.contains(&"file3.py".to_string()));

        // 清理测试文件
        fs::remove_dir_all(test_dir).unwrap();
    }
    */
}

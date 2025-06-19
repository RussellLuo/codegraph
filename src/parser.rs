use glob::Pattern;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use strum_macros;
use tree_sitter;
use tree_sitter::StreamingIterator;
use tree_sitter_go;
use tree_sitter_python;
use walkdir::WalkDir;

use crate::util;
use crate::Database;
use crate::{Edge, EdgeType, Language, Node, NodeType};

mod common;
mod go;
mod python;
mod typescript;

use common::PendingImport;

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
    nodes: IndexMap<String, Node>,
    edges: Vec<Edge>,

    pending_imports: HashMap<String, Vec<PendingImport>>, // file node name -> imported info
    func_param_types: HashMap<String, Vec<FuncParamType>>, // function name -> parameter types

    // Language-specific parsers
    go_parser: go::Parser,
    typescript_parser: typescript::Parser,
    python_parser: python::Parser,
}

impl Parser {
    pub fn new(repo_path: PathBuf, config: ParserConfig) -> Self {
        Self {
            repo_path: repo_path.clone(),
            config: config,
            nodes: IndexMap::new(),
            edges: Vec::new(),
            pending_imports: HashMap::new(),
            func_param_types: HashMap::new(),

            go_parser: go::Parser::new(repo_path.clone()),
            typescript_parser: typescript::Parser::new(repo_path.clone()),
            python_parser: python::Parser::new(repo_path.clone()),
        }
    }

    /// Parses the directory and returns references to parsed nodes and edges
    ///
    /// # Arguments
    /// * `dir_path` - Path to the directory to parse
    ///
    /// # Returns
    /// Tuple of references to parsed nodes and edges vectors
    /// Will write JSON files to configured output directory if specified
    pub fn parse(
        &mut self,
        path: &PathBuf,
    ) -> Result<(IndexMap<String, Node>, Vec<Edge>), Box<dyn std::error::Error>> {
        if path.is_dir() {
            self.traverse_directory(&path)?;
        } else if path.is_file() {
            let (file_node, nodes, rels, pending_imports, func_param_types) =
                self.parse_file(&path)?;

            self.nodes.insert(file_node.name.clone(), file_node); // Add file node to nodes map
            for (n_name, n) in nodes {
                self.nodes.insert(n_name, n);
            }
            for r in rels {
                self.edges.push(r);
            }
            if let Some(pending_imports) = pending_imports {
                self.pending_imports.extend(pending_imports);
            }
            if let Some(func_param_types) = func_param_types {
                self.func_param_types.extend(func_param_types);
            }
        } else {
            return Err("Invalid path".into());
        }

        Ok((self.nodes.clone(), self.edges.clone()))
    }

    pub fn resolve_pending_edges(
        &self,
        db: Option<&mut Database>,
    ) -> Result<Vec<Edge>, Box<dyn std::error::Error>> {
        let mut edges: Vec<Edge> = Vec::new();

        let import_edges = self.resolve_pending_imports()?;
        for edge in import_edges {
            edges.push(edge);
        }

        if let Some(db) = db {
            let ref_edges =
                self.resolve_func_param_type_edges(&self.nodes, &self.func_param_types, db)?;
            for edge in ref_edges {
                edges.push(edge);
            }
        }

        Ok(edges)
    }

    pub fn resolve_pending_imports(&self) -> Result<Vec<Edge>, Box<dyn std::error::Error>> {
        // TypeScript-specific import resolution
        return self
            .typescript_parser
            .resolve_pending_imports(&self.nodes, &self.pending_imports);

        Ok(vec![])
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

    /// Traverses all files and directories in the specified directory, creates Node and Edge objects
    /// This method processes files by calling self.parse_file directly when encountering supported file types
    ///
    /// # Arguments
    /// - `dir_path`: The directory path to traverse
    ///
    /// # Returns
    /// - A tuple containing (nodes, edges) where:
    ///   - nodes: Vector of Node objects representing directories, files, and parsed code elements
    ///   - edges: Vector of Edge objects representing Contains edges
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
                        let (file_node, nodes, rels, pending_imports, func_param_types) =
                            self.parse_file(&entry_path)?;
                        for (n_name, n) in nodes {
                            self.nodes.insert(n_name, n);
                        }
                        for r in rels {
                            self.edges.push(r);
                        }
                        if let Some(pending_imports) = pending_imports {
                            self.pending_imports.extend(pending_imports);
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

                    // Find parent directory and create Contains edge
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
                            let edge = Edge {
                                r#type: EdgeType::Contains,
                                from: parent_node.clone(),
                                to: current_node.clone(),
                                import: None,
                                alias: None,
                            };
                            self.edges.push(edge);
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
            Vec<Edge>,
            Option<HashMap<String, Vec<PendingImport>>>,
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
            Language::Go => {
                let (nodes, rels, func_param_types) =
                    self.go_parser.parse(&file_node, &file_path.to_path_buf())?;
                return Ok((file_node, nodes, rels, None, func_param_types));
            }
            Language::TypeScript => {
                let (nodes, rels, pending_imports, func_param_types) = self
                    .typescript_parser
                    .parse(&file_node, &file_path.to_path_buf())?;
                return Ok((file_node, nodes, rels, pending_imports, func_param_types));
            }
            Language::Python => {
                let (nodes, rels) = self
                    .python_parser
                    .parse(&file_node, &file_path.to_path_buf())?;
                return Ok((file_node, nodes, rels, None, None));
            }
            Language::Text => {
                return Ok((file_node, IndexMap::new(), vec![], None, None));
            }
        }
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
        let result = parser.parse(&dir_path);
        match result {
            Ok((nodes, edges)) => {
                //for node in nodes {
                //    println!("Node: {:?}", node);
                //}
                //for rel in edges {
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
        let result = parser.parse(&dir_path);
        match result {
            Ok((nodes, edges)) => {
                let mut node_strings: Vec<_> = nodes.values().cloned().map(|n| n.name).collect();
                let mut edge_strings: Vec<_> = edges
                    .into_iter()
                    .map(|r| format!("{}-[{}]->{}", r.from.name, r.r#type, r.to.name))
                    .collect();

                node_strings.sort();
                edge_strings.sort();

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
                    edge_strings,
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

    #[test]
    fn test_parse_typescript() {
        // Create test file
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let dir_path = PathBuf::from(manifest_dir)
            .join("examples")
            .join("typescript");

        let config =
            ParserConfig::default().ignore_patterns(vec!["*".to_string(), "!*.ts".to_string()]);

        let mut parser = Parser::new(dir_path.clone(), config);
        let result = parser.parse(&dir_path);
        match result {
            Ok((nodes, edges)) => {
                let result = parser.resolve_pending_edges(None);
                match result {
                    Err(e) => {
                        println!("Failed to resolve pending edges: {:?}", e);
                    }
                    Ok(resolved_edges) => {
                        // merge edges and resolved edges
                        let mut edges = edges;
                        edges.extend(resolved_edges);

                        let mut node_strings: Vec<_> =
                            nodes.values().cloned().map(|n| n.name).collect();
                        let mut edge_strings: Vec<_> = edges
                            .into_iter()
                            .map(|r| format!("{}-[{}]->{}", r.from.name, r.r#type, r.to.name))
                            .collect();

                        node_strings.sort();
                        edge_strings.sort();

                        assert_eq!(
                            node_strings,
                            [
                                "",
                                "main.ts",
                                "types.ts",
                                "types.ts:User",
                                "types.ts:UserService"
                            ],
                        );
                        assert_eq!(
                            edge_strings,
                            [
                                "-[contains]->main.ts",
                                "-[contains]->types.ts",
                                "main.ts-[imports]->types.ts:User",
                                "main.ts-[imports]->types.ts:UserService",
                                "types.ts-[contains]->types.ts:User",
                                "types.ts-[contains]->types.ts:UserService"
                            ],
                        );
                    }
                }
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

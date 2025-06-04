use glob::Pattern;
use serde::Serialize;
use std::collections::HashMap;
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

/// The tree-sitter definition query source for different languages.
pub const PYTHON_DEFINITIONS_QUERY_SOURCE: &str = include_str!("python/definitions.scm");
pub const GO_DEFINITIONS_QUERY_SOURCE: &str = include_str!("go/definitions.scm");

#[derive(Debug, Clone, strum_macros::EnumString, strum_macros::Display, serde::Serialize)]
pub enum NodeType {
    #[strum(serialize = "unparsed")]
    Unparsed,
    #[strum(serialize = "directory")]
    Directory,
    #[strum(serialize = "file")]
    File,
    #[strum(serialize = "class")]
    Class,
    #[strum(serialize = "function")]
    Function,
}

#[derive(Debug, Clone, strum_macros::Display, strum_macros::EnumString, serde::Serialize)]
pub enum EdgeType {
    #[strum(serialize = "contains")]
    Contains,
    #[strum(serialize = "imports")]
    Imports,
    #[strum(serialize = "inherits")]
    Inherits,
    #[strum(serialize = "references")]
    References,
}

#[derive(Debug, Clone, strum_macros::Display, strum_macros::EnumString, serde::Serialize)]
pub enum Language {
    Text,
    Python,
    Go,
    // TypeScript,
    // JavaScript,
}

impl Language {
    fn from_path(path: &str) -> Self {
        let ext = Path::new(path).extension().and_then(|e| e.to_str());

        match ext {
            Some("py") => Language::Python,
            Some("go") => Language::Go,
            // Some("ts") => Language::TypeScript,
            // Some("js") => Language::JavaScript,
            _ => Language::Text,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    /// File path
    pub name: String,
    // Node type
    pub r#type: NodeType,
    // Language type
    pub language: Language,
    /// Start line (0-based)
    pub start_line: usize,
    /// End line (0-based)
    pub end_line: usize,
    /// The code text
    pub code: String,
}

impl Node {
    pub fn from_dict(data: &HashMap<String, serde_json::Value>) -> Self {
        Self {
            name: data.get("name").unwrap().as_str().unwrap().to_string(),
            r#type: match data.get("type").unwrap().as_str().unwrap() {
                "Unparsed" => NodeType::Unparsed,
                "Class" => NodeType::Class,
                _ => NodeType::Unparsed, // 默认值
            },
            language: data
                .get("lanuage")
                .unwrap()
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
            start_line: data.get("start_line").unwrap().as_u64().unwrap() as usize,
            end_line: data.get("end_line").unwrap().as_u64().unwrap() as usize,
            code: data
                .get("code")
                .map(|v| v.as_str().unwrap().to_string())
                .unwrap_or_default(),
        }
    }

    pub fn short_names(&self) -> Vec<String> {
        fn make_names(name: &str) -> Vec<String> {
            let lower = name.to_lowercase();
            if lower != name {
                vec![name.to_string(), lower]
            } else {
                vec![name.to_string()]
            }
        }

        if !self.name.contains(':') {
            // "src/a.py" => a
            let file_name = self.name.rsplit('/').next().unwrap_or(&self.name.as_str());
            make_names(file_name)
        } else {
            // "src/a.py:A" => A, a
            let attr_name = self.name.rsplit(':').next().unwrap_or(self.name.as_str());
            if !attr_name.contains('.') {
                make_names(attr_name)
            } else {
                // "src/a.py:A.meth" => meth
                let sub_attr_name = attr_name.rsplit('.').next().unwrap_or(attr_name);
                make_names(sub_attr_name)
            }
        }
    }

    /// 将Node转换为字典格式，包含基本字段和short_names字段
    pub fn to_dict(&self) -> HashMap<String, serde_json::Value> {
        let mut dict = HashMap::new();

        // 添加基本字段
        dict.insert(
            "name".to_string(),
            serde_json::Value::String(self.name.clone()),
        );
        dict.insert(
            "type".to_string(),
            serde_json::Value::String(self.r#type.to_string()),
        );
        dict.insert(
            "language".to_string(),
            serde_json::Value::String(self.language.to_string()),
        );
        dict.insert(
            "start_line".to_string(),
            serde_json::Value::Number(serde_json::Number::from(self.start_line)),
        );
        dict.insert(
            "end_line".to_string(),
            serde_json::Value::Number(serde_json::Number::from(self.end_line)),
        );
        dict.insert(
            "code".to_string(),
            serde_json::Value::String(self.code.clone()),
        );

        // 添加short_names字段
        let short_names: Vec<serde_json::Value> = self
            .short_names()
            .into_iter()
            .map(|name| serde_json::Value::String(name))
            .collect();
        dict.insert(
            "short_names".to_string(),
            serde_json::Value::Array(short_names),
        );

        dict
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Relationship {
    /// 关系类型
    pub r#type: EdgeType,
    /// 起始节点
    pub from: Node,
    /// 目标节点
    pub to: Node,
    /// 导入路径（可选）
    pub import: Option<String>,
    /// 别名（可选）
    pub alias: Option<String>,
}

impl Relationship {
    /// 从字典数据创建关系
    pub fn from_dict(
        data: &HashMap<String, serde_json::Value>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let type_str = data
            .get("_label")
            .and_then(|v| v.as_str())
            .ok_or("Missing _label field")?;
        let edge_type = type_str.parse::<EdgeType>()?;

        let type_field = data
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or("Missing type field")?;
        let parts: Vec<&str> = type_field.split('_').collect();
        if parts.len() != 2 {
            return Err("Invalid type format".into());
        }

        let from_type = parts[0].parse::<NodeType>()?;
        let to_type = parts[1].parse::<NodeType>()?;

        let from_node = Node {
            name: String::new(),
            r#type: from_type,
            language: Language::Text,
            start_line: 0,
            end_line: 0,
            code: String::new(),
        };

        let to_node = Node {
            name: String::new(),
            r#type: to_type,
            language: Language::Text,
            start_line: 0,
            end_line: 0,
            code: String::new(),
        };

        let import = data
            .get("import")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let alias = data
            .get("alias")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(Relationship {
            r#type: edge_type,
            from: from_node,
            to: to_node,
            import,
            alias,
        })
    }

    /// 转换为字典格式
    pub fn to_dict(&self) -> HashMap<String, serde_json::Value> {
        let mut dict = HashMap::new();

        dict.insert(
            "from".to_string(),
            serde_json::Value::String(self.from.name.clone()),
        );
        dict.insert(
            "to".to_string(),
            serde_json::Value::String(self.to.name.clone()),
        );
        dict.insert(
            "type".to_string(),
            serde_json::Value::String(self.from_to()),
        );

        match self.r#type {
            EdgeType::Imports => {
                if let Some(ref import) = self.import {
                    dict.insert(
                        "import".to_string(),
                        serde_json::Value::String(import.clone()),
                    );
                }
                if let Some(ref alias) = self.alias {
                    dict.insert(
                        "alias".to_string(),
                        serde_json::Value::String(alias.clone()),
                    );
                }
            }
            _ => {}
        }

        dict
    }

    /// 获取from_to字符串表示
    pub fn from_to(&self) -> String {
        format!(
            "{}_{}",
            self.from.r#type.to_string().to_lowercase(),
            self.to.r#type.to_string().to_lowercase()
        )
    }
}

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
    /// Output directory for saving parsed nodes as JSON file (default is None)
    /// If specified, the parsed nodes will be written to a JSON file in this directory
    pub out_dir: Option<String>,
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
            out_dir: None,
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

    /// 设置输出目录，用于保存解析后的节点JSON文件
    pub fn out_dir(mut self, out_dir: String) -> Self {
        self.out_dir = Some(out_dir);
        self
    }
}

pub struct Parser {
    config: ParserConfig,
    nodes: HashMap<String, Node>,
    relationships: Vec<Relationship>,
}

impl Parser {
    pub fn new(config: ParserConfig) -> Self {
        Parser {
            config: config,
            nodes: HashMap::new(),
            relationships: Vec::new(),
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
        self.traverse_directory(&dir_path)?;
        let nodes: Vec<Node> = self.nodes.values().cloned().collect();

        // If output directory is specified, write parsed results to JSON files
        if let Some(ref out_dir) = self.config.out_dir {
            let nodes_dir = PathBuf::from(out_dir).join("nodes");
            let rels_dir = PathBuf::from(out_dir).join("relationships");
            std::fs::create_dir_all(&nodes_dir)?;
            std::fs::create_dir_all(&rels_dir)?;
            self.write_nodes_to_json(&nodes, nodes_dir.to_str().unwrap())?;
            self.write_relationships_to_json(&self.relationships, rels_dir.to_str().unwrap())?;

            Ok((vec![], vec![]))
        } else {
            // Return references to parsed nodes and relationships
            Ok((nodes, self.relationships.clone()))
        }
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
            name: dir_path
                .strip_prefix(dir_path)
                .unwrap_or(dir_path)
                .to_string_lossy()
                .to_string(),
            r#type: NodeType::Directory,
            language: Language::Text,
            start_line: 0,
            end_line: 0,
            code: String::new(),
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
                        }
                    } else {
                        let file_language =
                            Language::from_path(entry_path.to_path_buf().to_str().unwrap());
                        let file_node = Node {
                            name: entry_path
                                .strip_prefix(dir_path)
                                .unwrap_or(entry_path)
                                .to_string_lossy()
                                .to_string(),
                            r#type: NodeType::File,
                            language: file_language,
                            start_line: 0,
                            end_line: 0,
                            code: String::new(),
                        };
                        // Parse the file and add parsed nodes to the collection
                        match file_node.language {
                            Language::Python => self.parse_python_file(
                                &file_node,
                                dir_path,
                                &entry_path.to_path_buf(),
                                "",
                            )?,
                            Language::Go => self.parse_go_file(
                                &file_node,
                                dir_path,
                                &entry_path.to_path_buf(),
                                "",
                            )?,
                            Language::Text => (),
                        }

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

    /// 将解析的节点按类型分组写入JSON文件
    fn write_nodes_to_json(
        &self,
        nodes: &[Node],
        out_dir: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 确保输出目录存在
        std::fs::create_dir_all(out_dir)?;

        // 按节点类型分组
        let mut grouped_nodes: HashMap<String, Vec<HashMap<String, serde_json::Value>>> =
            HashMap::new();

        for node in nodes {
            let type_key = node.r#type.to_string();
            let node_dict = node.to_dict();
            grouped_nodes
                .entry(type_key)
                .or_insert_with(Vec::new)
                .push(node_dict);
        }

        // 为每个节点类型创建单独的JSON文件
        for (node_type, type_nodes) in grouped_nodes {
            let json_filename = format!("{}.json", node_type);
            let json_path = PathBuf::from(out_dir).join(json_filename);

            // 将该类型的节点序列化为JSON
            let json_content = serde_json::to_string_pretty(&type_nodes)?;
            // 写入文件
            std::fs::write(&json_path, json_content)?;
            /*println!(
                "已写入 {} 个 {} 类型的节点到文件: {}",
                type_nodes.len(),
                node_type,
                json_path.display()
            );*/
        }

        Ok(())
    }

    /// 将解析的关系按类型分组写入JSON文件
    fn write_relationships_to_json(
        &self,
        relationships: &[Relationship],
        out_dir: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 确保输出目录存在
        std::fs::create_dir_all(out_dir)?;

        // 按关系类型分组，使用 to_dict() 转换为字典格式
        let mut grouped_relationships: HashMap<String, Vec<HashMap<String, serde_json::Value>>> =
            HashMap::new();

        for relationship in relationships {
            let key = format!(
                "{}_{}_{}.json",
                relationship.r#type.to_string(),
                relationship.from.r#type.to_string(),
                relationship.to.r#type.to_string()
            );
            let relationship_dict = relationship.to_dict();
            grouped_relationships
                .entry(key)
                .or_insert_with(Vec::new)
                .push(relationship_dict);
        }

        // 为每个关系类型创建单独的JSON文件
        for (key, type_relationships) in grouped_relationships {
            let json_filename = &key;
            let json_path = PathBuf::from(out_dir).join(json_filename);

            // 将该类型的关系序列化为JSON（现在使用 to_dict() 的结果）
            let json_content = serde_json::to_string_pretty(&type_relationships)?;
            // 写入文件
            std::fs::write(&json_path, json_content)?;
            /*println!(
                "已写入 {} 个 {} 类型的关系到文件: {}",
                type_relationships.len(),
                key,
                json_path.display()
            );*/
        }

        Ok(())
    }

    fn parse_python_file(
        &mut self,
        file_node: &Node,
        dir_path: &Path,
        file_path: &PathBuf,
        query_path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let query_source = if query_path.is_empty() {
            PYTHON_DEFINITIONS_QUERY_SOURCE.to_string()
        } else {
            let query_path = PathBuf::from(query_path);
            fs::read_to_string(query_path).expect("Should have been able to read the query file")
        };

        if query_source == "" {
            return Ok(());
        }

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
            /*println!(
                "[CAPTURE]\nname: {capture_name}, start: {}, end: {}, text: {:?}, capture: {:?}",
                pos_start,
                pos_end,
                capture.node.utf8_text(&source_code).unwrap_or(""),
                capture.node.to_sexp()
            );*/

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
                        };
                        let _ = self.add_node(&node);

                        let relationship = Relationship {
                            r#type: EdgeType::Contains,
                            from: file_node.clone(),
                            to: node.clone(),
                            import: None,
                            alias: None,
                        };
                        self.relationships.push(relationship);
                    }
                }
                "definition.class" => {
                    cur_class_node = Some(capture.node);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn parse_go_file(
        &mut self,
        file_node: &Node,
        dir_path: &Path,
        file_path: &PathBuf,
        query_path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let query_source = if query_path.is_empty() {
            GO_DEFINITIONS_QUERY_SOURCE.to_string()
        } else {
            let query_path = PathBuf::from(query_path);
            fs::read_to_string(query_path).expect("Should have been able to read the query file")
        };

        if query_source == "" {
            return Ok(());
        }

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

        let mut current_node: Option<Node> = None;
        let mut parent_struct_name: Option<String> = None;

        // 使用 streaming iterator 的正确方式来迭代QueryCaptures
        while let Some((mat, capture_index)) = captures.next() {
            let capture = mat.captures[*capture_index];
            let capture_name = query.capture_names()[capture.index as usize];
            let pos_start = capture.node.start_position();
            let pos_end = capture.node.end_position();
            /*println!(
                "[CAPTURE]\nname: {capture_name}, start: {}, end: {}, text: {:?}, capture: {:?}",
                pos_start,
                pos_end,
                capture.node.utf8_text(&source_code).unwrap_or(""),
                capture.node.to_sexp()
            );*/

            match capture_name {
                "definition.class" | "definition.function" | "definition.method" => {
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
                        let _ = self.add_node(&prev_node);

                        let relationship =
                            if let Some(parent_struct_name) = parent_struct_name.take() {
                                let parent_node_name = prev_node
                                    .name
                                    .rsplit_once('.')
                                    .map(|(prefix, _)| prefix)
                                    .unwrap();
                                let parent_node = self.nodes.get(parent_node_name).unwrap();
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
                        self.relationships.push(relationship);
                    }

                    current_node = Some(Node {
                        name: "".to_string(),       // fill in later
                        r#type: NodeType::Unparsed, // fill in later
                        language: file_node.language.clone(),
                        start_line: capture.node.start_position().row + 1,
                        end_line: capture.node.end_position().row + 1,
                        code: capture
                            .node
                            .utf8_text(&source_code)
                            .unwrap_or("")
                            .to_string(),
                    });
                    //println!("Create a new node: {:?}", current_node);
                }
                "definition.class.name" | "definition.function.name" | "definition.method.name" => {
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
                        curr_node.r#type = match capture_name {
                            "definition.class.name" => NodeType::Class,
                            "definition.function.name" => NodeType::Function,
                            "definition.method.name" => NodeType::Function,
                            _ => NodeType::Unparsed,
                        };
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
                    // 检查父节点是否存在，先调用闭包再检查
                    if self.nodes.contains_key(&struct_node_name) {
                        parent_struct_name = Some(node_name);
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
            let _ = self.add_node(&prev_node);

            let relationship = if let Some(parent_struct_name) = parent_struct_name.take() {
                let parent_node_name = prev_node
                    .name
                    .rsplit_once('.')
                    .map(|(prefix, _)| prefix)
                    .unwrap();
                let parent_node = self.nodes.get(parent_node_name).unwrap();
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
            self.relationships.push(relationship);
        }

        Ok(())
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
        let out_dir = dir_path.join("temp_out").to_str().unwrap().to_string();

        let config = ParserConfig::default()
            .ignore_patterns(vec!["*".to_string(), "!d.py".to_string()])
            .out_dir(out_dir);
        let mut parser = Parser::new(config);
        let result = parser.parse(dir_path);
        match result {
            Ok((nodes, relationships)) => {
                for node in nodes {
                    println!("Node: {:?}", node);
                }
                for rel in relationships {
                    println!("Relationship: {:?}", rel);
                }
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
        let out_dir = dir_path.join("temp_out").to_str().unwrap().to_string();

        let config = ParserConfig::default()
            .ignore_patterns(vec!["*".to_string(), "!*.go".to_string()])
            .out_dir(out_dir);
        let mut parser = Parser::new(config);
        let result = parser.parse(dir_path);
        match result {
            Ok((nodes, relationships)) => {
                for node in nodes {
                    println!("Node: {:?}", node);
                }
                for rel in relationships {
                    println!("Relationship: {:?}", rel);
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

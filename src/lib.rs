use std::fs;
use std::path::Path;
use std::path::PathBuf;
use glob::Pattern;
use walkdir::WalkDir;
use tree_sitter;
use tree_sitter::StreamingIterator;
use tree_sitter_python;
use tree_sitter_go;
use strum_macros;

/// The tree-sitter definition query source for different languages.
pub const PYTHON_DEFINITIONS_QUERY_SOURCE: &str = include_str!("python/definitions.scm");
pub const GO_DEFINITIONS_QUERY_SOURCE: &str = include_str!("go/definitions.scm");

#[derive(Debug, Clone, strum_macros::EnumString, strum_macros::Display)]
pub enum NodeType{
    #[strum(serialize = "unparsed")]
    UNPARSED,
    #[strum(serialize = "class")]
    CLASS,
}

#[derive(Debug, Clone)]
pub enum Language {
    Python,
    Go,
    // TypeScript,
    // JavaScript,
}

impl From<&str> for Language {
    fn from(path: &str) -> Self {
        let ext = Path::new(path).extension().and_then(|e| e.to_str());

        match ext {
            Some("py") => Language::Python,
            Some("go") => Language::Go,
            // Some("ts") => Language::TypeScript,
            // Some("js") => Language::JavaScript,
            _ => panic!("Unsupport extension: {:?}", ext),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    /// File path
    pub name: String,
    // Node type
    pub typ: NodeType,
    /// Start line (0-based)
    pub start_line: usize,
    /// End line (0-based)
    pub end_line: usize,
    /// The code text
    pub code: String,
}

pub struct Parser {}

impl Parser {
    pub fn new() -> Self {
        Parser {}
    }
    pub fn parse(&mut self, repo_path: &str, dir_path: &str) -> Result<Vec<Node>, Box<dyn std::error::Error>> {
        // 遍历目录并启用.gitignore
        let mut options = TraverseOptions::default();
        options.ignore_patterns = vec![];
        options.use_gitignore_files = true;

        let mut nodes: Vec<Node> = Vec::new();
        let result = traverse_directory(dir_path, options, |path| {
            // 处理 path.to_str() 返回的 Option<&str> 类型
            if let Some(path_str) = path.to_str() {
                let parsed_nodes = self._parse(repo_path,  path_str, "");
                nodes.extend(parsed_nodes.unwrap());
            } else {
                eprintln!("警告: 无法将路径转换为字符串: {:?}", path);
            }
        });

        Ok(nodes)
    }

    fn _parse(&mut self, repo_path: &str, file_path: &str, query_path: &str) -> Result<Vec<Node>, Box<dyn std::error::Error>> {
        let snippet_language = Language::from(file_path);
        let query_source = if query_path.is_empty() {
            match snippet_language {
                Language::Python => PYTHON_DEFINITIONS_QUERY_SOURCE.to_string(),
                Language::Go => GO_DEFINITIONS_QUERY_SOURCE.to_string(),
                // Language::TypeScript => TYPESCRIPT_DEFINITIONS_QUERY_SOURCE.to_string(),
                // Language::JavaScript => JAVASCRIPT_DEFINITIONS_QUERY_SOURCE.to_string(),
            }
        } else {
            let query_path = PathBuf::from(query_path);
            fs::read_to_string(query_path).expect("Should have been able to read the query file")
        };

        let source_code = fs::read(&file_path).expect("Should have been able to read the file");

        //println!("[SOURCE]\n\n{}\n", String::from_utf8_lossy(&source_code));
        //println!("[QUERY]\n\n{}\n", query_source);

        let mut parser = tree_sitter::Parser::new();
        let language = match snippet_language {
            Language::Python => &tree_sitter_python::LANGUAGE.into(),
            Language::Go => &tree_sitter_go::LANGUAGE.into(),
            // Language::TypeScript => tree_sitter_typescript::language_typescript(),
            // Language::JavaScript => tree_sitter_javascript::language(),
        };
        parser
            .set_language(language)
            .expect("Error loading Python parser");

        let tree = parser.parse(source_code.clone(), None).unwrap();
        let root_node = tree.root_node();

        let mut cursor = tree_sitter::QueryCursor::new();
        let query = tree_sitter::Query::new(language, &query_source).unwrap();
        let mut captures = cursor.captures(&query, root_node, source_code.as_slice());

        let mut nodes: Vec<Node> = Vec::new();
        let mut cur_class_node : Option<tree_sitter::Node> = None;
        // 使用 streaming iterator 的正确方式来迭代 QueryCaptures
        while let Some((mat, capture_index)) = captures.next() {
            let capture = mat.captures[*capture_index];
            let capture_name = query.capture_names()[capture.index as usize];
            let pos_start = capture.node.start_position();
            let pos_end = capture.node.end_position();
            //println!("[CAPTURE]\nname: {capture_name}, start: {}, end: {}, text: {:?}, capture: {:?}", pos_start, pos_end, capture.node.utf8_text(&source_code).unwrap_or(""), capture.node.to_sexp());

            match capture_name {
                "definition.class.name" => {
                    let class_name: String = capture.node.utf8_text(&source_code).unwrap_or("").to_string();
                    if let Some(class_node) = cur_class_node {
                        let node = Node {
                            name: format!("{}:{}", file_path, class_name),
                            typ: NodeType::CLASS,
                            start_line: class_node.start_position().row + 1,
                            end_line: class_node.end_position().row + 1,
                            code: class_node
                                .utf8_text(&source_code)
                                .unwrap_or("")
                                .to_string(),
                        };
                        nodes.push(node);
                    }
                },
                "definition.class" => {
                    cur_class_node = Some(capture.node);
                }
                _ => {
                }
            }
        }
        
        Ok(nodes)
    }
}

/// Configuration options struct for controlling directory traversal behavior
#[derive(Debug, Clone)]
pub struct TraverseOptions {
    /// Whether to recursively traverse subdirectories (default is true)
    pub recursive: bool,
    /// Whether to follow symbolic links (default is false)
    pub follow_links: bool,
    /// Maximum recursion depth, None means no limit (default is None)
    pub max_depth: Option<usize>,
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

impl Default for TraverseOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            follow_links: false,
            max_depth: None,
            continue_on_error: false,
            ignore_patterns: Vec::new(),
            use_gitignore_files: true,
        }
    }
}

/// Traverses all files in the specified directory and calls a processing function for each file
///
/// # Arguments
/// - `dir_path`: The directory path to traverse
/// - `options`: Traversal options configuration
/// - `process_file`: Function to process each file, receives file path as parameter
///
/// # Examples
/// ```rust
/// use codegraph::{traverse_directory, TraverseOptions};
///
/// let mut options = TraverseOptions::default();
/// options.ignore_patterns = vec!["*.log".to_string(), "node_modules/".to_string()];
/// traverse_directory("./src", options, |file_path| {
///     println!("Processing file: {}", file_path.display());
/// }).unwrap();
/// ```
pub fn traverse_directory<F>(
    dir_path: &str,
    options: TraverseOptions,
    mut process_file: F
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(&Path),
{
    // Check if directory exists
    let path = Path::new(dir_path);
    if !path.exists() {
        return Err(format!("Directory does not exist: {}", dir_path).into());
    }

    // Create WalkDir instance and apply configuration options
    let mut walkdir = WalkDir::new(dir_path);
    
    // Configure whether to follow symbolic links
    walkdir = walkdir.follow_links(options.follow_links);
    
    // Configure maximum recursion depth
    if let Some(depth) = options.max_depth {
        walkdir = walkdir.max_depth(depth);
    }
    
    // If not recursive, set depth to 1 (only traverse current directory)
    if !options.recursive {
        walkdir = walkdir.max_depth(1);
    }

    // Compile ignore patterns
    let mut ignore_patterns: Vec<Pattern> = options.ignore_patterns
        .iter()
        .filter_map(|p| Pattern::new(p).ok())
        .collect();

    // Add patterns from .gitignore files if enabled
    if options.use_gitignore_files {
        if let Ok(gitignore_path) = Path::new(dir_path).join(".gitignore").canonicalize() {
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

    // Traverse directory
    for entry in walkdir {
        match entry {
            Ok(entry) => {
                // Only process files, skip directories
                if entry.file_type().is_file() && entry.path().extension().map_or(false, |ext| ext == "py") {
                    let path = entry.path();
                    
                    // Get relative path from the root directory
                    let rel_path = path.strip_prefix(dir_path)
                        .unwrap_or(path)
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
                    
                    if !should_skip {
                        process_file(path);
                    }
                }
            }
            Err(err) => {
                // Decide whether to continue on error based on configuration
                if options.continue_on_error {
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

/// Provides a simplified version without options for backward compatibility
///
/// # Arguments
/// - `dir_path`: The directory path to traverse
/// - `process_file`: Function to process each file, receives file path as parameter
///
/// # Examples
/// ```rust
/// use codegraph::traverse_directory_simple;
///
/// traverse_directory_simple("./src", |file_path| {
///     println!("Processing file: {}", file_path.display());
/// }).unwrap();
/// ```
pub fn traverse_directory_simple<F>(dir_path: &str, process_file: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(&Path),
{
    traverse_directory(dir_path, TraverseOptions::default(), process_file)
}

/// Example processing function: prints file information
///
/// # Arguments
/// - `file_path`: File path
pub fn print_file_info(file_path: &Path) {
    println!("File: {}", file_path.display());
    
    // Get file extension
    if let Some(extension) = file_path.extension() {
        println!("  Extension: {:?}", extension);
    }
    
    // Get file size
    if let Ok(metadata) = std::fs::metadata(file_path) {
        println!("  Size: {} bytes", metadata.len());
    }
    
    println!("  ----");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_parse() {
        // Create test file
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let repo_dir = PathBuf::from(manifest_dir).join("examples").join("python");
        let code_dir = repo_dir.join("d.py");

        let mut parser = Parser::new();
        if let (Some(repo_dir), Some(code_dir)) = (repo_dir.to_str(), code_dir.to_str()) {
            let nodes = parser.parse(repo_dir, code_dir);
            for node in nodes.unwrap() {
                println!("Node: {:?}", node);
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
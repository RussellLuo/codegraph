use codegraph;
use napi_derive::napi;
use std::path::PathBuf;

#[napi(string_enum)]
pub enum NodeType {
    Unparsed,
    Directory,
    File,
    Interface,
    Class,
    Function,
    OtherType,
}

impl From<codegraph::NodeType> for NodeType {
    fn from(r#type: codegraph::NodeType) -> Self {
        match r#type {
            codegraph::NodeType::Unparsed => NodeType::Unparsed,
            codegraph::NodeType::Directory => NodeType::Directory,
            codegraph::NodeType::File => NodeType::File,
            codegraph::NodeType::Interface => NodeType::Interface,
            codegraph::NodeType::Class => NodeType::Class,
            codegraph::NodeType::Function => NodeType::Function,
            codegraph::NodeType::OtherType => NodeType::OtherType,
        }
    }
}

impl Into<codegraph::NodeType> for NodeType {
    fn into(self) -> codegraph::NodeType {
        match self {
            NodeType::Unparsed => codegraph::NodeType::Unparsed,
            NodeType::Directory => codegraph::NodeType::Directory,
            NodeType::File => codegraph::NodeType::File,
            NodeType::Interface => codegraph::NodeType::Interface,
            NodeType::Class => codegraph::NodeType::Class,
            NodeType::Function => codegraph::NodeType::Function,
            NodeType::OtherType => codegraph::NodeType::OtherType,
        }
    }
}

#[napi(string_enum)]
pub enum EdgeType {
    Contains,
    Imports,
    Inherits,
    References,
}

impl From<codegraph::EdgeType> for EdgeType {
    fn from(r#type: codegraph::EdgeType) -> Self {
        match r#type {
            codegraph::EdgeType::Contains => EdgeType::Contains,
            codegraph::EdgeType::Imports => EdgeType::Imports,
            codegraph::EdgeType::Inherits => EdgeType::Inherits,
            codegraph::EdgeType::References => EdgeType::References,
        }
    }
}

impl Into<codegraph::EdgeType> for EdgeType {
    fn into(self) -> codegraph::EdgeType {
        match self {
            EdgeType::Contains => codegraph::EdgeType::Contains,
            EdgeType::Imports => codegraph::EdgeType::Imports,
            EdgeType::Inherits => codegraph::EdgeType::Inherits,
            EdgeType::References => codegraph::EdgeType::References,
        }
    }
}

#[napi(object)]
#[derive(Clone)]
pub struct Node {
    /// File path
    pub name: String,
    pub short_name: String,
    // Node type
    pub r#type: NodeType,
    // Language type
    pub language: String,
    /// Start line (0-based)
    pub start_line: u32,
    /// End line (0-based)
    pub end_line: u32,
    /// The code text
    pub code: String,
    /// The skeleton code text
    pub skeleton_code: String,
}

impl From<codegraph::Node> for Node {
    fn from(n: codegraph::Node) -> Self {
        // 先获取需要借用的数据，避免所有权冲突
        let short_name = n.short_name().clone();
        Self {
            name: n.name,
            short_name,
            r#type: NodeType::from(n.r#type),
            language: n.language.to_string(),
            start_line: n.start_line as u32,
            end_line: n.end_line as u32,
            code: n.code,
            skeleton_code: n.skeleton_code,
        }
    }
}

impl Into<codegraph::Node> for Node {
    fn into(self) -> codegraph::Node {
        codegraph::Node {
            name: self.name,
            r#type: self.r#type.into(),
            language: self.language.parse().unwrap(),
            start_line: self.start_line as usize,
            end_line: self.end_line as usize,
            code: self.code,
            skeleton_code: self.skeleton_code,
        }
    }
}

#[napi(object)]
#[derive(Clone)]
pub struct Edge {
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

impl From<codegraph::Edge> for Edge {
    fn from(rel: codegraph::Edge) -> Self {
        Self {
            r#type: EdgeType::from(rel.r#type),
            from: Node::from(rel.from),
            to: Node::from(rel.to),
            import: rel.import,
            alias: rel.alias,
        }
    }
}

impl Into<codegraph::Edge> for Edge {
    fn into(self) -> codegraph::Edge {
        codegraph::Edge {
            r#type: self.r#type.into(),
            from: self.from.into(),
            to: self.to.into(),
            import: self.import,
            alias: self.alias,
        }
    }
}

#[napi(object)]
#[derive(Clone)]
pub struct Snippet {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
}

impl From<codegraph::Snippet> for Snippet {
    fn from(s: codegraph::Snippet) -> Self {
        Self {
            path: s.path,
            start_line: s.start_line as u32,
            end_line: s.end_line as u32,
            content: s.content,
        }
    }
}

#[napi(object)]
#[derive(Clone, Debug)]
pub struct Config {
    /// Whether to recursively traverse subdirectories (default is true)
    pub recursive: Option<bool>,
    /// Whether to follow symbolic links (default is false)
    pub follow_links: Option<bool>,
    /// Maximum recursion depth, None means no limit (default is 0)
    pub max_depth: Option<u32>,
    /// Whether to continue traversal when encountering errors (default is false)
    pub continue_on_error: Option<bool>,
    /// Ignore patterns following gitignore syntax (default is empty)
    /// Each pattern follows gitignore rules:
    /// - Pattern starting with '!' negates the pattern
    /// - Pattern ending with '/' matches directories only
    /// - Pattern starting with '/' is anchored to root
    /// - '*' matches any sequence of characters except '/'
    /// - '**' matches any sequence of characters including '/'
    /// - '?' matches any single character
    /// - '[abc]' matches any character in brackets
    pub ignore_patterns: Option<Vec<String>>,
    /// Whether to use .gitignore files found in directories (default is true)
    pub use_gitignore_files: Option<bool>,
}

impl Into<codegraph::Config> for Config {
    fn into(self) -> codegraph::Config {
        let mut cfg = codegraph::Config::default();
        if let Some(recursive) = self.recursive {
            cfg = cfg.recursive(recursive);
        }
        if let Some(follow_links) = self.follow_links {
            cfg = cfg.follow_links(follow_links);
        }
        if let Some(max_depth) = self.max_depth {
            cfg = cfg.max_depth(max_depth as usize);
        }
        if let Some(continue_on_error) = self.continue_on_error {
            cfg = cfg.continue_on_error(continue_on_error);
        }
        if let Some(ignore_patterns) = self.ignore_patterns {
            cfg = cfg.ignore_patterns(ignore_patterns);
        }
        if let Some(use_gitignore_files) = self.use_gitignore_files {
            cfg = cfg.use_gitignore_files(use_gitignore_files);
        }
        cfg
    }
}
#[napi(object)]
pub struct ParseResult {
    pub nodes: Vec<Node>,
    pub relationships: Vec<Edge>,
}

#[napi]
pub struct CodeGraph {
    db_path: String,
    repo_path: String,
    config: Config,

    graph: codegraph::CodeGraph,
}

/// All the interfaces of CodeGraph are synchronous. To avoid block the overall execution
/// of the Node.js application, it's recommended to call these interfaces in a separate thread
/// by leveraging Node.js's worker threads.
///
/// Note that we did not use `napi::bindgen_prelude::AsyncTask` because it relies on libuv threads,
/// which do not support Rust's lifetime. As a result, it cannot meet the restriction that Kuzu can
/// only have one read-write database instance.
///
/// References:
/// - https://napi.rs/docs/concepts/async-task
/// - https://docs.kuzudb.com/concurrency
#[napi]
impl CodeGraph {
    // Args:
    // db_path: Path of the indexing database to use.
    //
    // Example:
    //
    // ```javascript
    // import * as codegraph from '@codegraph-js/codegraph'
    // let config = {};
    // let graph = new codegraph.Parser("path/to/db", "/path/to/repo", config);
    // ```
    #[napi(constructor)]
    pub fn new(db_path: String, repo_path: String, config: Config) -> Self {
        Self {
            db_path: db_path.clone(),
            repo_path: repo_path.clone(),
            config: config.clone(),
            graph: codegraph::CodeGraph::new(
                PathBuf::from(db_path),
                PathBuf::from(repo_path),
                config.into(),
            ),
        }
    }

    #[napi]
    pub fn index(&mut self, path: String, force: bool) -> napi::Result<()> {
        let result = self.graph.index(PathBuf::from(path.clone()), force);
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(napi::Error::from_reason(format!("Indexing failed: {}", e))),
        }
    }

    #[napi]
    pub fn index_dirty_file(&mut self, path: String, content: String) -> napi::Result<()> {
        let result = self
            .graph
            .index_dirty_file(PathBuf::from(path.clone()), &content.into_bytes());
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(napi::Error::from_reason(format!("Indexing failed: {}", e))),
        }
    }

    #[napi]
    pub fn get_func_param_types(
        &mut self,
        file_path: String,
        line: u32,
    ) -> napi::Result<Vec<Snippet>> {
        let result = self.graph.get_func_param_types(file_path, line as usize);
        match result {
            Ok(snippets) => {
                let js_snippets = snippets.into_iter().map(Snippet::from).collect::<Vec<_>>();
                Ok(js_snippets)
            }
            Err(e) => Err(napi::Error::from_reason(format!(
                "Failed to get function parameter types: {}",
                e
            ))),
        }
    }

    #[napi]
    pub fn clean(&mut self, del: bool) -> napi::Result<()> {
        match self.graph.clean(del) {
            Ok(_) => Ok(()),
            Err(e) => Err(napi::Error::from_reason(format!("Cleaning failed: {}", e))),
        }
    }
}

#[napi(string_enum)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Into<log::LevelFilter> for LogLevel {
    fn into(self) -> log::LevelFilter {
        match self {
            LogLevel::Error => log::LevelFilter::Error,
            LogLevel::Warn => log::LevelFilter::Warn,
            LogLevel::Info => log::LevelFilter::Info,
            LogLevel::Debug => log::LevelFilter::Debug,
            LogLevel::Trace => log::LevelFilter::Trace,
        }
    }
}

#[napi]
pub fn init_logger(log_level: LogLevel) {
    let mut builder = env_logger::Builder::new();
    // Set the log level
    builder.filter_level(log_level.into());
    // Use try_init to avoid panicking if the logger is already initialized.
    let _ = builder.try_init();
}

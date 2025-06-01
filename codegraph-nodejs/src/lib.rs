use codegraph;
use napi_derive::napi;
use std::path::PathBuf;

#[napi(string_enum)]
pub enum NodeType {
    Unparsed,
    Directory,
    File,
    Class,
    Function,
}

impl From<codegraph::NodeType> for NodeType {
    fn from(r#type: codegraph::NodeType) -> Self {
        match r#type {
            codegraph::NodeType::Unparsed => NodeType::Unparsed,
            codegraph::NodeType::Directory => NodeType::Directory,
            codegraph::NodeType::File => NodeType::File,
            codegraph::NodeType::Class => NodeType::Class,
            codegraph::NodeType::Function => NodeType::Function,
        }
    }
}

impl Into<codegraph::NodeType> for NodeType {
    fn into(self) -> codegraph::NodeType {
        match self {
            NodeType::Unparsed => codegraph::NodeType::Unparsed,
            NodeType::Directory => codegraph::NodeType::Directory,
            NodeType::File => codegraph::NodeType::File,
            NodeType::Class => codegraph::NodeType::Class,
            NodeType::Function => codegraph::NodeType::Function,
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
    pub short_names: Vec<String>,
    // Node type
    pub r#type: NodeType,
    /// Start line (0-based)
    pub start_line: u32,
    /// End line (0-based)
    pub end_line: u32,
    /// The code text
    pub code: String,
}

impl From<codegraph::Node> for Node {
    fn from(n: codegraph::Node) -> Self {
        // 先获取需要借用的数据，避免所有权冲突
        let short_names = n.short_names().clone();
        Self {
            name: n.name,
            short_names,
            r#type: NodeType::from(n.r#type),
            start_line: n.start_line as u32,
            end_line: n.end_line as u32,
            code: n.code,
        }
    }
}

impl Into<codegraph::Node> for Node {
    fn into(self) -> codegraph::Node {
        codegraph::Node {
            name: self.name,
            r#type: self.r#type.into(),
            start_line: self.start_line as usize,
            end_line: self.end_line as usize,
            code: self.code,
        }
    }
}

#[napi(object)]
#[derive(Clone)]
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

impl From<codegraph::Relationship> for Relationship {
    fn from(rel: codegraph::Relationship) -> Self {
        Self {
            r#type: EdgeType::from(rel.r#type),
            from: Node::from(rel.from),
            to: Node::from(rel.to),
            import: rel.import,
            alias: rel.alias,
        }
    }
}

impl Into<codegraph::Relationship> for Relationship {
    fn into(self) -> codegraph::Relationship {
        codegraph::Relationship {
            r#type: self.r#type.into(),
            from: self.from.into(),
            to: self.to.into(),
            import: self.import,
            alias: self.alias,
        }
    }
}

#[napi(object)]
#[derive(Clone, Debug)]
pub struct ParserConfig {
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
    /// Output directory for saving parsed nodes as JSON file (default is None)
    /// If specified, the parsed nodes will be written to a JSON file in this directory
    pub out_dir: Option<String>,
}

impl Into<codegraph::ParserConfig> for ParserConfig {
    fn into(self) -> codegraph::ParserConfig {
        let mut cfg = codegraph::ParserConfig::default();
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
        if let Some(out_dir) = self.out_dir {
            cfg = cfg.out_dir(out_dir);
        }
        cfg
    }
}
#[napi(object)]
pub struct ParseResult {
    pub nodes: Vec<Node>,
    pub relationships: Vec<Relationship>,
}

#[napi]
pub struct Parser {
    parser: codegraph::Parser,
}

#[napi]
impl Parser {
    // Args:
    // db_path: Path of the indexing database to use.
    //
    // Example:
    //
    // ```javascript
    // import * as codegraph from '@codegraph-js/codegraph'
    // let config = {};
    // let graph = new codegraph.Parser(config);
    // ```
    #[napi(constructor)]
    pub fn new(config: ParserConfig) -> Self {
        Self {
            parser: codegraph::Parser::new(config.into()),
        }
    }

    #[napi]
    pub fn parse(&mut self, dir_path: String) -> napi::Result<ParseResult> {
        let result = self.parser.parse(PathBuf::from(dir_path));
        if let Ok((nodes, relationships)) = result {
            let js_nodes = nodes.into_iter().map(Node::from).collect::<Vec<_>>();
            let js_relationships = relationships
                .into_iter()
                .map(Relationship::from)
                .collect::<Vec<_>>();
            Ok(ParseResult {
                nodes: js_nodes,
                relationships: js_relationships,
            })
        } else {
            Ok(ParseResult {
                nodes: vec![],
                relationships: vec![],
            })
        }
    }
}

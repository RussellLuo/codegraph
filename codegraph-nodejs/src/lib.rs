use codegraph;
use napi_derive::napi;

#[napi]
pub enum NodeType{
    Unparsed,
    Class,
}

impl From<codegraph::NodeType> for NodeType {
    fn from(typ: codegraph::NodeType) -> Self {
        match typ {
            codegraph::NodeType::UNPARSED => NodeType::Unparsed,
            codegraph::NodeType::CLASS => NodeType::Class,
        }
    }
}

#[napi]
#[derive(Clone)]
pub struct Node {
    /// File path
    pub name: String,
    // Node type
    pub typ: NodeType,
    /// Start line (0-based)
    pub start_line: u32,
    /// End line (0-based)
    pub end_line: u32,
    /// The code text
    pub code: String,
}

impl From<codegraph::Node> for Node {
    fn from(n: codegraph::Node) -> Self {
        Self {
            name: n.name,
            typ: NodeType::from(n.typ),
            start_line: n.start_line as u32,
            end_line: n.end_line as u32,
            code: n.code,
        }
    }
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
    // import * as codegraph from '@codegraph/codegraph'
    // let graph = new codegraph.Parser();
    // ```
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            parser: codegraph::Parser::new(),
        }
    }

    #[napi]
    pub fn parse(&mut self, repo_path: String, source_path: String) -> napi::Result<Vec<Node>> {
        let nodes = self.parser.parse(repo_path.as_str(), source_path.as_str());
        let py_nodes = nodes.unwrap()
            .into_iter()
            .map(|n| Node::from(n))
            .collect::<Vec<_>>();
        Ok(py_nodes)
    }
}
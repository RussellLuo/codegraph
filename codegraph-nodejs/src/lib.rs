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
pub struct CodeGraph {
    parser: codegraph::Parser,
}

#[napi]
impl CodeGraph {
    // Args:
    // db_path: Path of the indexing database to use.
    //
    // Example:
    //
    // ```javascript
    // import * as codegraph from '@codegraph/codegraph'
    // let graph = new codegraph.CodeGraph('./graph/db');
    // ```
    #[napi(constructor)]
    pub fn new(db_path: String) -> Self {
        Self {
            parser: codegraph::Parser::new(db_path.as_str()),
        }
    }

    #[napi]
    pub fn index(&mut self, repo_path: String, source_path: String) -> napi::Result<()> {
        self.parser.index(repo_path.as_str(), source_path.as_str());
        Ok(())
    }

    #[napi]
    pub fn clean(&mut self, delete: bool) -> napi::Result<()> {
        self.parser.clean();
        Ok(())
    }

    #[napi]
    pub fn query(&mut self, stmt: String) -> napi::Result<Vec<Node>> {
        let nodes = self.parser.query(stmt.as_str()).unwrap();
        let py_nodes = nodes
            .into_iter()
            .map(|n| Node::from(n))
            .collect::<Vec<_>>();
        Ok(py_nodes)
    }
}
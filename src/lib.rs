use pathdiff;
use std::path::PathBuf;

mod db;
mod parser;
mod types;
mod util;

pub use db::Database;
pub use parser::{Parser, ParserConfig};
pub use types::{Edge, EdgeType, Language, Node, NodeType, Relationship};

pub type Config = ParserConfig;

#[derive(Debug)]
pub struct Snippet {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
}

pub struct CodeGraph {
    db: Database,
    parser: Parser,
    repo_path: PathBuf,
}

impl CodeGraph {
    pub fn new(db_path: PathBuf, repo_path: PathBuf, config: Config) -> Self {
        Self {
            db: Database::new(db_path),
            parser: Parser::new(repo_path.clone(), config),
            repo_path: repo_path,
        }
    }

    pub fn index(&mut self, paths: Vec<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
        // Only parse the first path for now.
        let (nodes, relationships) = self.parser.parse(paths[0].clone())?;
        self.parser.save(&mut self.db)?;
        Ok(())
    }

    pub fn query(&mut self, stmt: String) -> Result<Vec<Node>, Box<dyn std::error::Error>> {
        return self.db.query_nodes(stmt.as_str());
    }

    pub fn get_func_param_types(
        &mut self,
        file_path: String,
        line: usize,
    ) -> Result<Vec<Snippet>, Box<dyn std::error::Error>> {
        let mut snippets: Vec<Snippet> = Vec::new();

        // Make file_path a relative path to the repo_path.
        let file_path = pathdiff::diff_paths(&file_path, &self.repo_path)
            .unwrap_or(PathBuf::from(&file_path))
            .to_string_lossy()
            .to_string();

        let stmt = format!(
            r#"
MATCH (file {{ name: "{}" }})
MATCH (file)-[:CONTAINS*1..2]->(func)
MATCH (func)-[:REFERENCES]->(typ)
WHERE func.start_line < {} AND func.end_line > {}
OPTIONAL MATCH (typ)-[r:CONTAINS]->(meth)
RETURN file.name, typ.start_line, typ.end_line, typ.code, COLLECT(meth.skeleton_code) AS methods;
        "#,
            file_path, line, line
        );
        if let Some(result) = self.db.query(stmt.as_str())? {
            for row in result {
                let path = match &row[0] {
                    kuzu::Value::String(path) => path.clone(),
                    _ => "".to_string(),
                };
                let start_line = match &row[1] {
                    kuzu::Value::UInt32(line) => *line as usize,
                    _ => 0,
                };
                let end_line = match &row[2] {
                    kuzu::Value::UInt32(line) => *line as usize,
                    _ => 0,
                };

                let mut content = String::new();
                match &row[3] {
                    kuzu::Value::String(type_code) => {
                        content.push_str(type_code.as_str());
                    }
                    _ => {}
                }
                match &row[4] {
                    kuzu::Value::List(_, methods) => {
                        for meth in methods {
                            match meth {
                                kuzu::Value::String(meth_skeleton_code) => {
                                    content.push_str("\n\n");
                                    content.push_str(meth_skeleton_code.as_str());
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                snippets.push(Snippet {
                    path,
                    start_line,
                    end_line,
                    content,
                });
            }
        };

        Ok(snippets)
    }

    /// Clean the database.
    /// If `delete` is true, the database directory will be deleted. Otherwise, the database will be cleaned up.
    pub fn clean(&mut self, delete: bool) -> Result<(), Box<dyn std::error::Error>> {
        if !delete {
            self.db.clean()?;
            return Ok(());
        }

        // Delete the database directory.
        if self.db.db_path.exists() {
            std::fs::remove_dir_all(&self.db.db_path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn test_index() {
        //let manifest_dir = env!("CARGO_MANIFEST_DIR");
        //let dir_path = PathBuf::from(manifest_dir)
        //    .join("examples")
        //    .join("go")
        //    .join("demo");
        let dir_path =
            PathBuf::from("/Users/russellluo/Projects/work/opencsg/projects/starhub-server");
        let db_path = dir_path.join("kuzu_db");

        let config = Config::default().ignore_patterns(vec!["*".to_string(), "!*.go".to_string()]);
        let mut graph = CodeGraph::new(db_path, dir_path.clone(), config);
        match graph.index(vec![dir_path]) {
            Err(e) => {
                println!("Failed to index: {:?}", e);
            }
            Ok(_) => {}
        }
        let result = graph.query("MATCH (n) RETURN *".to_string());
        match result {
            Ok(nodes) => {
                /*for node in nodes {
                    println!("Query Node: {:?}", node);
                }*/
            }
            Err(e) => {
                println!("Failed to query: {:?}", e);
            }
        }
        //let _ = graph.clean(true);
    }

    #[test]
    fn test_get_func_param_types() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let dir_path = PathBuf::from(manifest_dir)
            .join("examples")
            .join("go")
            .join("demo");
        let db_path = dir_path.join("kuzu_db");

        let config = Config::default().ignore_patterns(vec!["*".to_string(), "!*.go".to_string()]);
        let mut graph = CodeGraph::new(db_path, dir_path.clone(), config);
        //let file_path = "/Users/russellluo/Projects/work/opencsg/projects/starhub-server/builder/store/database/mirror.go".to_string();
        let file_path = "main.go".to_string();
        let line = 50;
        let result = graph.get_func_param_types(file_path, line);
        match result {
            Ok(snippets) => {
                for s in snippets {
                    println!("Snippet: {:?}", s);
                }
            }
            Err(e) => {
                println!("Failed to query: {:?}", e);
            }
        }
    }
}

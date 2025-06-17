use log;
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

    /// Index the given paths into the database.
    ///
    /// If `force` is true, the existing files will be re-indexed.
    pub fn index(&mut self, path: PathBuf, force: bool) -> Result<(), Box<dyn std::error::Error>> {
        if path == self.repo_path {
            // Try to index the root directory of the repository.
            // We assume that there are many files in the repository, so we need to
            // use the Kuzu's `COPY FROM` command (i.e. batch insert) for better performance.

            if force {
                // Since the `COPY FROM` command does not support deleting existing nodes,
                // we need to delete the existing nodes manually.
                self.clean(true)?;
            }

            let (nodes, relationships) = self.parser.parse(path.clone())?;
            self.db.bulk_insert_nodes_via_csv(&nodes)?;
            self.db.bulk_insert_relationships_via_csv(&relationships)?;

            // TODO: needs improvement.
            let type_rels = self.parser.resolve_func_param_type_relationships(
                &self.parser.nodes,
                &self.parser.func_param_types,
                &mut self.db,
            )?;
            self.db.bulk_insert_relationships_via_csv(&type_rels)?;

            return Ok(());
        }

        // Otherwise, we assume that the given path is a single file or a small directory.
        // We use the Kuzu's `MERGE` command to upsert (i.e. insert or update) the nodes.
        if path.is_file() {
            let rel_file_path = path
                .strip_prefix(self.repo_path.clone())
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            // find all existing nodes related to the file.
            let stmt = format!(
                r#"
MATCH (file)-[:CONTAINS*1..2]->(def)
WHERE file.name = "{}"
RETURN def;
"#,
                &rel_file_path,
            );
            let old_nodes = self.db.query_nodes(stmt.as_str())?;

            let (file_node, nodes, rels, func_param_types) = self.parser.parse_file(&path)?;

            // Delete outdated nodes.
            // Find nodes that exist in old_nodes but not in nodes (outdated nodes to be deleted)
            let node_names_to_delete: Vec<String> = old_nodes
                .clone()
                .into_iter()
                .filter(|old_node| !nodes.contains_key(&old_node.name))
                .map(|old_node| old_node.name)
                .collect();
            self.db.delete_nodes(&node_names_to_delete)?;

            // Delete all out-going relationships from the current file node and old nodes.
            let mut node_names_for_rel_deletion = vec![rel_file_path.clone()];
            node_names_for_rel_deletion
                .extend(old_nodes.clone().into_iter().map(|node| node.name.clone()));
            // Convert node names to a string array for the query. e.g. ["file1", "node1", "node2"]
            let node_names_array = format!(
                "[{}]",
                node_names_for_rel_deletion
                    .iter()
                    .map(|name| format!("{:?}", name))
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let stmt = format!(
                r#"
MATCH (a)-[e]->()
WHERE a.name IN {}
DELETE e;
"#,
                &node_names_array,
            );
            log::debug!("delete out-going relationships: {}", stmt);
            let _ = self.db.query(stmt.as_str())?;

            // Upsert the file node first.
            self.db.upsert_nodes(&vec![file_node])?;

            // Upsert the rest of the nodes and relationships.
            let vec_nodes: Vec<Node> = nodes.values().cloned().collect();
            self.db.upsert_nodes(&vec_nodes)?;
            self.db.upsert_relationships(&rels)?;

            // TODO: needs improvement.
            let type_rels = self.parser.resolve_func_param_type_relationships(
                &nodes,
                &func_param_types.unwrap(),
                &mut self.db,
            )?;

            if log::log_enabled!(log::Level::Debug) {
                for r in &type_rels {
                    log::debug!("type_rel: {}-[{}]{}", r.from.name, r.r#type, r.to.name);
                }
            }

            self.db.upsert_relationships(&type_rels)?;
        } else if path.is_dir() {
            return Err("Not supported yet".into());
        } else {
            return Err(format!(
                "{:?} does not exist or is neither a file nor directory",
                path
            )
            .into());
        }

        Ok(())
    }

    pub fn query_nodes(&mut self, stmt: String) -> Result<Vec<Node>, Box<dyn std::error::Error>> {
        return self.db.query_nodes(stmt.as_str());
    }

    pub fn query_relationships(
        &mut self,
        stmt: String,
    ) -> Result<Vec<Relationship>, Box<dyn std::error::Error>> {
        return self.db.query_relationships(stmt.as_str());
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
RETURN typ.name, typ.start_line, typ.end_line, typ.code, COLLECT(meth.skeleton_code) AS methods;
        "#,
            file_path, line, line
        );
        if let Some(result) = self.db.query(stmt.as_str())? {
            for row in result {
                let path = match &row[0] {
                    kuzu::Value::String(path) => {
                        let parts: Vec<&str> = path.split(':').collect();
                        parts[0].clone().to_string()
                    }
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
    ///
    /// TODO: support clean specific files or directories.
    /// - `clean(path: PathBuf)`
    /// - `clean(path: PathBuf, delete: bool)`
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
    use log::LevelFilter;
    use std::path::{Path, PathBuf};

    fn init() {
        let _ = env_logger::builder()
            .filter_level(LevelFilter::Info)
            .is_test(true)
            .try_init();
    }

    #[test]
    fn test_index() {
        init();

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let dir_path = PathBuf::from(manifest_dir)
            .join("examples")
            .join("go")
            .join("demo");
        let db_path = dir_path.join("kuzu_db");

        let config = Config::default().ignore_patterns(vec![
            "*".to_string(),
            "!main.go".to_string(),
            "!types.go".to_string(),
        ]);
        let mut graph = CodeGraph::new(db_path, dir_path.clone(), config);

        graph.clean(true).unwrap();
        graph.index(dir_path, false).unwrap();

        let existing_nodes = graph.query_nodes("MATCH (n) RETURN n".to_string()).unwrap();
        assert_eq!(existing_nodes.len(), 11);
        graph.clean(true).unwrap();
    }

    #[test]
    fn test_upsert_file() {
        init();

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let repo_path = PathBuf::from(manifest_dir)
            .join("examples")
            .join("go")
            .join("demo");
        let db_path = repo_path.join("kuzu_db");

        let config = Config::default().ignore_patterns(vec![
            "*".to_string(),
            "!main.go".to_string(),
            "!types.go".to_string(),
        ]);
        let mut graph = CodeGraph::new(db_path, repo_path.clone(), config);

        // 1.1 initial index
        graph.clean(true).unwrap();
        graph.index(repo_path.clone(), true).unwrap();

        // 1.2 assert data
        let final_nodes = graph.query_nodes("MATCH (n) RETURN n".to_string()).unwrap();
        let final_rels = graph
            .query_relationships("MATCH (a)-[e]->(b) RETURN a.name, b.name, e".to_string())
            .unwrap();
        let mut node_strings: Vec<_> = final_nodes.into_iter().map(|n| n.name).collect();
        let mut rel_strings: Vec<_> = final_rels
            .into_iter()
            .map(|r| format!("{}-[{}]->{}", r.from.name, r.r#type, r.to.name))
            .collect();

        node_strings.sort();
        rel_strings.sort();

        assert_eq!(
            node_strings,
            [
                ".",
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
                ".-[contains]->main.go",
                ".-[contains]->types.go",
                "main.go-[contains]->main.go:User",
                "main.go-[contains]->main.go:main",
                "main.go:User-[contains]->main.go:User.DisplayInfo",
                "main.go:User-[contains]->main.go:User.NewUser",
                "main.go:User-[contains]->main.go:User.SetAddress",
                "main.go:User-[contains]->main.go:User.UpdateEmail",
                "main.go:User.SetAddress-[references]->types.go:Address",
                "main.go:User.SetAddress-[references]->types.go:Hobby",
                "types.go-[contains]->types.go:Address",
                "types.go-[contains]->types.go:Hobby"
            ],
        );

        // 2.1 upsert `types.go`
        let types_go_path = repo_path
            .clone()
            .join("types.go")
            .to_string_lossy()
            .to_string();
        let modified_file_path = repo_path
            .clone()
            .join("diff")
            .join("modified_types.go")
            .to_string_lossy()
            .to_string();
        let _ = duct::cmd!("cp", modified_file_path, types_go_path.clone())
            .read()
            .unwrap();

        graph
            .index(repo_path.clone().join("types.go"), true)
            .unwrap();

        // 2.2 assert data
        let final_nodes = graph.query_nodes("MATCH (n) RETURN n".to_string()).unwrap();
        let final_rels = graph
            .query_relationships("MATCH (a)-[e]->(b) RETURN a.name, b.name, e".to_string())
            .unwrap();
        let mut node_strings: Vec<_> = final_nodes.into_iter().map(|n| n.name).collect();
        let mut rel_strings: Vec<_> = final_rels
            .into_iter()
            .map(|r| format!("{}-[{}]->{}", r.from.name, r.r#type, r.to.name))
            .collect();

        node_strings.sort();
        rel_strings.sort();

        assert_eq!(
            node_strings,
            [
                ".",
                "main.go",
                "main.go:User",
                "main.go:User.DisplayInfo",
                "main.go:User.NewUser",
                "main.go:User.SetAddress",
                "main.go:User.UpdateEmail",
                "main.go:main",
                "types.go",
                "types.go:Address2",
                "types.go:Hobby"
            ]
        );
        assert_eq!(
            rel_strings,
            [
                ".-[contains]->main.go",
                ".-[contains]->types.go",
                "main.go-[contains]->main.go:User",
                "main.go-[contains]->main.go:main",
                "main.go:User-[contains]->main.go:User.DisplayInfo",
                "main.go:User-[contains]->main.go:User.NewUser",
                "main.go:User-[contains]->main.go:User.SetAddress",
                "main.go:User-[contains]->main.go:User.UpdateEmail",
                "main.go:User.SetAddress-[references]->types.go:Hobby",
                "types.go-[contains]->types.go:Address2",
                "types.go-[contains]->types.go:Hobby"
            ],
        );

        // 3. clean up (revert `types.go`)
        graph.clean(true).unwrap();

        let original_file_path = repo_path
            .clone()
            .join("diff")
            .join("original_types.go")
            .to_string_lossy()
            .to_string();
        let _ = duct::cmd!("cp", original_file_path, types_go_path.clone())
            .read()
            .unwrap();
    }

    #[test]
    fn test_get_func_param_types() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let dir_path = PathBuf::from(manifest_dir)
            .join("examples")
            .join("go")
            .join("demo");
        let db_path = dir_path.join("kuzu_db");

        let config = Config::default().ignore_patterns(vec![
            "*".to_string(),
            "!main.go".to_string(),
            "!types.go".to_string(),
        ]);
        let mut graph = CodeGraph::new(db_path, dir_path.clone(), config);
        graph.index(dir_path, false).unwrap();

        let file_path = "main.go".to_string();
        let line = 37; // SetAddress()
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

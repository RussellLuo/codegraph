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

/// The tree-sitter definition query source for Python.
pub const PYTHON_DEFINITIONS_QUERY_SOURCE: &str = include_str!("queries/python-definitions.scm");

pub struct Parser {
    repo_path: PathBuf,
}

impl Parser {
    pub fn new(repo_path: PathBuf) -> Self {
        Self { repo_path }
    }

    pub fn parse(
        &self,
        file_node: &Node,
        file_path: &PathBuf,
    ) -> Result<(IndexMap<String, Node>, Vec<Edge>), Box<dyn std::error::Error>> {
        let query_source = PYTHON_DEFINITIONS_QUERY_SOURCE.to_string();
        let mut nodes: IndexMap<String, Node> = IndexMap::new();
        let mut edges: Vec<Edge> = Vec::new();

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
                                    .strip_prefix(&self.repo_path)
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

                        let edge = Edge {
                            r#type: EdgeType::Contains,
                            from: file_node.clone(),
                            to: node.clone(),
                            import: None,
                            alias: None,
                        };
                        edges.push(edge);
                    }
                }
                "definition.class" => {
                    cur_class_node = Some(capture.node);
                }
                _ => {}
            }
        }
        Ok((nodes, edges))
    }
}

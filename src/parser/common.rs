use crate::{Edge, EdgeType, Language, Node, NodeType};
use std::path::Path;
use std::path::PathBuf;
use strum_macros;
use tree_sitter;

/// Tree-sitter query patterns.
#[derive(Debug, Clone, PartialEq, Eq, strum_macros::FromRepr)]
pub enum QueryPattern {
    Import,
    Interface,
    Class,
    Function,
    Method,
}

pub fn parse_simple_interface(
    query: &tree_sitter::Query,
    mat: &tree_sitter::QueryMatch,
    repo_path: &PathBuf,
    file_node: &Node,
    file_path: &PathBuf,
    source_code: &[u8],
) -> Option<Node> {
    let mut current_node: Option<Node> = None;

    for capture in mat.captures {
        let capture_name = query.capture_names()[capture.index as usize];
        let capture_node_text: String = capture
            .node
            .utf8_text(&source_code)
            .unwrap_or("")
            .to_string();
        log_capture(&capture, capture_name, &capture_node_text);

        match capture_name {
            "definition.interface" => {
                current_node = Some(Node {
                    name: "".to_string(), // fill in later
                    r#type: NodeType::Interface,
                    language: file_node.language.clone(),
                    start_line: capture.node.start_position().row,
                    end_line: capture.node.end_position().row,
                    code: capture_node_text,
                    skeleton_code: String::new(),
                });
            }
            "definition.interface.name" => {
                if let Some(curr_node) = &mut current_node {
                    curr_node.name = format!(
                        "{}:{}",
                        Path::new(file_path)
                            .strip_prefix(repo_path)
                            .unwrap_or_else(|_| Path::new(file_path))
                            .to_string_lossy(),
                        capture_node_text
                    );
                }
            }
            _ => {}
        }
    }

    return current_node;
}

pub fn parse_simple_class(
    query: &tree_sitter::Query,
    mat: &tree_sitter::QueryMatch,
    repo_path: &PathBuf,
    file_node: &Node,
    file_path: &PathBuf,
    source_code: &[u8],
) -> Option<Node> {
    let mut current_node: Option<Node> = None;

    for capture in mat.captures {
        let capture_name = query.capture_names()[capture.index as usize];
        let capture_node_text: String = capture
            .node
            .utf8_text(&source_code)
            .unwrap_or("")
            .to_string();
        log_capture(&capture, capture_name, &capture_node_text);

        match capture_name {
            "definition.class" => {
                current_node = Some(Node {
                    name: "".to_string(), // fill in later
                    r#type: NodeType::Class,
                    language: file_node.language.clone(),
                    start_line: capture.node.start_position().row,
                    end_line: capture.node.end_position().row,
                    code: capture_node_text,
                    skeleton_code: String::new(),
                });
            }
            "definition.class.name" => {
                if let Some(curr_node) = &mut current_node {
                    curr_node.name = format!(
                        "{}:{}",
                        Path::new(file_path)
                            .strip_prefix(repo_path)
                            .unwrap_or_else(|_| Path::new(file_path))
                            .to_string_lossy(),
                        capture_node_text
                    );
                }
            }
            _ => {}
        }
    }

    return current_node;
}

pub fn log_capture(
    capture: &tree_sitter::QueryCapture,
    capture_name: &str,
    capture_node_text: &String,
) {
    let start = capture.node.start_position();
    let end = capture.node.end_position();
    log::trace!(
        "[CAPTURE]\nname: {capture_name}, start: {start}, end: {end}, text: {:?}, capture: {:?}",
        capture_node_text,
        capture.node.to_sexp()
    );
}

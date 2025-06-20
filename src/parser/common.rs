use crate::{Edge, EdgeType, Language, Node, NodeType};
use std::path::Path;
use std::path::PathBuf;
use tree_sitter;

/// A pending import relationship that needs to be resolved as an edge.
#[derive(Debug, Clone)]
pub struct PendingImport {
    pub language: Language,
    // The path of the source (imported) module
    pub source_path: String,
    // None if the entire source module is imported
    // - TypeScript: Some<"export default"> if the default export is imported
    pub symbol: Option<String>,
    pub alias: Option<String>,
}

impl PendingImport {
    pub fn import_name(&self) -> String {
        if let Some(alias) = &self.alias {
            alias.clone()
        } else if let Some(symbol) = &self.symbol {
            symbol.clone()
        } else {
            unreachable!()
        }
    }
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

pub fn parse_simple_enum(
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
            "definition.enum" => {
                current_node = Some(Node {
                    name: "".to_string(), // fill in later
                    r#type: NodeType::OtherType,
                    language: file_node.language.clone(),
                    start_line: capture.node.start_position().row,
                    end_line: capture.node.end_position().row,
                    code: capture_node_text,
                    skeleton_code: String::new(),
                });
            }
            "definition.enum.name" => {
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

pub fn parse_simple_type_alias(
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
            "definition.type_alias" => {
                current_node = Some(Node {
                    name: "".to_string(), // fill in later
                    r#type: NodeType::OtherType,
                    language: file_node.language.clone(),
                    start_line: capture.node.start_position().row,
                    end_line: capture.node.end_position().row,
                    code: capture_node_text,
                    skeleton_code: String::new(),
                });
            }
            "definition.type_alias.name" => {
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

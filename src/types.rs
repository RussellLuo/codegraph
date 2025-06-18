use indexmap::IndexMap;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use strum_macros;

#[derive(
    Debug, Clone, PartialEq, Eq, strum_macros::EnumString, strum_macros::Display, serde::Serialize,
)]
pub enum NodeType {
    #[strum(serialize = "unparsed")]
    Unparsed,
    #[strum(serialize = "directory")]
    Directory,
    #[strum(serialize = "file")]
    File,
    #[strum(serialize = "interface")]
    Interface,
    #[strum(serialize = "class")]
    Class,
    #[strum(serialize = "function")]
    Function,
}

#[derive(Debug, Clone, strum_macros::Display, strum_macros::EnumString, serde::Serialize)]
pub enum EdgeType {
    #[strum(serialize = "contains")]
    Contains,
    #[strum(serialize = "imports")]
    Imports,
    #[strum(serialize = "inherits")]
    Inherits,
    #[strum(serialize = "references")]
    References,
}

#[derive(Debug, Clone, strum_macros::Display, strum_macros::EnumString, serde::Serialize)]
pub enum Language {
    Text,
    Python,
    Go,
    // TypeScript,
    // JavaScript,
}

impl Language {
    pub fn from_path(path: &str) -> Self {
        let ext = Path::new(path).extension().and_then(|e| e.to_str());

        match ext {
            Some("py") => Language::Python,
            Some("go") => Language::Go,
            // Some("ts") => Language::TypeScript,
            // Some("js") => Language::JavaScript,
            _ => Language::Text,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    /// File path
    pub name: String,
    // Node type
    pub r#type: NodeType,
    // Language type
    pub language: Language,
    /// Start line (0-based)
    pub start_line: usize,
    /// End line (0-based)
    pub end_line: usize,
    /// The code text
    pub code: String,
    /// The skeleton code text
    pub skeleton_code: String,
}

impl Node {
    pub fn from_type_and_name(r#type: NodeType, name: String) -> Self {
        Self {
            name,
            r#type,
            language: Language::Text,
            start_line: 0,
            end_line: 0,
            code: String::new(),
            skeleton_code: String::new(),
        }
    }

    pub fn from_dict(data: &HashMap<String, serde_json::Value>) -> Self {
        Self {
            name: data.get("name").unwrap().as_str().unwrap().to_string(),
            r#type: match data.get("type").unwrap().as_str().unwrap() {
                "Unparsed" => NodeType::Unparsed,
                "Class" => NodeType::Class,
                _ => NodeType::Unparsed, // 默认值
            },
            language: data
                .get("lanuage")
                .unwrap()
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
            start_line: data.get("start_line").unwrap().as_u64().unwrap() as usize,
            end_line: data.get("end_line").unwrap().as_u64().unwrap() as usize,
            code: data
                .get("code")
                .map(|v| v.as_str().unwrap().to_string())
                .unwrap_or_default(),
            skeleton_code: data
                .get("skeleton_code")
                .map(|v| v.as_str().unwrap().to_string())
                .unwrap_or_default(),
        }
    }

    pub fn short_name(&self) -> String {
        fn make_names(name: &str) -> Vec<String> {
            let lower = name.to_lowercase();
            if lower != name {
                vec![name.to_string(), lower]
            } else {
                vec![name.to_string()]
            }
        }

        if !self.name.contains(':') {
            // "src/a.py" => a
            let file_name = self.name.rsplit('/').next().unwrap_or(&self.name.as_str());
            make_names(file_name).last().unwrap().to_string()
        } else {
            // "src/a.py:A" => A, a
            let attr_name = self.name.rsplit(':').next().unwrap_or(self.name.as_str());
            if !attr_name.contains('.') {
                make_names(attr_name).last().unwrap().to_string()
            } else {
                // "src/a.py:A.meth" => meth
                let sub_attr_name = attr_name.rsplit('.').next().unwrap_or(attr_name);
                make_names(sub_attr_name).last().unwrap().to_string()
            }
        }
    }

    /// 将Node转换为字典格式，包含基本字段和short_names字段
    ///
    /// Due to the limitation of kuzu CSV import,
    /// we must keep the keys (i.e. the CSV header) in the same order as the database schema fields.
    pub fn to_dict(&self) -> IndexMap<String, serde_json::Value> {
        let mut dict = IndexMap::new();

        // 添加基本字段
        dict.insert(
            "name".to_string(),
            serde_json::Value::String(self.name.clone()),
        );
        dict.insert(
            "type".to_string(),
            serde_json::Value::String(self.r#type.to_string()),
        );

        /*
        // 添加short_names字段
        let short_names: Vec<serde_json::Value> = self
            .short_names()
            .into_iter()
            .map(|name| serde_json::Value::String(name))
            .collect();
        dict.insert(
            "short_names".to_string(),
            serde_json::Value::Array(short_names),
        );
        */
        dict.insert(
            "short_name".to_string(),
            serde_json::Value::String(self.short_name().clone()),
        );

        match self.r#type {
            NodeType::Unparsed | NodeType::Directory => {
                // 对于Unparsed和Directory类型，不需要start_line和end_line
            }
            NodeType::File => {
                dict.insert(
                    "language".to_string(),
                    serde_json::Value::String(self.language.to_string()),
                );
                dict.insert(
                    "code".to_string(),
                    serde_json::Value::String(self.code.clone()),
                );
                dict.insert(
                    "skeleton_code".to_string(),
                    serde_json::Value::String(self.skeleton_code.clone()),
                );
            }
            NodeType::Interface | NodeType::Class | NodeType::Function => {
                dict.insert(
                    "language".to_string(),
                    serde_json::Value::String(self.language.to_string()),
                );
                dict.insert(
                    "code".to_string(),
                    serde_json::Value::String(self.code.clone()),
                );
                dict.insert(
                    "skeleton_code".to_string(),
                    serde_json::Value::String(self.skeleton_code.clone()),
                );
                dict.insert(
                    "start_line".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(self.start_line)),
                );
                dict.insert(
                    "end_line".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(self.end_line)),
                );
            }
        }

        dict
    }
}

#[derive(Debug, Clone, Serialize)]
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

impl Edge {
    /// 从字典数据创建关系
    pub fn from_dict(
        data: &HashMap<String, serde_json::Value>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let type_str = data
            .get("_label")
            .and_then(|v| v.as_str())
            .ok_or("Missing _label field")?;
        let edge_type = type_str.parse::<EdgeType>()?;

        let type_field = data
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or("Missing type field")?;
        let parts: Vec<&str> = type_field.split('_').collect();
        if parts.len() != 2 {
            return Err("Invalid type format".into());
        }

        let from_type = parts[0].parse::<NodeType>()?;
        let to_type = parts[1].parse::<NodeType>()?;

        let from_node = Node {
            name: String::new(),
            r#type: from_type,
            language: Language::Text,
            start_line: 0,
            end_line: 0,
            code: String::new(),
            skeleton_code: String::from(""),
        };

        let to_node = Node {
            name: String::new(),
            r#type: to_type,
            language: Language::Text,
            start_line: 0,
            end_line: 0,
            code: String::new(),
            skeleton_code: String::from(""),
        };

        let import = data
            .get("import")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let alias = data
            .get("alias")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(Edge {
            r#type: edge_type,
            from: from_node,
            to: to_node,
            import,
            alias,
        })
    }

    /// 转换为字典格式
    ///
    /// Due to the limitation of kuzu CSV import,
    /// we must keep the keys (i.e. the CSV header) in the same order as the database schema fields.
    pub fn to_dict(&self) -> IndexMap<String, serde_json::Value> {
        let mut dict = IndexMap::new();

        dict.insert(
            "from".to_string(),
            serde_json::Value::String(self.from.name.clone()),
        );
        dict.insert(
            "to".to_string(),
            serde_json::Value::String(self.to.name.clone()),
        );
        dict.insert(
            "type".to_string(),
            serde_json::Value::String(self.from_to()),
        );

        match self.r#type {
            EdgeType::Imports => {
                let import_value = if let Some(ref import) = self.import {
                    serde_json::Value::String(import.clone())
                } else {
                    // For compatibility with the kuzu CSV format.
                    serde_json::Value::Null
                };
                dict.insert("import".to_string(), import_value);

                let alias_value = if let Some(ref alias) = self.alias {
                    serde_json::Value::String(alias.clone())
                } else {
                    // For compatibility with the kuzu CSV format.
                    serde_json::Value::Null
                };
                dict.insert("alias".to_string(), alias_value);
            }
            _ => {}
        }

        dict
    }

    /// 获取from_to字符串表示
    pub fn from_to(&self) -> String {
        format!(
            "{}_{}",
            self.from.r#type.to_string().to_lowercase(),
            self.to.r#type.to_string().to_lowercase()
        )
    }
}

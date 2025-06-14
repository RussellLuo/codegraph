use crate::{EdgeType, Language, Node, NodeType, Relationship};
use indexmap::IndexMap;
use kuzu;
use log;
use serde_json;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use tempfile;

// The database schema.
pub const CREATE_DATABASE_SCHEMA: &str = include_str!("schema.cypher");

pub struct Database {
    pub db_path: PathBuf,
    initialized: bool,
    db: Option<kuzu::Database>,
}

impl Database {
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            initialized: false,
            db_path: db_path,
            db: None,
        }
    }

    fn init(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.initialized {
            return Ok(());
        }

        let db = kuzu::Database::new(&self.db_path, kuzu::SystemConfig::default())?;
        self.db = Some(db);

        // 创建连接并初始化数据库模式
        if let Some(db) = &self.db {
            let conn = kuzu::Connection::new(db)?;
            conn.query(CREATE_DATABASE_SCHEMA)?;

            // install and load the JSON extension for bulk insertion.
            //conn.query("INSTALL json")?;
            //conn.query("LOAD json")?;
        }

        self.initialized = true;
        Ok(())
    }

    /// 将解析的节点按类型分组写入JSON文件
    fn write_nodes_to_json(
        &self,
        nodes: &[Node],
        out_dir: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 按节点类型分组
        let mut grouped_nodes: HashMap<String, Vec<IndexMap<String, serde_json::Value>>> =
            HashMap::new();

        for node in nodes {
            let type_key = node.r#type.to_string();
            let node_dict = node.to_dict();
            grouped_nodes
                .entry(type_key)
                .or_insert_with(Vec::new)
                .push(node_dict);
        }

        // 为每个节点类型创建单独的JSON文件
        for (node_type, type_nodes) in grouped_nodes {
            let json_filename = format!("{}.json", node_type);
            let json_path = PathBuf::from(out_dir).join(json_filename);

            // 将该类型的节点序列化为JSON
            let json_content = serde_json::to_string_pretty(&type_nodes)?;
            // 写入文件
            std::fs::write(&json_path, json_content)?;
            /*println!(
                "已写入 {} 个 {} 类型的节点到文件: {}",
                type_nodes.len(),
                node_type,
                json_path.display()
            );*/
        }

        Ok(())
    }

    /// 将解析的关系按类型分组写入JSON文件
    fn write_relationships_to_json(
        &self,
        relationships: &[Relationship],
        out_dir: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 确保输出目录存在
        std::fs::create_dir_all(out_dir)?;

        // 按关系类型分组，使用 to_dict() 转换为字典格式
        let mut grouped_relationships: HashMap<String, Vec<IndexMap<String, serde_json::Value>>> =
            HashMap::new();

        for relationship in relationships {
            let key = format!(
                "{}_{}_{}.json",
                relationship.r#type.to_string(),
                relationship.from.r#type.to_string(),
                relationship.to.r#type.to_string()
            );
            let relationship_dict = relationship.to_dict();
            grouped_relationships
                .entry(key)
                .or_insert_with(Vec::new)
                .push(relationship_dict);
        }

        // 为每个关系类型创建单独的JSON文件
        for (key, type_relationships) in grouped_relationships {
            let json_filename = &key;
            let json_path = PathBuf::from(out_dir).join(json_filename);

            // 将该类型的关系序列化为JSON（现在使用 to_dict() 的结果）
            let json_content = serde_json::to_string_pretty(&type_relationships)?;
            // 写入文件
            std::fs::write(&json_path, json_content)?;
            /*println!(
                "已写入 {} 个 {} 类型的关系到文件: {}",
                type_relationships.len(),
                key,
                json_path.display()
            );*/
        }

        Ok(())
    }

    /// 将解析的节点按类型分组写入CSV文件
    fn write_nodes_to_csv(
        &self,
        nodes: &[Node],
        out_dir: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 确保输出目录存在
        std::fs::create_dir_all(out_dir)?;

        // 按节点类型分组
        let mut grouped_nodes: HashMap<String, Vec<IndexMap<String, serde_json::Value>>> =
            HashMap::new();
        for node in nodes {
            let node_dict = node.to_dict();
            grouped_nodes
                .entry(node.r#type.to_string())
                .or_insert_with(Vec::new)
                .push(node_dict);
        }

        // 为每个节点类型创建单独的CSV文件
        for (node_type, type_nodes) in grouped_nodes {
            let csv_filename = format!("{}.csv", node_type);
            let csv_path = PathBuf::from(out_dir).join(csv_filename);

            // 创建CSV writer
            let mut writer = csv::Writer::from_path(&csv_path)?;

            // 收集所有可能的字段名（使用第一个节点的字典键）
            let field_names: Vec<String> = if let Some(first_node) = type_nodes.first() {
                first_node.keys().map(|k| k.to_string()).collect()
            } else {
                continue; // 跳过空节点组
            };

            // 写入CSV头
            writer.write_record(&field_names)?;

            // 写入每个节点的数据
            for node_dict in type_nodes {
                let mut record = Vec::new();
                for field in &field_names {
                    let value = node_dict.get(field).unwrap_or(&serde_json::Value::Null);
                    record.push(match value {
                        serde_json::Value::String(s) => {
                            if field == "name" && s.is_empty() {
                                // Kuzu CSV does not support using empty strings as primary keys, use a placeholder "." instead.
                                ".".to_string()
                            } else {
                                s.clone()
                            }
                        }
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Array(a) => serde_json::to_string(a).unwrap_or_default(),
                        serde_json::Value::Object(_) => {
                            serde_json::to_string(value).unwrap_or_default()
                        }
                        serde_json::Value::Null => String::new(),
                    });
                }
                //println!("record: {:?}", record);
                writer.write_record(&record)?;
            }

            // 确保所有数据写入文件
            writer.flush()?;
        }

        Ok(())
    }

    /// 将解析的关系按类型分组写入CSV文件
    fn write_relationships_to_csv(
        &self,
        relationships: &[Relationship],
        out_dir: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 确保输出目录存在
        std::fs::create_dir_all(out_dir)?;

        // 按关系类型分组
        let mut grouped_relationships: HashMap<String, Vec<IndexMap<String, serde_json::Value>>> =
            HashMap::new();
        for relationship in relationships {
            let key = format!(
                "{}_{}_{}",
                relationship.r#type.to_string(),
                relationship.from.r#type.to_string(),
                relationship.to.r#type.to_string()
            );
            let relationship_dict = relationship.to_dict();
            grouped_relationships
                .entry(key)
                .or_insert_with(Vec::new)
                .push(relationship_dict);
        }

        // 为每个关系类型创建单独的CSV文件
        for (key, type_relationships) in grouped_relationships {
            let csv_filename = format!("{}.csv", key);
            let csv_path = PathBuf::from(out_dir).join(csv_filename);

            // 创建CSV writer
            let mut writer = csv::Writer::from_path(&csv_path)?;

            // 收集所有可能的字段名（使用第一个关系的字典键）
            let field_names: Vec<String> = if let Some(first_rel) = type_relationships.first() {
                first_rel.keys().map(|k| k.to_string()).collect()
            } else {
                continue; // 跳过空关系组
            };

            // 写入CSV头
            writer.write_record(&field_names)?;

            // 写入每个关系的数据
            for rel_dict in type_relationships {
                let mut record = Vec::new();
                for field in &field_names {
                    let value = rel_dict.get(field).unwrap_or(&serde_json::Value::Null);
                    record.push(match value {
                        serde_json::Value::String(s) => {
                            if ["from", "to"].contains(&field.as_str()) && s.is_empty() {
                                // Kuzu CSV does not support using empty strings as node primary keys, use a placeholder "." instead.
                                ".".to_string()
                            } else {
                                s.clone()
                            }
                        }
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Array(a) => serde_json::to_string(a).unwrap_or_default(),
                        serde_json::Value::Object(_) => {
                            serde_json::to_string(value).unwrap_or_default()
                        }
                        serde_json::Value::Null => String::new(),
                    });
                }
                writer.write_record(&record)?;
            }

            // 确保所有数据写入文件
            writer.flush()?;
        }

        Ok(())
    }

    pub fn bulk_insert_nodes(
        &mut self,
        nodes: &Vec<Node>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.init()?;

        let temp_dir = tempfile::tempdir()?;
        let temp_dir_path = temp_dir.path();
        log::debug!(
            "save {} nodes in temp_dir: {:?}",
            nodes.len(),
            temp_dir_path
        );
        log::info!("bulk-insert {} nodes", nodes.len());
        self.write_nodes_to_json(nodes, &temp_dir_path)?;

        if let Some(db) = &self.db {
            let conn = kuzu::Connection::new(db)?;

            let node_files = std::fs::read_dir(&temp_dir_path)?;
            for entry in node_files {
                let entry = entry?;
                let file_path = entry.path();

                if let Some(extension) = file_path.extension() {
                    if extension == "json" {
                        let file_stem = file_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .ok_or("Invalid file name")?;

                        // Capitalize first letter of filename for table name
                        let table_name = format!(
                            "{}{}",
                            file_stem.chars().next().unwrap().to_uppercase(),
                            &file_stem[1..]
                        );

                        let query = format!(r#"COPY {} FROM {:?}"#, table_name, file_path);
                        conn.query(query.as_str())?;
                    }
                }
            }
        }

        temp_dir.close()?;

        Ok(())
    }

    pub fn bulk_insert_nodes_via_csv(
        &mut self,
        nodes: &Vec<Node>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.init()?;

        let temp_dir = tempfile::tempdir()?;
        let temp_dir_path = temp_dir.path();
        log::debug!(
            "save {} nodes in temp_dir: {:?}",
            nodes.len(),
            temp_dir_path
        );
        log::info!("bulk-insert {} nodes", nodes.len());
        self.write_nodes_to_csv(nodes, &temp_dir_path)?;

        if let Some(db) = &self.db {
            let conn = kuzu::Connection::new(db)?;

            let node_files = std::fs::read_dir(&temp_dir_path)?;
            for entry in node_files {
                let entry = entry?;
                let file_path = entry.path();

                if let Some(extension) = file_path.extension() {
                    if extension == "csv" {
                        let file_stem = file_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .ok_or("Invalid file name")?;

                        // Capitalize first letter of filename for table name
                        let table_name = format!(
                            "{}{}",
                            file_stem.chars().next().unwrap().to_uppercase(),
                            &file_stem[1..]
                        );

                        // Quoted newlines are not supported in parallel CSV reader, thus we have to specify PARALLEL=FALSE in the options.
                        let query = format!(
                            r#"COPY {} FROM {:?} (HEADER=true, PARALLEL=false)"#,
                            table_name, file_path
                        );
                        conn.query(query.as_str())?;
                    }
                }
            }
        }

        temp_dir.close()?;

        Ok(())
    }

    pub fn bulk_insert_relationships(
        &mut self,
        relationships: &Vec<Relationship>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.init()?;

        let temp_dir = tempfile::tempdir()?;
        let temp_dir_path = temp_dir.path();
        log::debug!(
            "save {} relationships in temp_dir: {:?}",
            relationships.len(),
            temp_dir_path
        );
        log::info!("bulk-insert {} relationships", relationships.len());
        self.write_relationships_to_json(relationships, &temp_dir_path)?;

        if let Some(db) = &self.db {
            let conn = kuzu::Connection::new(db)?;

            let node_files = std::fs::read_dir(&temp_dir_path)?;
            for entry in node_files {
                let entry = entry?;
                let file_path = entry.path();

                if let Some(extension) = file_path.extension() {
                    if extension == "json" {
                        let file_stem = file_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .ok_or("Invalid file name")?;

                        let parts: Vec<&str> = file_stem.split('_').collect();
                        if parts.len() != 3 {
                            return Err(format!(
                                "Invalid filename format for relationships file: {}",
                                file_stem
                            )
                            .into());
                        }

                        let table_name = parts[0].to_uppercase();
                        let from_type = to_title_case(parts[1]);
                        let to_type = to_title_case(parts[2]);

                        let query = format!(
                            r#"COPY {} FROM {:?} (from={:?}, to={:?})"#,
                            table_name, file_path, from_type, to_type
                        );
                        match conn.query(query.as_str()) {
                            Err(e) => {
                                log::error!("Failed to copy file {} :{}", file_path.display(), e);
                                log::error!("Error query: {}", query);
                            }
                            Ok(_) => {}
                        }
                    }
                }
            }
        }

        temp_dir.close()?;

        Ok(())
    }

    /// 批量通过CSV文件导入关系数据
    pub fn bulk_insert_relationships_via_csv(
        &mut self,
        relationships: &Vec<Relationship>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.init()?;

        let temp_dir = tempfile::tempdir()?;
        let temp_dir_path = temp_dir.path();
        log::debug!(
            "save {} relationships in temp_dir: {:?}",
            relationships.len(),
            temp_dir_path
        );
        log::info!("bulk-insert {} relationships", relationships.len());
        self.write_relationships_to_csv(relationships, &temp_dir_path)?;

        if let Some(db) = &self.db {
            let conn = kuzu::Connection::new(db)?;

            let node_files = std::fs::read_dir(&temp_dir_path)?;
            for entry in node_files {
                let entry = entry?;
                let file_path = entry.path();

                if let Some(extension) = file_path.extension() {
                    if extension == "csv" {
                        let file_stem = file_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .ok_or("Invalid file name")?;

                        let parts: Vec<&str> = file_stem.split('_').collect();
                        if parts.len() != 3 {
                            return Err(format!(
                                "Invalid filename format for relationships file: {}",
                                file_stem
                            )
                            .into());
                        }

                        let table_name = parts[0].to_uppercase();
                        let from_type = to_title_case(parts[1]);
                        let to_type = to_title_case(parts[2]);

                        // Quoted newlines are not supported in parallel CSV reader, thus we have to specify PARALLEL=FALSE in the options.
                        let query = format!(
                            r#"COPY {} FROM {:?} (from={:?}, to={:?}, HEADER=true, PARALLEL=false)"#,
                            table_name, file_path, from_type, to_type
                        );
                        match conn.query(query.as_str()) {
                            Err(e) => {
                                println!("Failed to copy file {} :{}", file_path.display(), e);
                                println!("Error query: {}", query);
                            }
                            Ok(_) => {}
                        }
                    }
                }
            }
        }

        temp_dir.close()?;

        Ok(())
    }

    fn to_merge_data(
        m: &IndexMap<String, serde_json::Value>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // 将 HashMap 中的键值对转换为 Cypher 查询中的键值对字符串
        let mut parts = Vec::new();

        for (key, value) in m {
            let formatted_value = match value {
                serde_json::Value::String(s) => string_repr(s), //repr_string(s),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Array(_) => serde_json::to_string(value)?,
                serde_json::Value::Object(_) => serde_json::to_string(value)?,
                serde_json::Value::Null => "null".to_string(),
            };
            parts.push(format!("{}: {}", key, formatted_value));
        }

        Ok(parts.join(", "))
    }

    fn to_set_data(
        tag: &str,
        pk: &str,
        m: &IndexMap<String, serde_json::Value>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // 将 HashMap 中的键值对转换为 Cypher 查询中的键值对字符串
        let mut parts = Vec::new();

        for (key, value) in m {
            let formatted_value = match value {
                serde_json::Value::String(s) => string_repr(s), //repr_string(s),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Array(_) => serde_json::to_string(value)?,
                serde_json::Value::Object(_) => serde_json::to_string(value)?,
                serde_json::Value::Null => "null".to_string(),
            };
            // Ignore primary key to avoid errors:
            //
            // Runtime exception: Found duplicated primary key value '<pk>',
            // which violates the uniqueness constraint of the primary key column.
            if key != pk {
                parts.push(format!("{}.{} = {}", tag, key, formatted_value));
            }
        }

        Ok(parts.join(", "))
    }

    pub fn upsert_nodes(&mut self, nodes: &Vec<Node>) -> Result<(), Box<dyn std::error::Error>> {
        self.init()?;

        log::info!("upsert {} nodes", nodes.len());

        // 每次需要连接时创建新的连接，避免生命周期问题
        if let Some(db) = &self.db {
            let conn = kuzu::Connection::new(db)?;

            for node in nodes {
                let table_name = to_title_case(node.r#type.to_string().as_str());
                let node_dict = node.to_dict();
                let set_data = Self::to_set_data(&"n", &"name", &node_dict)?;
                let query = format!(
                    r#"
MERGE (n:{} {{ name: "{}" }})
ON CREATE SET {}
ON MATCH SET {}
"#,
                    table_name, node.name, set_data, set_data
                );
                log::debug!("upsert_nodes query: {}", query);
                conn.query(query.as_str())?;
            }
        }

        Ok(())
    }

    pub fn upsert_relationships(
        &mut self,
        rels: &Vec<Relationship>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.init()?;

        log::info!("upsert {} relationships", rels.len());

        // 每次需要连接时创建新的连接，避免生命周期问题
        if let Some(db) = &self.db {
            let conn = kuzu::Connection::new(db)?;

            for rel in rels {
                let table_name = rel.r#type.to_string().to_ascii_uppercase();
                let _from_to = rel.from_to();
                let from_to = _from_to.split('_').collect::<Vec<&str>>();
                let from_node_table_name = to_title_case(from_to[0]);
                let to_node_table_name = to_title_case(from_to[1]);
                let rel_dict = rel
                    .to_dict()
                    .iter()
                    .filter(|(k, _)| *k != "from" && *k != "to")
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let set_data = Self::to_set_data(&"e", &"", &rel_dict)?;
                let query = format!(
                    r#"
MATCH (a:{}), (b:{})
WHERE a.name = '{}' AND b.name = '{}'
MERGE (a)-[e:{}]->(b)
ON CREATE SET {}
ON MATCH SET {}
                "#,
                    from_node_table_name,
                    to_node_table_name,
                    rel.from.name,
                    rel.to.name,
                    table_name,
                    set_data,
                    set_data,
                );
                log::debug!("upsert_relationships query: {}", query);
                conn.query(&query)?;
            }
        }

        Ok(())
    }

    pub fn query(
        &mut self,
        stmt: &str,
    ) -> Result<Option<kuzu::QueryResult>, Box<dyn std::error::Error>> {
        self.init()?;

        if let Some(db) = &self.db {
            let conn = kuzu::Connection::new(db)?;
            let result = conn.query(stmt)?;
            return Ok(Some(result));
        }

        Ok(None)
    }

    pub fn query_nodes(&mut self, stmt: &str) -> Result<Vec<Node>, Box<dyn std::error::Error>> {
        self.init()?;

        let mut nodes: Vec<Node> = vec![];

        if let Some(db) = &self.db {
            let conn = kuzu::Connection::new(db)?;
            let result = conn.query(stmt)?;
            for row in result {
                match &row[0] {
                    kuzu::Value::Node(node) => {
                        let props = node.get_properties();
                        let mut node = Node::from_type_and_name(NodeType::Unparsed, "".to_string());
                        for (prop_name, prop_value) in props {
                            match prop_name.as_str() {
                                "name" => {
                                    node.name = prop_value.to_string();
                                }
                                "type" => {
                                    node.r#type = prop_value
                                        .to_string()
                                        .parse()
                                        .unwrap_or(NodeType::Unparsed);
                                }
                                "language" => {
                                    node.language =
                                        prop_value.to_string().parse().unwrap_or(Language::Text);
                                }
                                "code" => {
                                    node.code = prop_value.to_string();
                                }
                                "start_line" => {
                                    node.start_line = prop_value.to_string().parse().unwrap_or(0);
                                }
                                "end_line" => {
                                    node.end_line = prop_value.to_string().parse().unwrap_or(0);
                                }
                                _ => {}
                            }
                        }
                        /*
                        if let kuzu::Value::String(name) = &props[0].1 {
                            node.name = name.to_string();
                        }
                        if let kuzu::Value::String(typ) = &props[1].1 {
                            node.r#type = typ.parse().unwrap();
                        }
                        if let kuzu::Value::String(lang) = &props[3].1 {
                            node.language = lang.parse().unwrap_or(Language::Text);
                        }
                        if let kuzu::Value::String(code) = &props[4].1 {
                            node.code = code.to_string();
                        }
                        if let kuzu::Value::UInt32(line) = &props[5].1 {
                            node.start_line = *line as usize;
                        }
                        if let kuzu::Value::UInt32(line) = &props[6].1 {
                            node.end_line = *line as usize;
                        }
                        */
                        nodes.push(node);
                    }
                    _ => println!("Unrecoginized node type"),
                }
            }
        }
        Ok(nodes)
    }

    pub fn query_relationships(
        &mut self,
        stmt: &str,
    ) -> Result<Vec<Relationship>, Box<dyn std::error::Error>> {
        self.init()?;

        let mut relationships: Vec<Relationship> = vec![];

        if let Some(db) = &self.db {
            let conn = kuzu::Connection::new(db)?;
            let result = conn.query(stmt)?;
            for row in result {
                let from_node_name = match &row[0] {
                    kuzu::Value::String(name) => name.clone(),
                    _ => "".to_string(),
                };
                let to_node_name = match &row[1] {
                    kuzu::Value::String(name) => name.clone(),
                    _ => "".to_string(),
                };
                match &row[2] {
                    kuzu::Value::Rel(rel) => {
                        let props = rel.get_properties();

                        let mut typ: String = "".to_string();
                        let mut import: Option<String> = None;
                        let mut alias: Option<String> = None;
                        for (prop_name, prop_value) in props {
                            match prop_name.as_str() {
                                "type" => {
                                    typ = prop_value.to_string();
                                }
                                "import" => {
                                    import = Some(prop_value.to_string());
                                }
                                "alias" => {
                                    alias = Some(prop_value.to_string());
                                }
                                _ => {}
                            }
                        }
                        /*
                        let typ: String = if let kuzu::Value::String(typ) = &props[0].1 {
                            typ.to_string()
                        } else {
                            "".to_string()
                        };
                        let import = if let kuzu::Value::String(import) = &props[1].1 {
                            Some(import)
                        } else {
                            None
                        };
                        let alias = if let kuzu::Value::String(alias) = &props[2].1 {
                            Some(alias)
                        } else {
                            None
                        };
                        */

                        let parts: Vec<&str> = typ.split('_').collect();
                        if parts.len() != 2 {
                            return Err(format!("Invalid relationship type: {}", typ).into());
                        }

                        let from_node_type: NodeType = parts[0].parse().unwrap();
                        let to_node_type: NodeType = parts[1].parse().unwrap();

                        // 获取关系类型
                        let rel_type = rel
                            .get_label_name()
                            .to_lowercase()
                            .parse()
                            .unwrap_or(EdgeType::Contains);

                        let relationship = Relationship {
                            r#type: rel_type,
                            from: Node::from_type_and_name(from_node_type, from_node_name),
                            to: Node::from_type_and_name(to_node_type, to_node_name),
                            import: import,
                            alias: alias,
                        };

                        relationships.push(relationship);
                    }
                    _ => println!("无法识别的关系类型"),
                }
            }
        }
        Ok(relationships)
    }

    pub fn delete_nodes(&mut self, names: &Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
        if names.is_empty() {
            return Ok(());
        }

        self.init()?;

        if let Some(db) = &self.db {
            let conn = kuzu::Connection::new(db)?;

            // Delete nodes and all of their relationships
            // see https://docs.kuzudb.com/cypher/data-manipulation-clauses/delete/#detach-delete.
            let query = format!("MATCH (n) WHERE n.name IN {:?} DETACH DELETE n", &names,);
            conn.query(&query)?;
        }

        Ok(())
    }

    pub fn clean(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(db) = &self.db {
            let conn = kuzu::Connection::new(db)?;
            // Delete all records
            let _ = conn.query("MATCH (n) DETACH DELETE n")?;
        }
        Ok(())
    }
}

fn repr_string(s: &str) -> String {
    // 添加引号，同时保留原始字符串内容
    //format!("{:?}", s)
    serde_json::to_string(s)
        .unwrap()
        .replace("\\n", "\n") // 把转义的 \n 替换回实际换行符
        .replace("\\t", "\t") // 同样处理制表符
        .replace("\\r", "\r") // 同样处理回车符
}

fn string_repr(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 2);
    result.push('"');

    for c in s.chars() {
        match c {
            '\"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\n"),
            '\r' => result.push_str("\r"),
            '\t' => result.push_str("\t"),
            '\0' => result.push_str("\\0"),
            '\x08' => result.push_str("\\b"), // 退格
            '\x0C' => result.push_str("\\f"), // 换页
            c if c.is_ascii_control() => {
                // 转义其他 ASCII 控制字符 (0-31)
                result.push_str(&format!("\\x{:02x}", c as u32));
            }
            _ => result.push(c), // 普通字符直接保留
        }
    }

    result.push('"');
    result
}

fn to_title_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;

    for c in s.chars() {
        if c.is_whitespace() || c.is_ascii_punctuation() {
            result.push(c);
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.extend(c.to_lowercase());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_query() {}

    #[test]
    fn test_query_relationships() {
        let nodes = vec![
            Node::from_type_and_name(NodeType::File, "file1".to_string()),
            Node::from_type_and_name(NodeType::Function, "func1".to_string()),
        ];
        let rels = vec![Relationship {
            r#type: EdgeType::Contains,
            from: Node::from_type_and_name(NodeType::File, "file1".to_string()),
            to: Node::from_type_and_name(NodeType::Function, "func1".to_string()),
            import: None,
            alias: None,
        }];
        let mut db = Database::new(PathBuf::from("db"));
        db.upsert_nodes(&nodes).unwrap();
        db.upsert_relationships(&rels).unwrap();

        let existing_rels = db
            .query_relationships("MATCH (a)-[e]->(b) RETURN a.name, b.name, e")
            .unwrap();
        let mut relStrings: Vec<_> = existing_rels
            .into_iter()
            .map(|r| format!("{}-[{}]->{}", r.from.name, r.r#type, r.to.name))
            .collect();
        assert_eq!(relStrings, ["file1-[contains]->func1"],);

        db.clean().unwrap();
    }

    #[test]
    fn test_delete_nodes() {
        let nodes = vec![Node {
            name: "Node1".to_string(),
            r#type: NodeType::Function,
            language: Language::Go,
            code: "func Node1() {\n    fmt.Println(\"Hello, World!\")\n}".to_string(),
            skeleton_code: "func Node1() {}".to_string(),
            start_line: 1,
            end_line: 1,
        }];
        let mut db = Database::new(PathBuf::from("test.db"));

        db.upsert_nodes(&nodes).unwrap();
        let mut existing_nodes = db.query_nodes("MATCH (n) RETURN n").unwrap();
        assert_eq!(existing_nodes.len(), 1);

        db.delete_nodes(&vec!["Node1".to_string()]).unwrap();
        existing_nodes = db.query_nodes("MATCH (n) RETURN n").unwrap();
        assert_eq!(existing_nodes.len(), 0);

        db.clean().unwrap();
    }

    #[test]
    fn test_write_nodes_to_csv() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let temp_out_dir = PathBuf::from(manifest_dir)
            .join("examples")
            .join("go")
            .join("demo")
            .join("temp_out_dir");
        let nodes = vec![Node {
            name: "Node1".to_string(),
            r#type: NodeType::Function,
            language: Language::Go,
            code: "func Node1() {\n    fmt.Println(\"Hello, World!\")\n}".to_string(),
            skeleton_code: "func Node1() {}".to_string(),
            start_line: 1,
            end_line: 1,
        }];
        let db = Database::new(PathBuf::from("test.db"));
        match db.write_nodes_to_csv(&nodes, &temp_out_dir) {
            Ok(_) => println!("Nodes written to CSV successfully."),
            Err(e) => println!("Error writing nodes to CSV: {}", e),
        }
    }
}

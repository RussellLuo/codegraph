use duct;
use regex::Regex;
use std::fs::read_to_string;
use std::path::PathBuf;

pub fn get_repo_module_file_path(
    repo_path: &PathBuf,
    repo_mod_path: &String,
    mod_import_path: &String,
) -> Option<PathBuf> {
    // Remove quotes and module path prefix.
    let rel_mod_path = mod_import_path.strip_prefix(repo_mod_path)?;

    // Remove leading slash if present
    let rel_mod_path = rel_mod_path.strip_prefix('/').unwrap_or(rel_mod_path);

    // Build cross-platform file path
    let mut result_path = repo_path.clone();
    for component in rel_mod_path.split('/') {
        if !component.is_empty() {
            result_path = result_path.join(component);
        }
    }

    Some(result_path)
}

pub fn get_go_repo_module_path(repo_path: &PathBuf) -> Option<String> {
    let go_mod_path = repo_path.join("go.mod");
    if !go_mod_path.exists() {
        return None;
    }

    let go_mod = read_to_string(go_mod_path).ok()?;
    let re = Regex::new(r"^module\s+(.+)").ok()?;

    re.captures(&go_mod)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_string())
}

/// 判断是否为 Go 语言的基础类型
///
/// # Arguments
/// * `type_name` - 类型名称
///
/// # Returns
/// * `bool` - 如果是基础类型返回 true，否则返回 false
pub fn is_go_builtin_type(type_name: &str) -> bool {
    match type_name {
        // 基础数据类型
        "bool" | "byte" | "rune" |
        // 整数类型
        "int" | "int8" | "int16" | "int32" | "int64" |
        "uint" | "uint8" | "uint16" | "uint32" | "uint64" | "uintptr" |
        // 浮点类型
        "float32" | "float64" |
        // 复数类型
        "complex64" | "complex128" |
        // 字符串类型
        "string" |
        // 特殊类型
        "error" | "interface{}" | "any" => true,
        _ => false,
    }
}

fn get_go_root() -> Result<String, Box<dyn std::error::Error>> {
    let go_root = duct::cmd!("go", "env", "GOROOT").read()?.trim().to_string();

    Ok(go_root)
}

fn get_go_path() -> Result<String, Box<dyn std::error::Error>> {
    let go_root = duct::cmd!("go", "env", "GOPATH").read()?.trim().to_string();

    Ok(go_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_get_repo_module_file_path() {
        let repo_path = PathBuf::from("/home/user/repo");
        let repo_mod_path = "github.com/user/repo".to_string();
        let mod_import_path = "github.com/user/repo/pkg/module".to_string();
        let expected_path = PathBuf::from("/home/user/repo/pkg/module");
        assert_eq!(
            get_repo_module_file_path(&repo_path, &repo_mod_path, &mod_import_path),
            Some(expected_path)
        );
    }
}

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

/// Load environment variables from .env file if it exists.
/// Returns a HashMap of key-value pairs.
/// If the .env file doesn't exist, returns an empty HashMap (no error).
pub fn load_env_file(dir: &Path) -> Result<HashMap<String, String>> {
    let env_path = dir.join(".env");

    if !env_path.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(&env_path)?;
    let mut vars = HashMap::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse KEY=VALUE
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();

            // Remove quotes from value if present
            let value = if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value[1..value.len() - 1].to_string()
            } else {
                value
            };

            vars.insert(key, value);
        }
    }

    Ok(vars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_load_env_file_exists() {
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "FOO=bar\nBAZ=qux").unwrap();

        let vars = load_env_file(dir.path()).unwrap();
        assert_eq!(vars.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(vars.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn test_load_env_file_missing() {
        let dir = TempDir::new().unwrap();
        let vars = load_env_file(dir.path()).unwrap();
        assert!(vars.is_empty());
    }

    #[test]
    fn test_load_env_with_quotes() {
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "QUOTED=\"hello world\"\nSINGLE='test'").unwrap();

        let vars = load_env_file(dir.path()).unwrap();
        assert_eq!(vars.get("QUOTED"), Some(&"hello world".to_string()));
        assert_eq!(vars.get("SINGLE"), Some(&"test".to_string()));
    }

    #[test]
    fn test_load_env_with_comments() {
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "# Comment\nKEY=value\n# Another comment\nFOO=bar").unwrap();

        let vars = load_env_file(dir.path()).unwrap();
        assert_eq!(vars.len(), 2);
        assert_eq!(vars.get("KEY"), Some(&"value".to_string()));
        assert_eq!(vars.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_load_env_with_empty_lines() {
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "KEY1=value1\n\nKEY2=value2\n\n").unwrap();

        let vars = load_env_file(dir.path()).unwrap();
        assert_eq!(vars.len(), 2);
        assert_eq!(vars.get("KEY1"), Some(&"value1".to_string()));
        assert_eq!(vars.get("KEY2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_load_env_with_spaces() {
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "KEY = value with spaces").unwrap();

        let vars = load_env_file(dir.path()).unwrap();
        assert_eq!(vars.get("KEY"), Some(&"value with spaces".to_string()));
    }
}

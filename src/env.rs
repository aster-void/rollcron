use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Expand shell-like variables in a string.
/// Supports ~ (home directory) and $VAR / ${VAR} (environment variables).
/// Returns the original string if expansion fails.
pub fn expand_string(s: &str) -> String {
    shellexpand::full(s)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| s.to_string())
}

/// Load environment variables from .env file if it exists.
/// Returns a HashMap of key-value pairs.
/// If the .env file doesn't exist, returns an empty HashMap (no error).
#[allow(dead_code)]
pub fn load_env_file(dir: &Path) -> Result<HashMap<String, String>> {
    let env_path = dir.join(".env");
    load_env_from_path(&env_path)
}

/// Load environment variables from a specific .env file path.
/// Returns a HashMap of key-value pairs.
/// If the file doesn't exist, returns an empty HashMap (no error).
pub fn load_env_from_path(path: &PathBuf) -> Result<HashMap<String, String>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let iter = dotenvy::from_path_iter(path)
        .with_context(|| format_env_error(path))?;
    let mut vars = HashMap::new();
    for item in iter {
        let (key, value) = item.with_context(|| format_env_error(path))?;
        vars.insert(key, value);
    }
    Ok(vars)
}

fn format_env_error(path: &Path) -> String {
    format!(
        "failed to parse env file: {}\n\
         hint: values with spaces must be quoted, e.g.:\n\
         PRIVATE_KEY=\"-----BEGIN PRIVATE KEY-----\\nABC\\n-----END PRIVATE KEY-----\"",
        path.display()
    )
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
        fs::write(&env_path, "KEY=\"value with spaces\"").unwrap();

        let vars = load_env_file(dir.path()).unwrap();
        assert_eq!(vars.get("KEY"), Some(&"value with spaces".to_string()));
    }

    #[test]
    fn test_load_env_from_path_exists() {
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join("custom.env");
        fs::write(&env_path, "FOO=bar\nBAZ=qux").unwrap();

        let vars = load_env_from_path(&env_path).unwrap();
        assert_eq!(vars.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(vars.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn test_load_env_from_path_missing() {
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join("missing.env");
        let vars = load_env_from_path(&env_path).unwrap();
        assert!(vars.is_empty());
    }

    #[test]
    fn test_expand_string_tilde() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_string("~/foo"), format!("{}/foo", home));
    }

    #[test]
    fn test_expand_string_env_var() {
        // SAFETY: Test is single-threaded and var is unique
        unsafe { std::env::set_var("TEST_EXPAND_VAR", "hello") };
        assert_eq!(expand_string("$TEST_EXPAND_VAR"), "hello");
        assert_eq!(expand_string("${TEST_EXPAND_VAR}"), "hello");
        unsafe { std::env::remove_var("TEST_EXPAND_VAR") };
    }

    #[test]
    fn test_expand_string_no_expansion() {
        assert_eq!(expand_string("plain text"), "plain text");
        assert_eq!(expand_string("/absolute/path"), "/absolute/path");
    }

    #[test]
    fn test_expand_string_undefined_var() {
        // Undefined variables are kept as-is (no error)
        let result = expand_string("$UNDEFINED_VAR_12345");
        assert!(result.contains("UNDEFINED") || result.is_empty());
    }
}

#[cfg(test)]
mod private_key_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_private_key_double_quoted() {
        // Private keys with spaces (e.g., "PRIVATE KEY") MUST be double-quoted
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(
            &env_path,
            r#"GOOGLE_PRIVATE_KEY="-----BEGIN PRIVATE KEY-----\nMIIEvgIBADANBg\n-----END PRIVATE KEY-----\n""#,
        )
        .unwrap();

        let vars = load_env_file(dir.path()).unwrap();
        let key = vars.get("GOOGLE_PRIVATE_KEY").unwrap();
        // Double quotes expand \n to actual newlines
        assert!(key.contains('\n'));
        assert!(key.contains("-----BEGIN PRIVATE KEY-----"));
    }

    #[test]
    fn test_private_key_unquoted_fails() {
        // Without quotes, spaces in values cause parse errors
        let dir = TempDir::new().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(
            &env_path,
            r#"GOOGLE_PRIVATE_KEY=-----BEGIN PRIVATE KEY-----\nABC"#,
        )
        .unwrap();

        let result = load_env_file(dir.path());
        let err = result.unwrap_err();
        let msg = format!("{:?}", err);
        assert!(msg.contains("hint:"));
        assert!(msg.contains("values with spaces must be quoted"));
    }
}

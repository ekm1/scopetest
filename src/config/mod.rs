use std::path::Path;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const CONFIG_FILE: &str = ".scopetestrc.json";

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config: {0}")]
    ReadError(String),
    #[error("Failed to parse config: {0}")]
    ParseError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default = "default_test_patterns")]
    pub test_patterns: Vec<String>,
    
    #[serde(default = "default_ignore_patterns")]
    pub ignore_patterns: Vec<String>,
    
    #[serde(default = "default_extensions")]
    pub extensions: Vec<String>,
    
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,
    
    #[serde(default = "default_base")]
    pub default_base: String,
    
    #[serde(default = "default_expand_barrels")]
    pub expand_barrels: bool,
}

fn default_test_patterns() -> Vec<String> {
    vec![
        "**/*.spec.ts".to_string(),
        "**/*.spec.tsx".to_string(),
        "**/*.test.ts".to_string(),
        "**/*.test.tsx".to_string(),
        "**/*.spec.js".to_string(),
        "**/*.spec.jsx".to_string(),
        "**/*.test.js".to_string(),
        "**/*.test.jsx".to_string(),
    ]
}

fn default_ignore_patterns() -> Vec<String> {
    vec![
        "**/node_modules/**".to_string(),
        "**/dist/**".to_string(),
        "**/build/**".to_string(),
        "**/.git/**".to_string(),
        "**/coverage/**".to_string(),
    ]
}

fn default_extensions() -> Vec<String> {
    vec![
        ".ts".to_string(),
        ".tsx".to_string(),
        ".js".to_string(),
        ".jsx".to_string(),
        ".mjs".to_string(),
        ".cjs".to_string(),
    ]
}

fn default_cache_enabled() -> bool { true }
fn default_base() -> String { "main".to_string() }
fn default_expand_barrels() -> bool { true }

impl Default for Config {
    fn default() -> Self {
        Self {
            test_patterns: default_test_patterns(),
            ignore_patterns: default_ignore_patterns(),
            extensions: default_extensions(),
            cache_enabled: default_cache_enabled(),
            default_base: default_base(),
            expand_barrels: default_expand_barrels(),
        }
    }
}

impl Config {
    pub fn load(root: &Path) -> Result<Self, ConfigError> {
        let config_path = root.join(CONFIG_FILE);
        
        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| ConfigError::ReadError(e.to_string()))?;

        serde_json::from_str(&content)
            .map_err(|e| ConfigError::ParseError(e.to_string()))
    }

    pub fn is_test_file(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        
        for pattern in &self.test_patterns {
            if let Ok(glob) = glob::Pattern::new(pattern) {
                if glob.matches(&path_str) {
                    return true;
                }
            }
            
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                if filename.contains(".spec.") || filename.contains(".test.") {
                    return true;
                }
            }
        }
        
        false
    }

    pub fn should_ignore(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        
        for pattern in &self.ignore_patterns {
            if let Ok(glob) = glob::Pattern::new(pattern) {
                if glob.matches(&path_str) {
                    return true;
                }
            }
        }
        
        path_str.contains("node_modules")
    }

    pub fn is_supported_extension(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                let ext_with_dot = format!(".{}", ext);
                self.extensions.contains(&ext_with_dot)
            })
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(!config.test_patterns.is_empty());
        assert!(!config.ignore_patterns.is_empty());
        assert!(config.cache_enabled);
    }

    #[test]
    fn test_is_test_file() {
        let config = Config::default();
        
        assert!(config.is_test_file(Path::new("src/foo.spec.ts")));
        assert!(config.is_test_file(Path::new("src/foo.test.tsx")));
        assert!(!config.is_test_file(Path::new("src/foo.ts")));
    }

    #[test]
    fn test_should_ignore() {
        let config = Config::default();
        
        assert!(config.should_ignore(Path::new("node_modules/foo/index.js")));
        assert!(config.should_ignore(Path::new("dist/bundle.js")));
        assert!(!config.should_ignore(Path::new("src/foo.ts")));
    }

    #[test]
    fn test_is_supported_extension() {
        let config = Config::default();
        
        assert!(config.is_supported_extension(Path::new("foo.ts")));
        assert!(config.is_supported_extension(Path::new("foo.tsx")));
        assert!(config.is_supported_extension(Path::new("foo.js")));
        assert!(!config.is_supported_extension(Path::new("foo.css")));
    }
}

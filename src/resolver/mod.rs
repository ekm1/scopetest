use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ResolveError {
    #[error("Could not resolve import: {0}")]
    NotFound(String),
    #[error("Failed to read tsconfig: {0}")]
    ConfigError(String),
}

#[derive(Debug, Clone, Default)]
pub struct ResolverConfig {
    pub base_url: Option<PathBuf>,
    pub paths: HashMap<String, Vec<String>>,
    pub extensions: Vec<String>,
}

pub struct PathResolver {
    config: ResolverConfig,
    root: PathBuf,
}

impl PathResolver {
    pub fn new(root: PathBuf) -> Self {
        Self {
            config: ResolverConfig {
                base_url: None,
                paths: HashMap::new(),
                extensions: vec![
                    ".ts".to_string(),
                    ".tsx".to_string(),
                    ".js".to_string(),
                    ".jsx".to_string(),
                ],
            },
            root,
        }
    }

    pub fn load_tsconfig(&mut self, tsconfig_path: &Path) -> Result<(), ResolveError> {
        let content = std::fs::read_to_string(tsconfig_path)
            .map_err(|e| ResolveError::ConfigError(e.to_string()))?;
        
        let json: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| ResolveError::ConfigError(e.to_string()))?;
        
        if let Some(compiler_options) = json.get("compilerOptions") {
            if let Some(base_url) = compiler_options.get("baseUrl").and_then(|v| v.as_str()) {
                let tsconfig_dir = tsconfig_path.parent().unwrap_or(Path::new("."));
                self.config.base_url = Some(tsconfig_dir.join(base_url));
            }
            
            if let Some(paths) = compiler_options.get("paths").and_then(|v| v.as_object()) {
                for (pattern, targets) in paths {
                    if let Some(targets_arr) = targets.as_array() {
                        let resolved_targets: Vec<String> = targets_arr
                            .iter()
                            .filter_map(|t| t.as_str().map(String::from))
                            .collect();
                        self.config.paths.insert(pattern.clone(), resolved_targets);
                    }
                }
            }
        }
        
        Ok(())
    }

    pub fn resolve(&self, from: &Path, import_path: &str) -> Result<PathBuf, ResolveError> {
        if import_path.starts_with('.') || import_path.starts_with('/') {
            let from_dir = from.parent().unwrap_or(Path::new("."));
            let base_path = from_dir.join(import_path);
            let normalized = self.normalize_path(&base_path);
            return self.resolve_with_extensions(&normalized);
        }

        if let Some(resolved) = self.resolve_alias(import_path) {
            return Ok(resolved);
        }

        if let Some(resolved) = self.resolve_workspace_package(import_path) {
            return Ok(resolved);
        }

        Err(ResolveError::NotFound(format!("External module: {}", import_path)))
    }

    fn resolve_workspace_package(&self, import_path: &str) -> Option<PathBuf> {
        let (package_name, subpath) = self.parse_package_import(import_path);
        let node_modules_path = self.root.join("node_modules").join(&package_name);
        
        if node_modules_path.exists() {
            let real_path = std::fs::canonicalize(&node_modules_path).ok()?;
            let canonical_root = std::fs::canonicalize(&self.root).ok()?;
            
            if !real_path.starts_with(&canonical_root) {
                return None;
            }
            
            let target = if subpath.is_empty() {
                self.resolve_package_entry(&real_path)?
            } else {
                real_path.join(&subpath)
            };
            
            let resolved = self.resolve_with_extensions(&target).ok()?;
            
            if let Ok(canonical_resolved) = std::fs::canonicalize(&resolved) {
                if let Ok(relative) = canonical_resolved.strip_prefix(&canonical_root) {
                    return Some(self.root.join(relative));
                }
                return Some(canonical_resolved);
            }
            
            return Some(resolved);
        }
        
        None
    }

    fn parse_package_import(&self, import_path: &str) -> (String, String) {
        let parts: Vec<&str> = import_path.splitn(3, '/').collect();
        
        if import_path.starts_with('@') && parts.len() >= 2 {
            let package_name = format!("{}/{}", parts[0], parts[1]);
            let subpath = if parts.len() > 2 { parts[2].to_string() } else { String::new() };
            (package_name, subpath)
        } else {
            let package_name = parts[0].to_string();
            let subpath = if parts.len() > 1 { parts[1..].join("/") } else { String::new() };
            (package_name, subpath)
        }
    }

    fn resolve_package_entry(&self, package_path: &Path) -> Option<PathBuf> {
        let package_json = package_path.join("package.json");
        if !package_json.exists() {
            return Some(package_path.to_path_buf());
        }
        
        let content = std::fs::read_to_string(&package_json).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;
        
        let entry_fields = ["source", "main", "module", "types"];
        
        for field in entry_fields {
            if let Some(entry) = json.get(field).and_then(|v| v.as_str()) {
                let entry_path = package_path.join(entry);
                if entry_path.exists() || self.resolve_with_extensions(&entry_path).is_ok() {
                    return Some(entry_path);
                }
            }
        }
        
        let src_index = package_path.join("src").join("index");
        if self.resolve_with_extensions(&src_index).is_ok() {
            return Some(src_index);
        }
        
        Some(package_path.join("index"))
    }

    fn resolve_alias(&self, import_path: &str) -> Option<PathBuf> {
        for (pattern, targets) in &self.config.paths {
            let pattern_base = pattern.trim_end_matches('*');
            
            if import_path.starts_with(pattern_base) {
                let suffix = &import_path[pattern_base.len()..];
                
                for target in targets {
                    let target_base = target.trim_end_matches('*');
                    let base_url = self.config.base_url.as_ref().unwrap_or(&self.root);
                    let resolved_path = base_url.join(target_base).join(suffix);
                    
                    if let Ok(path) = self.resolve_with_extensions(&resolved_path) {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    fn resolve_with_extensions(&self, base_path: &Path) -> Result<PathBuf, ResolveError> {
        if base_path.exists() && base_path.is_file() {
            return Ok(base_path.to_path_buf());
        }

        for ext in &self.config.extensions {
            let with_ext = base_path.with_extension(ext.trim_start_matches('.'));
            if with_ext.exists() {
                return Ok(with_ext);
            }
        }

        if base_path.is_dir() {
            for ext in &self.config.extensions {
                let index = base_path.join(format!("index{}", ext));
                if index.exists() {
                    return Ok(index);
                }
            }
        }

        for ext in &self.config.extensions {
            let index = base_path.join(format!("index{}", ext));
            if index.exists() {
                return Ok(index);
            }
        }

        Err(ResolveError::NotFound(base_path.display().to_string()))
    }

    fn normalize_path(&self, path: &Path) -> PathBuf {
        let mut components = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    if !components.is_empty() {
                        components.pop();
                    }
                }
                std::path::Component::CurDir => {}
                c => components.push(c),
            }
        }
        components.iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn test_resolve_relative_import() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("foo.ts"), "").unwrap();
        fs::write(src.join("bar.ts"), "").unwrap();

        let resolver = PathResolver::new(temp.path().to_path_buf());
        let result = resolver.resolve(&src.join("bar.ts"), "./foo");
        
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("foo.ts"));
    }

    #[test]
    fn test_resolve_index_file() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        let utils = src.join("utils");
        fs::create_dir_all(&utils).unwrap();
        fs::write(utils.join("index.ts"), "").unwrap();
        fs::write(src.join("main.ts"), "").unwrap();

        let resolver = PathResolver::new(temp.path().to_path_buf());
        let result = resolver.resolve(&src.join("main.ts"), "./utils");
        
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("index.ts"));
    }
}


    #[test]
    fn test_resolve_parent_dir_import() {
        use tempfile::TempDir;
        use std::fs;
        
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        let component = src.join("Component");
        let tests = component.join("__tests__");
        fs::create_dir_all(&tests).unwrap();
        
        fs::write(component.join("index.tsx"), "export const Component = () => {};").unwrap();
        fs::write(tests.join("index.spec.tsx"), "import { Component } from '..';").unwrap();

        let resolver = PathResolver::new(temp.path().to_path_buf());
        let result = resolver.resolve(&tests.join("index.spec.tsx"), "..");
        
        assert!(result.is_ok(), "Failed to resolve: {:?}", result);
        let resolved = result.unwrap();
        assert!(resolved.ends_with("index.tsx"), "Expected index.tsx, got {:?}", resolved);
    }

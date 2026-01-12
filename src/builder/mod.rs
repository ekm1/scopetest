use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use rayon::prelude::*;
use ignore::WalkBuilder;
use thiserror::Error;

use crate::config::Config;
use crate::graph::DependencyGraph;
use crate::parser::{self, ImportType};
use crate::resolver::PathResolver;

#[derive(Error, Debug)]
pub enum BuildError {
    #[error("Failed to walk directory: {0}")]
    WalkError(String),
    #[error("Failed to build graph: {0}")]
    GraphError(String),
}

pub struct GraphBuilder {
    root: PathBuf,
    config: Config,
    resolver: PathResolver,
}

impl GraphBuilder {
    pub fn new(root: PathBuf, config: Config) -> Self {
        let mut resolver = PathResolver::new(root.clone());
        
        let tsconfig_path = root.join("tsconfig.json");
        if tsconfig_path.exists() {
            let _ = resolver.load_tsconfig(&tsconfig_path);
        }

        Self { root, config, resolver }
    }

    pub fn build(&self) -> Result<DependencyGraph, BuildError> {
        let files = self.discover_files()?;
        let graph = Arc::new(Mutex::new(DependencyGraph::new()));
        
        {
            let mut g = graph.lock().unwrap();
            for file in &files {
                let is_test = self.config.is_test_file(file);
                g.add_file(file.clone(), is_test);
            }
        }

        let parse_results: Vec<_> = files
            .par_iter()
            .filter_map(|file| {
                match parser::parse_file(file) {
                    Ok(imports) => Some((file.clone(), imports)),
                    Err(e) => {
                        eprintln!("Warning: Failed to parse {}: {}", file.display(), e);
                        None
                    }
                }
            })
            .collect();

        {
            let mut g = graph.lock().unwrap();
            for (file, imports) in parse_results {
                let from_id = g.get_file_id(&file).unwrap();
                
                for import in imports {
                    if let Ok(resolved) = self.resolver.resolve(&file, &import.source) {
                        if import.import_type == ImportType::ReExport {
                            if let Some(to_id) = g.get_file_id(&resolved) {
                                g.add_dependency(from_id, to_id);
                            }
                        } else if let Some(to_id) = g.get_file_id(&resolved) {
                            g.add_dependency(from_id, to_id);
                        }
                    }
                }
            }
        }

        Arc::try_unwrap(graph)
            .map_err(|_| BuildError::GraphError("Failed to unwrap graph".to_string()))?
            .into_inner()
            .map_err(|e| BuildError::GraphError(e.to_string()))
    }

    pub fn update_incremental(&self, graph: &mut DependencyGraph) -> Result<usize, BuildError> {
        let stale_files = graph.get_stale_files();
        let current_files = self.discover_files()?;
        let current_set: std::collections::HashSet<_> = current_files.iter().collect();
        let existing_set: std::collections::HashSet<_> = graph.get_all_paths().into_iter().collect();

        let new_files: Vec<_> = current_files
            .iter()
            .filter(|f| !existing_set.contains(*f))
            .cloned()
            .collect();

        let deleted_files: Vec<_> = existing_set
            .iter()
            .filter(|f| !current_set.contains(f))
            .cloned()
            .collect();

        if stale_files.is_empty() && new_files.is_empty() && deleted_files.is_empty() {
            return Ok(0);
        }

        for path in &deleted_files {
            if let Some(id) = graph.get_file_id(path) {
                graph.remove_file(id);
            }
        }

        for path in &new_files {
            let is_test = self.config.is_test_file(path);
            graph.add_file(path.clone(), is_test);
        }

        let files_to_parse: Vec<_> = stale_files
            .into_iter()
            .filter(|f| current_set.contains(f))
            .chain(new_files.into_iter())
            .collect();

        let update_count = files_to_parse.len();

        for path in &files_to_parse {
            let is_test = self.config.is_test_file(path);
            if graph.contains_file(path) {
                graph.update_file(path, is_test);
            }
        }

        let parse_results: Vec<_> = files_to_parse
            .par_iter()
            .filter_map(|file| {
                match parser::parse_file(file) {
                    Ok(imports) => Some((file.clone(), imports)),
                    Err(e) => {
                        eprintln!("Warning: Failed to parse {}: {}", file.display(), e);
                        None
                    }
                }
            })
            .collect();

        for (file, imports) in parse_results {
            if let Some(from_id) = graph.get_file_id(&file) {
                for import in imports {
                    if let Ok(resolved) = self.resolver.resolve(&file, &import.source) {
                        if let Some(to_id) = graph.get_file_id(&resolved) {
                            graph.add_dependency(from_id, to_id);
                        }
                    }
                }
            }
        }

        Ok(update_count + deleted_files.len())
    }

    fn discover_files(&self) -> Result<Vec<PathBuf>, BuildError> {
        let mut files = Vec::new();

        let walker = WalkBuilder::new(&self.root)
            .hidden(true)
            .git_ignore(true)
            .build();

        for entry in walker {
            let entry = entry.map_err(|e| BuildError::WalkError(e.to_string()))?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            if !self.config.is_supported_extension(path) {
                continue;
            }

            if self.config.should_ignore(path) {
                continue;
            }

            files.push(path.to_path_buf());
        }

        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn test_discover_files() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        
        fs::write(src.join("a.ts"), "export const a = 1;").unwrap();
        fs::write(src.join("b.tsx"), "export const b = 2;").unwrap();
        fs::write(src.join("c.css"), "body {}").unwrap();

        let builder = GraphBuilder::new(temp.path().to_path_buf(), Config::default());
        let files = builder.discover_files().unwrap();

        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_build_graph() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        fs::create_dir_all(&src).unwrap();

        fs::write(src.join("a.ts"), r#"import { b } from './b';"#).unwrap();
        fs::write(src.join("b.ts"), "export const b = 1;").unwrap();
        fs::write(src.join("a.spec.ts"), r#"import { a } from './a';"#).unwrap();

        let builder = GraphBuilder::new(temp.path().to_path_buf(), Config::default());
        let graph = builder.build().unwrap();

        assert_eq!(graph.file_count(), 3);
        assert!(graph.edge_count() > 0);
    }

    #[test]
    fn test_incremental_update() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        fs::create_dir_all(&src).unwrap();

        fs::write(src.join("a.ts"), "export const a = 1;").unwrap();
        fs::write(src.join("b.ts"), "export const b = 2;").unwrap();

        let builder = GraphBuilder::new(temp.path().to_path_buf(), Config::default());
        let mut graph = builder.build().unwrap();

        assert_eq!(graph.file_count(), 2);

        fs::write(src.join("c.ts"), r#"import { a } from './a';"#).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));

        let _updated = builder.update_incremental(&mut graph).unwrap();

        assert_eq!(graph.file_count(), 3);
        assert!(graph.contains_file(&src.join("c.ts")));
    }
}

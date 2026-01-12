use std::path::{Path, PathBuf};
use std::fs;
use thiserror::Error;

use crate::graph::{DependencyGraph, SerializedGraph};

const CACHE_VERSION: u32 = 1;
const CACHE_DIR: &str = ".scopetest";
const CACHE_FILE: &str = "cache.bin";

#[derive(Error, Debug)]
pub enum CacheError {
    #[error("Failed to read cache: {0}")]
    ReadError(String),
    #[error("Failed to write cache: {0}")]
    WriteError(String),
    #[error("Cache version mismatch")]
    VersionMismatch,
    #[error("Cache corrupted")]
    Corrupted,
}

pub struct CacheManager {
    cache_dir: PathBuf,
}

impl CacheManager {
    pub fn new(project_root: &Path) -> Self {
        Self { cache_dir: project_root.join(CACHE_DIR) }
    }

    fn cache_path(&self) -> PathBuf {
        self.cache_dir.join(CACHE_FILE)
    }

    pub fn load(&self) -> Result<Option<DependencyGraph>, CacheError> {
        let cache_path = self.cache_path();
        
        if !cache_path.exists() {
            return Ok(None);
        }

        let data = fs::read(&cache_path)
            .map_err(|e| CacheError::ReadError(e.to_string()))?;

        let serialized: SerializedGraph = bincode::deserialize(&data)
            .map_err(|_| CacheError::Corrupted)?;

        if serialized.version != CACHE_VERSION {
            return Err(CacheError::VersionMismatch);
        }

        Ok(Some(DependencyGraph::deserialize(serialized)))
    }

    pub fn save(&self, graph: &DependencyGraph) -> Result<(), CacheError> {
        fs::create_dir_all(&self.cache_dir)
            .map_err(|e| CacheError::WriteError(e.to_string()))?;

        let serialized = graph.serialize();
        let data = bincode::serialize(&serialized)
            .map_err(|e| CacheError::WriteError(e.to_string()))?;

        fs::write(self.cache_path(), data)
            .map_err(|e| CacheError::WriteError(e.to_string()))?;

        Ok(())
    }

    pub fn invalidate(&self) -> Result<(), CacheError> {
        let cache_path = self.cache_path();
        
        if cache_path.exists() {
            fs::remove_file(&cache_path)
                .map_err(|e| CacheError::WriteError(e.to_string()))?;
        }

        Ok(())
    }

    pub fn exists(&self) -> bool {
        self.cache_path().exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_roundtrip() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::new(temp.path());

        let mut graph = DependencyGraph::new();
        graph.add_file(PathBuf::from("/test/a.ts"), false);
        graph.add_file(PathBuf::from("/test/b.ts"), true);

        cache.save(&graph).unwrap();
        
        let loaded = cache.load().unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().file_count(), 2);
    }

    #[test]
    fn test_cache_invalidate() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::new(temp.path());

        let graph = DependencyGraph::new();
        cache.save(&graph).unwrap();
        
        assert!(cache.exists());
        cache.invalidate().unwrap();
        assert!(!cache.exists());
    }
}

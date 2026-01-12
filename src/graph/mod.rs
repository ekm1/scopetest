use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use serde::{Deserialize, Serialize};

pub type FileId = NodeIndex<u32>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub path: PathBuf,
    pub is_test: bool,
    pub last_modified: u64,
    pub content_hash: u64,
}

impl FileNode {
    pub fn new(path: PathBuf, is_test: bool) -> Self {
        let last_modified = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|t| t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs())
            .unwrap_or(0);
        
        let content_hash = std::fs::read(&path)
            .map(|content| {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                content.hash(&mut hasher);
                hasher.finish()
            })
            .unwrap_or(0);
        
        Self { path, is_test, last_modified, content_hash }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerializedGraph {
    pub version: u32,
    pub nodes: Vec<FileNode>,
    pub edges: Vec<(u32, u32)>,
}

#[derive(Debug)]
pub struct DependencyGraph {
    graph: DiGraph<FileNode, ()>,
    path_to_id: HashMap<PathBuf, FileId>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            path_to_id: HashMap::new(),
        }
    }

    pub fn add_file(&mut self, path: PathBuf, is_test: bool) -> FileId {
        let canonical_path = std::fs::canonicalize(&path).unwrap_or(path);
        
        if let Some(&id) = self.path_to_id.get(&canonical_path) {
            return id;
        }
        
        let node = FileNode::new(canonical_path.clone(), is_test);
        let id = self.graph.add_node(node);
        self.path_to_id.insert(canonical_path, id);
        id
    }

    pub fn add_dependency(&mut self, from: FileId, to: FileId) {
        if !self.graph.contains_edge(from, to) {
            self.graph.add_edge(from, to, ());
        }
    }

    pub fn get_file_id(&self, path: &Path) -> Option<FileId> {
        if let Some(&id) = self.path_to_id.get(path) {
            return Some(id);
        }
        if let Ok(canonical) = std::fs::canonicalize(path) {
            return self.path_to_id.get(&canonical).copied();
        }
        None
    }

    pub fn get_file_path(&self, id: FileId) -> Option<&Path> {
        self.graph.node_weight(id).map(|n| n.path.as_path())
    }

    pub fn get_file_node(&self, id: FileId) -> Option<&FileNode> {
        self.graph.node_weight(id)
    }

    pub fn get_dependents(&self, file: FileId) -> Vec<FileId> {
        self.graph.neighbors_directed(file, Direction::Incoming).collect()
    }

    pub fn get_dependencies(&self, file: FileId) -> Vec<FileId> {
        self.graph.neighbors_directed(file, Direction::Outgoing).collect()
    }

    pub fn get_transitive_dependents(&self, files: &[FileId]) -> HashSet<FileId> {
        let mut result = HashSet::new();
        
        for &start in files {
            let mut visited = HashSet::new();
            let mut queue = vec![start];
            
            while let Some(current) = queue.pop() {
                if visited.contains(&current) {
                    continue;
                }
                visited.insert(current);
                result.insert(current);
                
                for dependent in self.get_dependents(current) {
                    if !visited.contains(&dependent) {
                        queue.push(dependent);
                    }
                }
            }
        }
        
        result
    }

    pub fn get_test_files(&self) -> Vec<FileId> {
        self.graph
            .node_indices()
            .filter(|&id| self.graph.node_weight(id).map(|n| n.is_test).unwrap_or(false))
            .collect()
    }

    pub fn get_all_files(&self) -> Vec<FileId> {
        self.graph.node_indices().collect()
    }

    pub fn file_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn remove_file(&mut self, id: FileId) {
        if let Some(node) = self.graph.node_weight(id) {
            self.path_to_id.remove(&node.path.clone());
        }
        self.graph.remove_node(id);
    }

    pub fn contains_file(&self, path: &Path) -> bool {
        self.get_file_id(path).is_some()
    }

    pub fn serialize(&self) -> SerializedGraph {
        let nodes: Vec<FileNode> = self.graph
            .node_indices()
            .filter_map(|id| self.graph.node_weight(id).cloned())
            .collect();
        
        let edges: Vec<(u32, u32)> = self.graph
            .edge_indices()
            .filter_map(|e| {
                self.graph.edge_endpoints(e).map(|(a, b)| (a.index() as u32, b.index() as u32))
            })
            .collect();

        SerializedGraph { version: 1, nodes, edges }
    }

    pub fn deserialize(data: SerializedGraph) -> Self {
        let mut graph = DiGraph::new();
        let mut path_to_id = HashMap::new();

        for node in data.nodes {
            let path = node.path.clone();
            let id = graph.add_node(node);
            path_to_id.insert(path, id);
        }

        for (from, to) in data.edges {
            let from_id = NodeIndex::new(from as usize);
            let to_id = NodeIndex::new(to as usize);
            graph.add_edge(from_id, to_id, ());
        }

        Self { graph, path_to_id }
    }

    pub fn get_all_paths(&self) -> Vec<PathBuf> {
        self.path_to_id.keys().cloned().collect()
    }

    pub fn get_stale_files(&self) -> Vec<PathBuf> {
        let mut stale = Vec::new();
        
        for (path, &id) in &self.path_to_id {
            if let Some(node) = self.graph.node_weight(id) {
                if !path.exists() {
                    stale.push(path.clone());
                    continue;
                }
                
                let current_mtime = std::fs::metadata(path)
                    .and_then(|m| m.modified())
                    .map(|t| t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs())
                    .unwrap_or(0);
                
                if current_mtime != node.last_modified {
                    stale.push(path.clone());
                }
            }
        }
        
        stale
    }

    pub fn clear_dependencies(&mut self, id: FileId) {
        let deps: Vec<_> = self.get_dependencies(id);
        for dep in deps {
            if let Some(edge) = self.graph.find_edge(id, dep) {
                self.graph.remove_edge(edge);
            }
        }
    }

    pub fn update_file(&mut self, path: &Path, is_test: bool) -> Option<FileId> {
        if let Some(&id) = self.path_to_id.get(path) {
            let new_node = FileNode::new(path.to_path_buf(), is_test);
            if let Some(node) = self.graph.node_weight_mut(id) {
                *node = new_node;
            }
            self.clear_dependencies(id);
            Some(id)
        } else {
            None
        }
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for DependencyGraph {
    fn clone(&self) -> Self {
        Self::deserialize(self.serialize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_file() {
        let mut graph = DependencyGraph::new();
        let id = graph.add_file(PathBuf::from("/test/a.ts"), false);
        
        assert_eq!(graph.file_count(), 1);
        assert_eq!(graph.get_file_path(id), Some(Path::new("/test/a.ts")));
    }

    #[test]
    fn test_add_dependency() {
        let mut graph = DependencyGraph::new();
        let a = graph.add_file(PathBuf::from("/test/a.ts"), false);
        let b = graph.add_file(PathBuf::from("/test/b.ts"), false);
        
        graph.add_dependency(a, b);
        
        assert_eq!(graph.edge_count(), 1);
        assert_eq!(graph.get_dependencies(a), vec![b]);
        assert_eq!(graph.get_dependents(b), vec![a]);
    }

    #[test]
    fn test_transitive_dependents() {
        let mut graph = DependencyGraph::new();
        let a = graph.add_file(PathBuf::from("/test/a.ts"), false);
        let b = graph.add_file(PathBuf::from("/test/b.ts"), false);
        let c = graph.add_file(PathBuf::from("/test/c.ts"), true);
        
        graph.add_dependency(b, a);
        graph.add_dependency(c, b);
        
        let dependents = graph.get_transitive_dependents(&[a]);
        
        assert!(dependents.contains(&a));
        assert!(dependents.contains(&b));
        assert!(dependents.contains(&c));
    }

    #[test]
    fn test_get_test_files() {
        let mut graph = DependencyGraph::new();
        graph.add_file(PathBuf::from("/test/a.ts"), false);
        graph.add_file(PathBuf::from("/test/a.spec.ts"), true);
        graph.add_file(PathBuf::from("/test/b.ts"), false);
        graph.add_file(PathBuf::from("/test/b.test.ts"), true);
        
        let tests = graph.get_test_files();
        assert_eq!(tests.len(), 2);
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut graph = DependencyGraph::new();
        let a = graph.add_file(PathBuf::from("/test/a.ts"), false);
        let b = graph.add_file(PathBuf::from("/test/b.ts"), true);
        graph.add_dependency(a, b);

        let serialized = graph.serialize();
        let restored = DependencyGraph::deserialize(serialized);

        assert_eq!(restored.file_count(), 2);
        assert_eq!(restored.edge_count(), 1);
    }
}

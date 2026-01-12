use std::path::{Path, PathBuf};
use std::collections::{HashSet, VecDeque};

use crate::graph::{DependencyGraph, FileId};
use crate::git::ChangeSet;

#[derive(Debug, Default)]
pub struct AffectedResult {
    pub tests: Vec<PathBuf>,
    pub sources: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct DependencyPath {
    pub chain: Vec<PathBuf>,
}

impl DependencyPath {
    pub fn format(&self) -> String {
        self.chain
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(" → ")
    }
}

pub struct AffectedTestFinder<'a> {
    graph: &'a DependencyGraph,
}

impl<'a> AffectedTestFinder<'a> {
    pub fn new(graph: &'a DependencyGraph) -> Self {
        Self { graph }
    }

    pub fn find_affected(&self, changes: &ChangeSet) -> AffectedResult {
        let changed_paths = changes.all_changed();
        
        let changed_ids: Vec<_> = changed_paths
            .iter()
            .filter_map(|p| self.graph.get_file_id(p))
            .collect();

        if changed_ids.is_empty() {
            return AffectedResult::default();
        }

        let affected_ids = self.graph.get_transitive_dependents(&changed_ids);

        let mut tests = Vec::new();
        let mut sources = Vec::new();
        let mut seen_paths: HashSet<PathBuf> = HashSet::new();

        for id in affected_ids {
            if let Some(node) = self.graph.get_file_node(id) {
                if seen_paths.contains(&node.path) {
                    continue;
                }
                seen_paths.insert(node.path.clone());

                if node.is_test {
                    tests.push(node.path.clone());
                } else {
                    let path_str = node.path.to_string_lossy();
                    if !path_str.contains("node_modules") {
                        sources.push(node.path.clone());
                    }
                }
            }
        }

        tests.sort();
        sources.sort();

        AffectedResult { tests, sources }
    }

    pub fn find_why(&self, test_path: &Path, changes: &ChangeSet) -> Option<DependencyPath> {
        let test_id = self.graph.get_file_id(test_path)?;
        
        let changed_ids: HashSet<_> = changes
            .all_changed()
            .iter()
            .filter_map(|p| self.graph.get_file_id(p))
            .collect();

        if changed_ids.is_empty() {
            return None;
        }

        let mut queue: VecDeque<(FileId, Vec<FileId>)> = VecDeque::new();
        let mut visited: HashSet<FileId> = HashSet::new();

        queue.push_back((test_id, vec![test_id]));
        visited.insert(test_id);

        while let Some((current, path)) = queue.pop_front() {
            if changed_ids.contains(&current) {
                let chain: Vec<PathBuf> = path
                    .iter()
                    .filter_map(|&id| self.graph.get_file_path(id).map(|p| p.to_path_buf()))
                    .collect();
                return Some(DependencyPath { chain });
            }

            for dep in self.graph.get_dependencies(current) {
                if !visited.contains(&dep) {
                    visited.insert(dep);
                    let mut new_path = path.clone();
                    new_path.push(dep);
                    queue.push_back((dep, new_path));
                }
            }
        }

        None
    }

    pub fn find_all_paths_to_test(&self, test_path: &Path, changes: &ChangeSet) -> Vec<DependencyPath> {
        let test_id = match self.graph.get_file_id(test_path) {
            Some(id) => id,
            None => return vec![],
        };

        let changed_ids: HashSet<_> = changes
            .all_changed()
            .iter()
            .filter_map(|p| self.graph.get_file_id(p))
            .collect();

        if changed_ids.is_empty() {
            return vec![];
        }

        let mut paths = Vec::new();

        for &changed_id in &changed_ids {
            if let Some(path) = self.find_path_between(changed_id, test_id) {
                paths.push(path);
            }
        }

        paths
    }

    fn find_path_between(&self, from: FileId, to: FileId) -> Option<DependencyPath> {
        let mut queue: VecDeque<(FileId, Vec<FileId>)> = VecDeque::new();
        let mut visited: HashSet<FileId> = HashSet::new();

        queue.push_back((from, vec![from]));
        visited.insert(from);

        while let Some((current, path)) = queue.pop_front() {
            if current == to {
                let chain: Vec<PathBuf> = path
                    .iter()
                    .filter_map(|&id| self.graph.get_file_path(id).map(|p| p.to_path_buf()))
                    .collect();
                return Some(DependencyPath { chain });
            }

            for dep in self.graph.get_dependents(current) {
                if !visited.contains(&dep) {
                    visited.insert(dep);
                    let mut new_path = path.clone();
                    new_path.push(dep);
                    queue.push_back((dep, new_path));
                }
            }
        }

        None
    }

    pub fn get_totals(&self) -> (usize, usize) {
        let all_files = self.graph.get_all_files();
        let test_count = self.graph.get_test_files().len();
        let source_count = all_files.len() - test_count;
        (test_count, source_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_graph() -> DependencyGraph {
        let mut graph = DependencyGraph::new();
        
        let utils = graph.add_file(PathBuf::from("/src/utils.ts"), false);
        let component = graph.add_file(PathBuf::from("/src/component.ts"), false);
        let test = graph.add_file(PathBuf::from("/src/test.spec.ts"), true);
        
        graph.add_dependency(component, utils);
        graph.add_dependency(test, component);
        
        graph
    }

    #[test]
    fn test_find_affected_direct() {
        let graph = create_test_graph();
        let finder = AffectedTestFinder::new(&graph);

        let changes = ChangeSet {
            modified: vec![PathBuf::from("/src/component.ts")],
            ..Default::default()
        };

        let result = finder.find_affected(&changes);
        
        assert_eq!(result.tests.len(), 1);
        assert!(result.tests[0].to_string_lossy().contains("test.spec.ts"));
    }

    #[test]
    fn test_find_affected_transitive() {
        let graph = create_test_graph();
        let finder = AffectedTestFinder::new(&graph);

        let changes = ChangeSet {
            modified: vec![PathBuf::from("/src/utils.ts")],
            ..Default::default()
        };

        let result = finder.find_affected(&changes);
        
        assert_eq!(result.tests.len(), 1);
        assert!(result.tests[0].to_string_lossy().contains("test.spec.ts"));
        assert!(result.sources.len() >= 1);
    }

    #[test]
    fn test_find_affected_no_changes() {
        let graph = create_test_graph();
        let finder = AffectedTestFinder::new(&graph);

        let changes = ChangeSet {
            modified: vec![PathBuf::from("/src/unknown.ts")],
            ..Default::default()
        };

        let result = finder.find_affected(&changes);
        
        assert!(result.tests.is_empty());
    }
}

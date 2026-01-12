use std::path::PathBuf;
use std::collections::HashSet;

use crate::graph::DependencyGraph;
use crate::git::ChangeSet;

#[derive(Debug, Default)]
pub struct AffectedResult {
    pub tests: Vec<PathBuf>,
    pub sources: Vec<PathBuf>,
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

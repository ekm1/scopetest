use proptest::prelude::*;
use std::collections::HashSet;
use std::path::PathBuf;
use tempfile::TempDir;
use std::fs;

use crate::parser;
use crate::graph::DependencyGraph;
use crate::config::Config;
use crate::output::OutputFormatter;

fn arb_import() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z]{1,6}".prop_map(|n| format!("import {{ {} }} from './{}';", n, n)),
        "[a-z]{1,6}".prop_map(|n| format!("import {} from './{}';", n, n)),
        "[a-z]{1,6}".prop_map(|n| format!("export * from './{}';", n)),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_import_extraction(imports in prop::collection::vec(arb_import(), 1..5)) {
        let code = imports.join("\n");
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.ts");
        fs::write(&file_path, &code).unwrap();
        let result = parser::parse_file(&file_path);
        prop_assert!(result.is_ok());
        prop_assert!(result.unwrap().len() >= 1);
    }

    #[test]
    fn prop_transitive_deps(chain_length in 2usize..8) {
        let mut graph = DependencyGraph::new();
        let mut ids = Vec::new();
        for i in 0..chain_length {
            let id = graph.add_file(PathBuf::from(format!("/f{}.ts", i)), false);
            ids.push(id);
        }
        for i in 0..chain_length - 1 {
            graph.add_dependency(ids[i + 1], ids[i]);
        }
        let deps = graph.get_transitive_dependents(&[ids[0]]);
        prop_assert_eq!(deps.len(), chain_length);
    }

    #[test]
    fn prop_test_pattern(name in "[a-z]{1,8}", is_test in prop::bool::ANY) {
        let config = Config::default();
        let file = if is_test { format!("{}.spec.ts", name) } else { format!("{}.ts", name) };
        prop_assert_eq!(config.is_test_file(&PathBuf::from(&file)), is_test);
    }

    #[test]
    fn prop_json_valid(n in 0usize..5) {
        let tests: Vec<PathBuf> = (0..n).map(|i| PathBuf::from(format!("/t{}.spec.ts", i))).collect();
        let json = OutputFormatter::format_json(&tests, &[], n + 5, 10);
        prop_assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());
    }

    #[test]
    fn prop_cache_roundtrip(n in 1usize..10) {
        let mut graph = DependencyGraph::new();
        for i in 0..n {
            graph.add_file(PathBuf::from(format!("/f{}.ts", i)), i % 2 == 0);
        }
        let count = graph.file_count();
        let restored = DependencyGraph::deserialize(graph.serialize());
        prop_assert_eq!(restored.file_count(), count);
    }
}

#[test]
fn test_cyclic_deps() {
    let mut graph = DependencyGraph::new();
    let a = graph.add_file(PathBuf::from("/a.ts"), false);
    let b = graph.add_file(PathBuf::from("/b.ts"), false);
    let c = graph.add_file(PathBuf::from("/c.ts"), true);
    graph.add_dependency(a, b);
    graph.add_dependency(b, c);
    graph.add_dependency(c, a);
    assert_eq!(graph.get_transitive_dependents(&[a]).len(), 3);
}

#[test]
fn test_diamond_deps() {
    let mut graph = DependencyGraph::new();
    let a = graph.add_file(PathBuf::from("/a.ts"), false);
    let b = graph.add_file(PathBuf::from("/b.ts"), false);
    let c = graph.add_file(PathBuf::from("/c.ts"), false);
    let d = graph.add_file(PathBuf::from("/d.spec.ts"), true);
    graph.add_dependency(b, a);
    graph.add_dependency(c, a);
    graph.add_dependency(d, b);
    graph.add_dependency(d, c);
    let deps = graph.get_transitive_dependents(&[a]);
    let unique: HashSet<_> = deps.iter().collect();
    assert_eq!(unique.len(), 4);
}

#[test]
fn test_empty_graph() {
    let graph = DependencyGraph::new();
    let restored = DependencyGraph::deserialize(graph.serialize());
    assert_eq!(restored.file_count(), 0);
}

#[test]
fn test_jest_escaping() {
    let tests = vec![PathBuf::from("/src/[1].spec.ts"), PathBuf::from("/src/(2).spec.ts")];
    let pattern = OutputFormatter::format_jest_pattern(&tests);
    assert!(regex::Regex::new(&pattern).is_ok());
}

#[test]
fn test_empty_json() {
    let json = OutputFormatter::format_json(&[], &[], 100, 500);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["stats"]["affected_tests"], 0);
}

#[test]
fn test_list_format() {
    let files = vec![PathBuf::from("/a.ts"), PathBuf::from("/b.ts")];
    assert_eq!(OutputFormatter::format_list(&files).lines().count(), 2);
}

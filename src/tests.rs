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
    let reserved = &["do", "if", "in", "for", "let", "new", "try", "var", "case", "else", "enum", "null", "this", "true", "void", "with"];
    prop_oneof![
        "[a-z]{1,6}".prop_filter("avoid JS reserved words", move |n| !reserved.contains(&n.as_str()))
            .prop_map(|n| format!("import {{ {} }} from './{}';", n, n)),
        "[a-z]{1,6}".prop_filter("avoid JS reserved words", move |n| !reserved.contains(&n.as_str()))
            .prop_map(|n| format!("import {} from './{}';", n, n)),
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
    let pattern = OutputFormatter::format_paths(&tests);
    assert!(!pattern.is_empty());
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

// === Bug reproduction: barrel `export { X }` (no `from` clause) is not tracked ===

/// Bug 1: BarrelAnalyzer ignores `export { X }` without a `from` clause.
/// When a barrel file does:
///   import { Foo } from "./Foo";
///   export { Foo };
/// The analyzer should map `Foo` → `./Foo` source file, but currently only
/// detects `default` (from `export default Foo`).
#[test]
fn test_barrel_analyzer_export_without_from_clause() {
    use crate::barrel::BarrelAnalyzer;

    let temp = TempDir::new().unwrap();
    let dir = temp.path();

    // Create the actual source file
    fs::write(dir.join("PersonalDetails.tsx"), r#"
        export const PersonalDetails = () => {};
    "#).unwrap();

    // Create a barrel that imports then re-exports without `from`
    fs::write(dir.join("index.ts"), r#"
import {PersonalDetails} from "./PersonalDetails";
export {PersonalDetails};
export default PersonalDetails;
"#).unwrap();

    let mut analyzer = BarrelAnalyzer::new(dir.to_path_buf());
    let exports = analyzer.analyze_barrel(&dir.join("index.ts"));

    // Should find both `PersonalDetails` and `default`
    assert!(
        exports.contains_key("PersonalDetails"),
        "Barrel analyzer should detect `export {{ X }}` without `from` clause. Got keys: {:?}",
        exports.keys().collect::<Vec<_>>()
    );
    assert!(exports.contains_key("default"));

    // `PersonalDetails` should point to the actual source, not the barrel itself
    let source = &exports["PersonalDetails"];
    let source_str = source.source_path.to_string_lossy();
    assert!(
        source_str.contains("PersonalDetails.tsx"),
        "Expected source to point to PersonalDetails.tsx, got: {}",
        source_str
    );
}

/// Bug 2: Builder skips fallback edge when barrel expansion resolves 0 names.
/// When `check_and_cache_barrel` returns true but `resolve_barrel_imports` can't
/// find the requested name, the builder `continue`s — no edge is added at all.
/// The importer becomes disconnected from the package it depends on.
///
/// Key setup: the top barrel MUST have SOME valid exports (so it's cached as a barrel)
/// but NOT the specific name being imported. This triggers the `continue` with no edge.
#[test]
fn test_builder_fallback_when_barrel_expansion_finds_nothing() {
    use crate::builder::GraphBuilder;

    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let pkg = root.join("packages").join("my-lib").join("src");
    let sub_widget = pkg.join("components").join("Widget");
    let sub_utils = pkg.join("utils");
    let app = root.join("packages").join("my-app").join("src");
    fs::create_dir_all(&sub_widget).unwrap();
    fs::create_dir_all(&sub_utils).unwrap();
    fs::create_dir_all(&app).unwrap();

    // A sub-module that DOES export correctly (via `export { X } from`)
    fs::write(sub_utils.join("helpers.ts"), r#"export const helper = () => {};"#).unwrap();
    fs::write(sub_utils.join("index.ts"), r#"export { helper } from "./helpers";"#).unwrap();

    // Widget sub-barrel: import + export (no `from`) — broken pattern
    fs::write(sub_widget.join("Widget.tsx"), r#"export const Widget = () => {};"#).unwrap();
    fs::write(sub_widget.join("index.ts"), r#"
import {Widget} from "./Widget";
export {Widget};
"#).unwrap();

    // Components barrel — pure re-export from Widget
    let components_dir = pkg.join("components");
    fs::write(components_dir.join("index.ts"), r#"
export * from "./Widget";
"#).unwrap();

    // Top-level package barrel — exports from BOTH utils and components
    // utils will succeed (so barrel IS cached with non-empty exports)
    // components/Widget will fail (bug 1) so "Widget" won't be in the map
    fs::write(pkg.join("index.ts"), r#"
export * from "./utils";
export * from "./components";
"#).unwrap();

    // App file imports Widget (which is NOT in the barrel's resolved exports)
    fs::write(app.join("page.tsx"), &format!(
        r#"import {{Widget}} from "{}";"#,
        pkg.join("index").to_string_lossy()
    )).unwrap();

    // Test imports from page
    fs::write(app.join("page.spec.tsx"), r#"import {Widget} from "./page";"#).unwrap();

    let config = Config::default();
    let graph = GraphBuilder::new(root.to_path_buf(), config).build().unwrap();

    let page_path = std::fs::canonicalize(app.join("page.tsx")).unwrap();
    let page_id = graph.get_file_id(&page_path);
    assert!(page_id.is_some(), "page file should be in graph");

    // page.tsx must depend on SOMETHING in the package
    let page_deps = graph.get_dependencies(page_id.unwrap());

    let widget_path = std::fs::canonicalize(sub_widget.join("Widget.tsx")).unwrap();
    let top_barrel_path = std::fs::canonicalize(pkg.join("index.ts")).unwrap();
    let components_barrel_path = std::fs::canonicalize(components_dir.join("index.ts")).unwrap();
    let sub_barrel_path = std::fs::canonicalize(sub_widget.join("index.ts")).unwrap();

    let has_any_pkg_dep = page_deps.iter().any(|&dep| {
        graph.get_file_path(dep).map(|p| {
            p == widget_path || p == top_barrel_path || p == components_barrel_path || p == sub_barrel_path
        }).unwrap_or(false)
    });

    assert!(
        has_any_pkg_dep,
        "page.tsx should have at least one dependency on the package (barrel or Widget.tsx). \
         Bug 2: builder `continue`s past fallback when barrel expansion can't resolve 'Widget'. \
         Actual deps: {:?}",
        page_deps.iter()
            .filter_map(|&id| graph.get_file_path(id).map(|p| p.to_path_buf()))
            .collect::<Vec<_>>()
    );
}

/// End-to-end: test affected detection through a barrel with `export { X }` pattern.
/// Simulates the real-world case: changed file deep in a package should mark a
/// cross-package test as affected via barrel re-exports.
#[test]
fn test_affected_detection_through_reexport_barrel() {
    use crate::builder::GraphBuilder;
    use crate::affected::AffectedTestFinder;
    use crate::git::ChangeSet;

    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Package: @tipico/personal-details equivalent
    let pkg_src = root.join("packages").join("mods").join("personal-details").join("src");
    let pkg_components = pkg_src.join("components").join("PersonalDetails");
    let pkg_contact = pkg_components.join("ContactDetails").join("ContactDetailsForm");
    fs::create_dir_all(&pkg_contact).unwrap();

    // Deep source file (will be "changed")
    fs::write(pkg_contact.join("types.ts"), r#"
        export interface ContactFormFields { email: string; }
    "#).unwrap();

    // ContactDetailsForm barrel
    fs::write(pkg_contact.join("index.tsx"), r#"
export * from "./types";
export const ContactDetailsForm = () => {};
"#).unwrap();

    // ContactDetails component imports ContactDetailsForm
    let contact_details_dir = pkg_components.join("ContactDetails");
    fs::write(contact_details_dir.join("ContactDetails.tsx"), r#"
import ContactDetailsForm from "./ContactDetailsForm";
export const ContactDetailsComp = () => {};
"#).unwrap();

    // ContactDetails barrel — pure re-export
    fs::write(contact_details_dir.join("index.tsx"), r#"
export {ContactDetailsComp as ContactDetails} from "./ContactDetails";
"#).unwrap();

    // PersonalDetails component imports ContactDetails
    fs::write(pkg_components.join("PersonalDetails.tsx"), r#"
import {ContactDetails} from "./ContactDetails";
export const PersonalDetails = () => {};
"#).unwrap();

    // PersonalDetails barrel — import + export (BUG 1 pattern)
    fs::write(pkg_components.join("index.ts"), r#"
import {PersonalDetails} from "./PersonalDetails";
export {PersonalDetails};
export default PersonalDetails;
"#).unwrap();

    // Package components barrel
    let pkg_components_root = pkg_src.join("components");
    fs::write(pkg_components_root.join("index.tsx"), r#"
export * from "./PersonalDetails";
"#).unwrap();

    // Package top-level barrel
    fs::write(pkg_src.join("index.tsx"), r#"
export * from "./components";
"#).unwrap();

    // App test that imports PersonalDetails from the package barrel
    let app_dir = root.join("packages").join("apps").join("account").join("src");
    fs::create_dir_all(&app_dir).unwrap();

    fs::write(app_dir.join("page.tsx"), &format!(
        r#"import {{PersonalDetails}} from "{}";"#,
        pkg_src.join("index").to_string_lossy()
    )).unwrap();

    fs::write(app_dir.join("page.spec.tsx"), r#"
import {PersonalDetails} from "./page";
describe("test", () => { it("works", () => {}); });
"#).unwrap();

    let config = Config::default();
    let graph = GraphBuilder::new(root.to_path_buf(), config).build().unwrap();

    // Simulate a change to the deep file
    let changed_file = std::fs::canonicalize(pkg_contact.join("types.ts")).unwrap();
    let changes = ChangeSet {
        modified: vec![changed_file.clone()],
        ..Default::default()
    };

    let finder = AffectedTestFinder::new(&graph);
    let result = finder.find_affected(&changes);

    let test_canonical = std::fs::canonicalize(app_dir.join("page.spec.tsx")).unwrap();
    let test_is_affected = result.tests.iter().any(|t| {
        std::fs::canonicalize(t).unwrap_or(t.clone()) == test_canonical
    });

    assert!(
        test_is_affected,
        "Test page.spec.tsx should be affected by change to types.ts through barrel chain.\n\
         Affected tests: {:?}",
        result.tests
    );
}

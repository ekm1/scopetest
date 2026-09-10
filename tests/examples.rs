//! End-to-end tests over the fixtures in `examples/`.
//!
//! Each test copies an example into a temp dir, makes it a git repo, edits one
//! file, and runs the real binary against it. That exercises the whole path
//! (discovery, parsing, alias resolution, barrel expansion, git diff, package
//! mapping) the way a user would, rather than any single module in isolation.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_scopetest");

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
}

impl Fixture {
    /// Copy `examples/<name>` into a fresh git repo.
    fn new(name: &str) -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join(name);
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(name);

        assert!(
            source.exists(),
            "missing example fixture: {}",
            source.display()
        );
        copy_dir(&source, &root);

        git(&root, &["init", "-q", "."]);
        git(&root, &["add", "-A"]);
        git(
            &root,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "commit",
                "-qm",
                "init",
            ],
        );

        Self { _temp: temp, root }
    }

    fn append(&self, relative: &str, content: &str) {
        let path = self.root.join(relative);
        let mut existing = fs::read_to_string(&path).unwrap();
        existing.push_str(content);
        fs::write(&path, existing).unwrap();
    }

    fn run(&self, args: &[&str]) -> String {
        let output = Command::new(BIN)
            .args(args)
            .current_dir(&self.root)
            .output()
            .unwrap_or_else(|e| panic!("failed to run {BIN}: {e}"));

        assert!(
            output.status.success(),
            "scopetest {:?} failed:\n{}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8_lossy(&output.stdout).to_string()
    }

    /// Affected package names after the current working-tree edits.
    fn affected_packages(&self) -> Vec<String> {
        lines(&self.run(&["affected", "--base", "HEAD", "--packages", "-f", "list"]))
    }

    /// Affected test files, as paths relative to the fixture root.
    fn affected_tests(&self) -> Vec<String> {
        lines(&self.run(&["affected", "--base", "HEAD", "-f", "list"]))
            .into_iter()
            .map(|p| {
                Path::new(&p)
                    .strip_prefix(&self.root)
                    .map(|r| r.display().to_string())
                    .unwrap_or(p)
            })
            .collect()
    }
}

fn lines(output: &str) -> Vec<String> {
    let mut result: Vec<String> = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    result.sort();
    result
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git is required to run these tests");

    assert!(
        output.status.success(),
        "git {:?} failed:\n{}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

// ---------------------------------------------------------------- turborepo

#[test]
fn turborepo_init_detects_workspace() {
    let fixture = Fixture::new("turborepo");
    let output = fixture.run(&["init", "--dry-run"]);

    assert!(output.contains("Turborepo"), "output was:\n{output}");
    assert!(output.contains("packages:  4"), "output was:\n{output}");
    assert!(output.contains("@acme/ui"), "output was:\n{output}");
    // The snippet must be the one for the detected tool.
    assert!(output.contains("turbo run test"), "output was:\n{output}");
}

#[test]
fn turborepo_init_writes_config() {
    let fixture = Fixture::new("turborepo");
    let config = fixture.root.join(".scopetestrc.json");

    assert!(!config.exists());
    fixture.run(&["init"]);
    assert!(config.exists());

    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    assert!(parsed.get("testPatterns").is_some());
}

#[test]
fn turborepo_init_does_not_clobber_existing_config() {
    let fixture = Fixture::new("turborepo");
    let config = fixture.root.join(".scopetestrc.json");

    fs::write(&config, r#"{"testPatterns":["custom/**"]}"#).unwrap();
    let output = fixture.run(&["init"]);

    assert!(output.contains("already exists"), "output was:\n{output}");
    assert!(fs::read_to_string(&config).unwrap().contains("custom/**"));
}

#[test]
fn turborepo_change_to_shared_package_affects_its_consumer() {
    let fixture = Fixture::new("turborepo");
    fixture.append(
        "packages/ui/src/button.ts",
        "\nexport const VARIANT = 'primary';\n",
    );

    // web imports @acme/ui; admin and utils are untouched.
    assert_eq!(fixture.affected_packages(), vec!["@acme/ui", "@acme/web"]);
}

#[test]
fn turborepo_unrelated_packages_are_not_affected() {
    let fixture = Fixture::new("turborepo");
    fixture.append(
        "packages/utils/src/format.ts",
        "\nexport const SYMBOL = '$';\n",
    );

    assert_eq!(
        fixture.affected_packages(),
        vec!["@acme/admin", "@acme/utils"]
    );
}

#[test]
fn turborepo_emits_filter_flags() {
    let fixture = Fixture::new("turborepo");
    fixture.append(
        "packages/ui/src/button.ts",
        "\nexport const VARIANT = 'primary';\n",
    );

    let output = fixture.run(&["affected", "--base", "HEAD", "-f", "turbo"]);
    assert_eq!(output.trim(), "--filter=@acme/ui --filter=@acme/web");
}

#[test]
fn turborepo_no_changes_yields_no_packages() {
    let fixture = Fixture::new("turborepo");
    assert!(fixture.affected_packages().is_empty());
}

// ----------------------------------------------------------------------- nx

#[test]
fn nx_init_detects_workspace() {
    let fixture = Fixture::new("nx");
    let output = fixture.run(&["init", "--dry-run"]);

    assert!(output.contains("Nx"), "output was:\n{output}");
    // Four projects, and the root manifest is not one of them.
    assert!(output.contains("packages:  4"), "output was:\n{output}");
    assert!(!output.contains("nx-example"), "output was:\n{output}");
    assert!(output.contains("nx run-many"), "output was:\n{output}");
}

#[test]
fn nx_resolves_aliases_from_tsconfig_base() {
    let fixture = Fixture::new("nx");
    fixture.append("libs/data/src/cart.ts", "\nexport const MAX = 99;\n");

    // checkout imports @shop/data, which only tsconfig.base.json maps.
    assert_eq!(fixture.affected_packages(), vec!["checkout", "data"]);
}

#[test]
fn nx_change_to_widely_used_lib_affects_both_apps() {
    let fixture = Fixture::new("nx");
    fixture.append("libs/ui/src/button.ts", "\nexport const SIZE = 'lg';\n");

    assert_eq!(fixture.affected_packages(), vec!["checkout", "shell", "ui"]);
}

#[test]
fn nx_barrel_export_nobody_imports_stays_local() {
    let fixture = Fixture::new("nx");
    // Both apps import @shop/ui, whose barrel re-exports spinner. Barrel
    // expansion should keep this change inside ui rather than fanning out.
    fixture.append("libs/ui/src/spinner.ts", "\nexport const DELAY = 100;\n");

    assert_eq!(fixture.affected_packages(), vec!["ui"]);
}

#[test]
fn nx_emits_comma_separated_projects() {
    let fixture = Fixture::new("nx");
    fixture.append("libs/ui/src/button.ts", "\nexport const SIZE = 'lg';\n");

    let output = fixture.run(&["affected", "--base", "HEAD", "-f", "nx"]);
    assert_eq!(output.trim(), "checkout,shell,ui");
}

#[test]
fn nx_selects_only_the_affected_tests() {
    let fixture = Fixture::new("nx");
    fixture.append("libs/data/src/cart.ts", "\nexport const MAX = 99;\n");

    assert_eq!(
        fixture.affected_tests(),
        vec![
            "apps/checkout/src/page.spec.ts",
            "libs/data/src/cart.spec.ts"
        ]
    );
}

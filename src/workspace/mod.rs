use std::path::{Path, PathBuf};

/// The monorepo tool driving this workspace, if any.
///
/// Detection is by marker file. A repo can carry several markers (an Nx repo
/// with a `turbo.json` left over from a migration, say), so the order in
/// `detect_kind` decides which one wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceKind {
    Nx,
    Turborepo,
    Lerna,
    Pnpm,
    NpmWorkspaces,
    Single,
}

impl WorkspaceKind {
    pub fn label(&self) -> &'static str {
        match self {
            WorkspaceKind::Nx => "Nx",
            WorkspaceKind::Turborepo => "Turborepo",
            WorkspaceKind::Lerna => "Lerna",
            WorkspaceKind::Pnpm => "pnpm workspaces",
            WorkspaceKind::NpmWorkspaces => "npm/yarn workspaces",
            WorkspaceKind::Single => "single package",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Name from package.json, falling back to the directory name.
    pub name: String,
    /// Absolute path to the package directory.
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub kind: WorkspaceKind,
    pub packages: Vec<Package>,
}

impl Workspace {
    pub fn discover(root: &Path) -> Self {
        let kind = Self::detect_kind(root);
        let patterns = Self::workspace_patterns(root, kind);
        let mut packages = Self::collect_packages(root, &patterns);

        // A single-package repo has no workspace members, so the root manifest
        // is the package. In a monorepo it is only the workspace declaration
        // and collect_packages has already skipped it.
        if packages.is_empty() {
            if let Some(package) = Self::read_package(root) {
                if root.join("package.json").exists() {
                    packages.push(package);
                }
            }
        }

        // Longest root first, so `package_for_file` can take the first match
        // and get the innermost package for nested layouts.
        packages.sort_by(|a, b| {
            b.root
                .components()
                .count()
                .cmp(&a.root.components().count())
                .then_with(|| a.name.cmp(&b.name))
        });

        Self {
            root: root.to_path_buf(),
            kind,
            packages,
        }
    }

    fn detect_kind(root: &Path) -> WorkspaceKind {
        if root.join("nx.json").exists() {
            return WorkspaceKind::Nx;
        }
        if root.join("turbo.json").exists() {
            return WorkspaceKind::Turborepo;
        }
        if root.join("lerna.json").exists() {
            return WorkspaceKind::Lerna;
        }
        if root.join("pnpm-workspace.yaml").exists() || root.join("pnpm-workspace.yml").exists() {
            return WorkspaceKind::Pnpm;
        }
        if Self::npm_workspace_globs(root).is_some() {
            return WorkspaceKind::NpmWorkspaces;
        }
        WorkspaceKind::Single
    }

    /// Globs describing where packages live, from whichever manifest declares them.
    fn workspace_patterns(root: &Path, kind: WorkspaceKind) -> Vec<String> {
        if let Some(globs) = Self::pnpm_workspace_globs(root) {
            if !globs.is_empty() {
                return globs;
            }
        }

        if let Some(globs) = Self::npm_workspace_globs(root) {
            if !globs.is_empty() {
                return globs;
            }
        }

        // Nx projects are not required to be npm workspaces; they are marked by
        // a project.json instead. Fall back to a repo-wide sweep for those.
        if kind == WorkspaceKind::Nx {
            return vec!["**".to_string()];
        }

        // Nothing declared. Probe the layouts these tools conventionally use.
        vec![
            "packages/*".to_string(),
            "apps/*".to_string(),
            "libs/*".to_string(),
        ]
    }

    /// `workspaces` from package.json, either the array or the `{ packages: [] }` form.
    fn npm_workspace_globs(root: &Path) -> Option<Vec<String>> {
        let content = std::fs::read_to_string(root.join("package.json")).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;
        let workspaces = json.get("workspaces")?;

        let array = match workspaces {
            serde_json::Value::Array(a) => a.clone(),
            serde_json::Value::Object(o) => o.get("packages")?.as_array()?.clone(),
            _ => return None,
        };

        Some(
            array
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
        )
    }

    /// The `packages:` list from pnpm-workspace.yaml.
    ///
    /// Parsed by hand rather than pulling in a YAML dependency: the file is a
    /// flat sequence of strings in every real-world case. Anything more exotic
    /// falls through to the conventional-layout probe.
    fn pnpm_workspace_globs(root: &Path) -> Option<Vec<String>> {
        let path = ["pnpm-workspace.yaml", "pnpm-workspace.yml"]
            .iter()
            .map(|f| root.join(f))
            .find(|p| p.exists())?;

        let content = std::fs::read_to_string(path).ok()?;
        let mut globs = Vec::new();
        let mut in_packages = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if trimmed.starts_with("packages:") {
                in_packages = true;
                continue;
            }

            if in_packages {
                if let Some(item) = trimmed.strip_prefix('-') {
                    let value = item.trim().trim_matches('"').trim_matches('\'');
                    if !value.is_empty() {
                        globs.push(value.to_string());
                    }
                } else {
                    // A new top-level key ends the sequence.
                    in_packages = false;
                }
            }
        }

        Some(globs)
    }

    fn collect_packages(root: &Path, patterns: &[String]) -> Vec<Package> {
        let mut packages: Vec<Package> = Vec::new();
        let mut seen: Vec<PathBuf> = Vec::new();

        for pattern in patterns {
            // Negated globs ("!packages/legacy") are a pnpm feature we do not
            // support yet; skipping beats mis-including the excluded package.
            if pattern.starts_with('!') {
                continue;
            }

            for manifest in ["package.json", "project.json"] {
                let full = root.join(pattern).join(manifest);
                let Some(full) = full.to_str() else { continue };

                let Ok(entries) = glob::glob(full) else {
                    continue;
                };

                for entry in entries.flatten() {
                    if entry.components().any(|c| c.as_os_str() == "node_modules") {
                        continue;
                    }

                    let Some(pkg_root) = entry.parent() else {
                        continue;
                    };

                    // The workspace root's own manifest declares the workspace;
                    // it is not a member of it.
                    if pkg_root == root {
                        continue;
                    }

                    if seen.iter().any(|p| p == pkg_root) {
                        continue;
                    }
                    seen.push(pkg_root.to_path_buf());

                    if let Some(package) = Self::read_package(pkg_root) {
                        packages.push(package);
                    }
                }
            }
        }

        packages
    }

    fn read_package(pkg_root: &Path) -> Option<Package> {
        let dir_name = pkg_root.file_name()?.to_string_lossy().to_string();

        // Prefer the npm name; Nx project.json carries a name too.
        for manifest in ["package.json", "project.json"] {
            let path = pkg_root.join(manifest);
            if !path.exists() {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(name) = json.get("name").and_then(|v| v.as_str()) {
                        if !name.is_empty() {
                            return Some(Package {
                                name: name.to_string(),
                                root: pkg_root.to_path_buf(),
                            });
                        }
                    }
                }
            }
        }

        Some(Package {
            name: dir_name,
            root: pkg_root.to_path_buf(),
        })
    }

    /// The innermost package containing `file`, if any.
    pub fn package_for_file(&self, file: &Path) -> Option<&Package> {
        // `packages` is sorted deepest-first, so the first containing package wins.
        self.packages.iter().find(|p| file.starts_with(&p.root))
    }

    /// Distinct package names owning the given files, sorted.
    pub fn packages_for_files(&self, files: &[PathBuf]) -> Vec<String> {
        let mut names: Vec<String> = files
            .iter()
            .filter_map(|f| self.package_for_file(f))
            .map(|p| p.name.clone())
            .collect();

        names.sort();
        names.dedup();
        names
    }

    pub fn is_monorepo(&self) -> bool {
        self.kind != WorkspaceKind::Single && self.packages.len() > 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn detects_turborepo_and_reads_npm_workspaces() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        write(&root.join("turbo.json"), r#"{"tasks":{}}"#);
        write(
            &root.join("package.json"),
            r#"{"name":"root","workspaces":["packages/*","apps/*"]}"#,
        );
        write(
            &root.join("packages/ui/package.json"),
            r#"{"name":"@acme/ui"}"#,
        );
        write(
            &root.join("apps/web/package.json"),
            r#"{"name":"@acme/web"}"#,
        );

        let ws = Workspace::discover(root);

        assert_eq!(ws.kind, WorkspaceKind::Turborepo);
        assert_eq!(ws.packages.len(), 2);
        assert!(ws.packages.iter().any(|p| p.name == "@acme/ui"));
        assert!(ws.packages.iter().any(|p| p.name == "@acme/web"));
    }

    #[test]
    fn detects_nx_over_other_markers() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        write(&root.join("nx.json"), r#"{}"#);
        write(&root.join("turbo.json"), r#"{"tasks":{}}"#);
        write(
            &root.join("package.json"),
            r#"{"name":"root","workspaces":["libs/*"]}"#,
        );
        write(&root.join("libs/data/package.json"), r#"{"name":"data"}"#);

        let ws = Workspace::discover(root);
        assert_eq!(ws.kind, WorkspaceKind::Nx);
    }

    #[test]
    fn discovers_nx_projects_without_npm_workspaces() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        write(&root.join("nx.json"), r#"{}"#);
        write(&root.join("package.json"), r#"{"name":"root"}"#);
        write(&root.join("apps/shell/project.json"), r#"{"name":"shell"}"#);
        write(&root.join("libs/ui/project.json"), r#"{"name":"ui"}"#);

        let ws = Workspace::discover(root);

        assert_eq!(ws.kind, WorkspaceKind::Nx);
        assert!(ws.packages.iter().any(|p| p.name == "shell"));
        assert!(ws.packages.iter().any(|p| p.name == "ui"));
    }

    #[test]
    fn parses_pnpm_workspace_globs() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        write(
            &root.join("pnpm-workspace.yaml"),
            "packages:\n  - 'packages/*'\n  - \"apps/*\"\n",
        );
        write(
            &root.join("packages/core/package.json"),
            r#"{"name":"core"}"#,
        );
        write(&root.join("apps/site/package.json"), r#"{"name":"site"}"#);

        let ws = Workspace::discover(root);

        assert_eq!(ws.kind, WorkspaceKind::Pnpm);
        assert_eq!(ws.packages.len(), 2);
    }

    #[test]
    fn maps_files_to_owning_package() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        write(
            &root.join("package.json"),
            r#"{"name":"root","workspaces":["packages/*"]}"#,
        );
        write(
            &root.join("packages/ui/package.json"),
            r#"{"name":"@acme/ui"}"#,
        );
        write(
            &root.join("packages/api/package.json"),
            r#"{"name":"@acme/api"}"#,
        );

        let ws = Workspace::discover(root);
        let files = vec![
            root.join("packages/ui/src/button.ts"),
            root.join("packages/ui/src/card.ts"),
            root.join("packages/api/src/index.ts"),
            root.join("tools/script.ts"),
        ];

        let names = ws.packages_for_files(&files);
        assert_eq!(names, vec!["@acme/api", "@acme/ui"]);
    }

    #[test]
    fn nested_package_wins_over_parent() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        write(
            &root.join("package.json"),
            r#"{"name":"root","workspaces":["packages/*","packages/*/*"]}"#,
        );
        write(
            &root.join("packages/group/package.json"),
            r#"{"name":"group"}"#,
        );
        write(
            &root.join("packages/group/inner/package.json"),
            r#"{"name":"inner"}"#,
        );

        let ws = Workspace::discover(root);
        let file = root.join("packages/group/inner/src/a.ts");

        assert_eq!(ws.package_for_file(&file).unwrap().name, "inner");
    }

    #[test]
    fn single_package_repo_has_no_monorepo_shape() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        write(&root.join("package.json"), r#"{"name":"solo"}"#);

        let ws = Workspace::discover(root);
        assert_eq!(ws.kind, WorkspaceKind::Single);
        assert!(!ws.is_monorepo());
    }

    #[test]
    fn ignores_packages_under_node_modules() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        write(&root.join("nx.json"), r#"{}"#);
        write(&root.join("package.json"), r#"{"name":"root"}"#);
        write(&root.join("libs/ui/package.json"), r#"{"name":"ui"}"#);
        write(
            &root.join("libs/ui/node_modules/dep/package.json"),
            r#"{"name":"dep"}"#,
        );

        let ws = Workspace::discover(root);
        assert!(ws.packages.iter().all(|p| p.name != "dep"));
    }
}

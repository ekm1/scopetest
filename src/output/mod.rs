use std::path::PathBuf;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Paths,
    Json,
    List,
    /// Repeated `--filter=` flags, ready to splice into a `turbo run` invocation.
    Turbo,
    /// Comma-separated project names, ready for `nx run-many -p`.
    Nx,
}

impl OutputFormat {
    /// Whether this format describes packages rather than files.
    ///
    /// `turbo` and `nx` only make sense as package lists, so selecting either
    /// implies `--packages` without the user having to pass it too.
    pub fn implies_packages(&self) -> bool {
        matches!(self, OutputFormat::Turbo | OutputFormat::Nx)
    }
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "paths" | "jest" | "vitest" => Ok(OutputFormat::Paths),
            "json" => Ok(OutputFormat::Json),
            "list" => Ok(OutputFormat::List),
            "turbo" | "turborepo" => Ok(OutputFormat::Turbo),
            "nx" => Ok(OutputFormat::Nx),
            _ => Err(format!(
                "Unknown format: {}. Use: paths, list, json, turbo, nx",
                s
            )),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AffectedStats {
    pub total_tests: usize,
    pub affected_tests: usize,
    pub total_sources: usize,
    pub affected_sources: usize,
}

#[derive(Debug, Serialize)]
pub struct PackageStats {
    pub total_packages: usize,
    pub affected_packages: usize,
}

#[derive(Debug, Serialize)]
pub struct PackageJsonOutput {
    pub packages: Vec<String>,
    pub stats: PackageStats,
}

#[derive(Debug, Serialize)]
pub struct JsonOutput {
    pub tests: Vec<String>,
    pub sources: Vec<String>,
    pub stats: AffectedStats,
}

pub struct OutputFormatter;

impl OutputFormatter {
    /// Space-separated paths for test runners
    pub fn format_paths(files: &[PathBuf]) -> String {
        files
            .iter()
            .filter_map(|p| p.to_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn format_json(
        tests: &[PathBuf],
        sources: &[PathBuf],
        total_tests: usize,
        total_sources: usize,
    ) -> String {
        let output = JsonOutput {
            tests: tests.iter().filter_map(|p| p.to_str()).map(String::from).collect(),
            sources: sources.iter().filter_map(|p| p.to_str()).map(String::from).collect(),
            stats: AffectedStats {
                total_tests,
                affected_tests: tests.len(),
                total_sources,
                affected_sources: sources.len(),
            },
        };

        serde_json::to_string_pretty(&output).unwrap_or_default()
    }

    /// Render affected package names in the requested shape.
    pub fn format_packages(
        packages: &[String],
        format: OutputFormat,
        total_packages: usize,
    ) -> String {
        match format {
            OutputFormat::Paths => packages.join(" "),
            OutputFormat::List => packages.join("\n"),
            OutputFormat::Nx => packages.join(","),
            OutputFormat::Turbo => packages
                .iter()
                .map(|p| format!("--filter={}", p))
                .collect::<Vec<_>>()
                .join(" "),
            OutputFormat::Json => {
                let output = PackageJsonOutput {
                    packages: packages.to_vec(),
                    stats: PackageStats {
                        total_packages,
                        affected_packages: packages.len(),
                    },
                };
                serde_json::to_string_pretty(&output).unwrap_or_default()
            }
        }
    }

    pub fn format_list(files: &[PathBuf]) -> String {
        files
            .iter()
            .filter_map(|p| p.to_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_empty() {
        let pattern = OutputFormatter::format_paths(&[]);
        assert_eq!(pattern, "");
    }

    #[test]
    fn test_paths_single() {
        let tests = vec![PathBuf::from("src/foo.spec.ts")];
        let pattern = OutputFormatter::format_paths(&tests);
        assert_eq!(pattern, "src/foo.spec.ts");
    }

    #[test]
    fn test_paths_multiple() {
        let tests = vec![
            PathBuf::from("src/foo.spec.ts"),
            PathBuf::from("src/bar.test.ts"),
        ];
        let pattern = OutputFormatter::format_paths(&tests);
        assert_eq!(pattern, "src/foo.spec.ts src/bar.test.ts");
    }

    #[test]
    fn test_format_list() {
        let files = vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")];
        let list = OutputFormatter::format_list(&files);
        assert_eq!(list, "src/a.ts\nsrc/b.ts");
    }

    #[test]
    fn test_format_packages_turbo() {
        let pkgs = vec!["@acme/ui".to_string(), "@acme/web".to_string()];
        let out = OutputFormatter::format_packages(&pkgs, OutputFormat::Turbo, 5);
        assert_eq!(out, "--filter=@acme/ui --filter=@acme/web");
    }

    #[test]
    fn test_format_packages_nx() {
        let pkgs = vec!["shell".to_string(), "ui".to_string()];
        let out = OutputFormatter::format_packages(&pkgs, OutputFormat::Nx, 5);
        assert_eq!(out, "shell,ui");
    }

    #[test]
    fn test_format_packages_empty() {
        let out = OutputFormatter::format_packages(&[], OutputFormat::Turbo, 3);
        assert_eq!(out, "");
    }

    #[test]
    fn test_format_packages_json() {
        let pkgs = vec!["a".to_string()];
        let out = OutputFormatter::format_packages(&pkgs, OutputFormat::Json, 4);
        assert!(out.contains("\"packages\""));
        assert!(out.contains("\"total_packages\": 4"));
        assert!(out.contains("\"affected_packages\": 1"));
    }

    #[test]
    fn test_package_formats_parse() {
        assert!(matches!("turbo".parse::<OutputFormat>(), Ok(OutputFormat::Turbo)));
        assert!(matches!("turborepo".parse::<OutputFormat>(), Ok(OutputFormat::Turbo)));
        assert!(matches!("nx".parse::<OutputFormat>(), Ok(OutputFormat::Nx)));
        assert!(OutputFormat::Turbo.implies_packages());
        assert!(OutputFormat::Nx.implies_packages());
        assert!(!OutputFormat::Paths.implies_packages());
    }

    #[test]
    fn test_format_parse() {
        assert!(matches!("paths".parse::<OutputFormat>(), Ok(OutputFormat::Paths)));
        assert!(matches!("jest".parse::<OutputFormat>(), Ok(OutputFormat::Paths)));
        assert!(matches!("vitest".parse::<OutputFormat>(), Ok(OutputFormat::Paths)));
        assert!(matches!("list".parse::<OutputFormat>(), Ok(OutputFormat::List)));
        assert!(matches!("json".parse::<OutputFormat>(), Ok(OutputFormat::Json)));
    }
}

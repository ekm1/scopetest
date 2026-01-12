use std::path::PathBuf;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Paths,
    Json,
    List,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "paths" | "jest" | "vitest" => Ok(OutputFormat::Paths),
            "json" => Ok(OutputFormat::Json),
            "list" => Ok(OutputFormat::List),
            _ => Err(format!("Unknown format: {}. Use: paths, list, json", s)),
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
    fn test_format_parse() {
        assert!(matches!("paths".parse::<OutputFormat>(), Ok(OutputFormat::Paths)));
        assert!(matches!("jest".parse::<OutputFormat>(), Ok(OutputFormat::Paths)));
        assert!(matches!("vitest".parse::<OutputFormat>(), Ok(OutputFormat::Paths)));
        assert!(matches!("list".parse::<OutputFormat>(), Ok(OutputFormat::List)));
        assert!(matches!("json".parse::<OutputFormat>(), Ok(OutputFormat::Json)));
    }
}

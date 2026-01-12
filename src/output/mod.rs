use std::path::PathBuf;
use serde::Serialize;
use regex::escape;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Jest,
    Json,
    List,
    Coverage,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "jest" => Ok(OutputFormat::Jest),
            "json" => Ok(OutputFormat::Json),
            "list" => Ok(OutputFormat::List),
            "coverage" => Ok(OutputFormat::Coverage),
            _ => Err(format!("Unknown format: {}", s)),
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

#[derive(Debug, Serialize)]
pub struct CoverageThreshold {
    pub branches: u8,
    pub lines: u8,
    pub functions: u8,
    pub statements: u8,
}

impl Default for CoverageThreshold {
    fn default() -> Self {
        Self { branches: 80, lines: 80, functions: 80, statements: 80 }
    }
}

pub struct OutputFormatter;

impl OutputFormatter {
    pub fn format_jest_pattern(tests: &[PathBuf]) -> String {
        if tests.is_empty() {
            return "^$".to_string();
        }

        tests
            .iter()
            .filter_map(|p| p.file_name())
            .filter_map(|n| n.to_str())
            .map(|n| escape(n))
            .collect::<Vec<_>>()
            .join("|")
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

    pub fn format_coverage_from(sources: &[PathBuf]) -> String {
        sources
            .iter()
            .filter_map(|p| p.to_str())
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn format_coverage_threshold(sources: &[PathBuf], threshold: CoverageThreshold) -> String {
        use std::collections::HashMap;

        let mut coverage_threshold: HashMap<String, CoverageThreshold> = HashMap::new();

        for source in sources {
            if let Some(path_str) = source.to_str() {
                coverage_threshold.insert(path_str.to_string(), CoverageThreshold {
                    branches: threshold.branches,
                    lines: threshold.lines,
                    functions: threshold.functions,
                    statements: threshold.statements,
                });
            }
        }

        #[derive(Serialize)]
        struct ThresholdConfig {
            #[serde(rename = "coverageThreshold")]
            coverage_threshold: HashMap<String, CoverageThreshold>,
        }

        let config = ThresholdConfig { coverage_threshold };
        serde_json::to_string_pretty(&config).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jest_pattern_empty() {
        let pattern = OutputFormatter::format_jest_pattern(&[]);
        assert_eq!(pattern, "^$");
    }

    #[test]
    fn test_jest_pattern_single() {
        let tests = vec![PathBuf::from("/src/foo.spec.ts")];
        let pattern = OutputFormatter::format_jest_pattern(&tests);
        assert_eq!(pattern, "foo\\.spec\\.ts");
    }

    #[test]
    fn test_jest_pattern_multiple() {
        let tests = vec![
            PathBuf::from("/src/foo.spec.ts"),
            PathBuf::from("/src/bar.test.ts"),
        ];
        let pattern = OutputFormatter::format_jest_pattern(&tests);
        assert!(pattern.contains("foo\\.spec\\.ts"));
        assert!(pattern.contains("bar\\.test\\.ts"));
        assert!(pattern.contains("|"));
    }

    #[test]
    fn test_format_list() {
        let files = vec![PathBuf::from("/src/a.ts"), PathBuf::from("/src/b.ts")];
        let list = OutputFormatter::format_list(&files);
        assert!(list.contains("/src/a.ts"));
        assert!(list.contains("/src/b.ts"));
    }
}

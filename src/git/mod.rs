use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GitError {
    #[error("Not a git repository")]
    NotARepo,
    #[error("Git command failed: {0}")]
    CommandFailed(String),
    #[error("Invalid base reference: {0}")]
    InvalidRef(String),
}

/// Set of changed files from git diff
#[derive(Debug, Default)]
pub struct ChangeSet {
    /// Modified files
    pub modified: Vec<PathBuf>,
    /// Added files
    pub added: Vec<PathBuf>,
    /// Deleted files
    pub deleted: Vec<PathBuf>,
    /// Renamed files (old path, new path)
    pub renamed: Vec<(PathBuf, PathBuf)>,
}

impl ChangeSet {
    /// Get all changed file paths (excluding deleted)
    pub fn all_changed(&self) -> Vec<PathBuf> {
        let mut result = Vec::new();
        result.extend(self.modified.clone());
        result.extend(self.added.clone());
        for (old, new) in &self.renamed {
            result.push(old.clone());
            result.push(new.clone());
        }
        result
    }

    /// Check if there are any changes
    pub fn is_empty(&self) -> bool {
        self.modified.is_empty()
            && self.added.is_empty()
            && self.deleted.is_empty()
            && self.renamed.is_empty()
    }
}

/// Detects file changes using git
pub struct GitChangeDetector {
    repo_root: PathBuf,
}

impl GitChangeDetector {
    pub fn new(repo_root: PathBuf) -> Result<Self, GitError> {
        // Verify it's a git repo
        let output = Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(&repo_root)
            .output()
            .map_err(|e| GitError::CommandFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(GitError::NotARepo);
        }

        Ok(Self { repo_root })
    }

    /// Get the default base reference (usually main or master)
    pub fn get_default_base(&self) -> String {
        // Try main first, then master
        let output = Command::new("git")
            .args(["rev-parse", "--verify", "main"])
            .current_dir(&self.repo_root)
            .output();

        if output.map(|o| o.status.success()).unwrap_or(false) {
            return "main".to_string();
        }

        "master".to_string()
    }

    /// Detect changes compared to a base reference
    pub fn detect_changes(&self, base_ref: &str) -> Result<ChangeSet, GitError> {
        // Verify the ref exists
        let verify = Command::new("git")
            .args(["rev-parse", "--verify", base_ref])
            .current_dir(&self.repo_root)
            .output()
            .map_err(|e| GitError::CommandFailed(e.to_string()))?;

        if !verify.status.success() {
            return Err(GitError::InvalidRef(base_ref.to_string()));
        }

        // Get diff with name-status
        let output = Command::new("git")
            .args(["diff", "--name-status", base_ref])
            .current_dir(&self.repo_root)
            .output()
            .map_err(|e| GitError::CommandFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(GitError::CommandFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        self.parse_diff_output(&stdout)
    }

    /// Parse git diff --name-status output
    fn parse_diff_output(&self, output: &str) -> Result<ChangeSet, GitError> {
        let mut changeset = ChangeSet::default();

        for line in output.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.is_empty() {
                continue;
            }

            let status = parts[0];
            match status.chars().next() {
                Some('M') => {
                    if parts.len() >= 2 {
                        changeset.modified.push(self.repo_root.join(parts[1]));
                    }
                }
                Some('A') => {
                    if parts.len() >= 2 {
                        changeset.added.push(self.repo_root.join(parts[1]));
                    }
                }
                Some('D') => {
                    if parts.len() >= 2 {
                        changeset.deleted.push(self.repo_root.join(parts[1]));
                    }
                }
                Some('R') => {
                    // Renamed: R100\told\tnew
                    if parts.len() >= 3 {
                        changeset.renamed.push((
                            self.repo_root.join(parts[1]),
                            self.repo_root.join(parts[2]),
                        ));
                    }
                }
                _ => {}
            }
        }

        Ok(changeset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_diff_output() {
        let detector = GitChangeDetector {
            repo_root: PathBuf::from("/repo"),
        };

        let output = "M\tsrc/foo.ts\nA\tsrc/bar.ts\nD\tsrc/old.ts\nR100\tsrc/a.ts\tsrc/b.ts";
        let changeset = detector.parse_diff_output(output).unwrap();

        assert_eq!(changeset.modified.len(), 1);
        assert_eq!(changeset.added.len(), 1);
        assert_eq!(changeset.deleted.len(), 1);
        assert_eq!(changeset.renamed.len(), 1);
    }
}

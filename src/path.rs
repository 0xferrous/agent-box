use eyre::{Result, eyre};
use std::path::{Path, PathBuf};

use crate::config::Config;

/// Type of workspace (git or jj)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceType {
    Git,
    Jj,
}

/// A relative path identifier for a repository that can be resolved
/// against different base directories (git_dir, jj_dir, workspace_dir, etc.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoIdentifier {
    /// The relative path from any base directory (e.g., "myproject" or "work/project")
    relative_path: PathBuf,
}

impl RepoIdentifier {
    /// Create from a path within base_repo_dir
    pub fn from_repo_path(config: &Config, full_path: &Path) -> Result<Self> {
        let relative_path = calculate_relative_path(&config.base_repo_dir, full_path)?;
        Ok(Self { relative_path })
    }

    /// Create from a path within git_dir (bare repo location)
    pub fn from_git_path(config: &Config, full_path: &Path) -> Result<Self> {
        let relative_path = calculate_relative_path(&config.git_dir, full_path)?;
        Ok(Self { relative_path })
    }

    /// Create from a path within jj_dir
    pub fn from_jj_path(config: &Config, full_path: &Path) -> Result<Self> {
        let relative_path = calculate_relative_path(&config.jj_dir, full_path)?;
        Ok(Self { relative_path })
    }

    /// Create from a workspace path, auto-detecting type
    /// Returns (identifier, workspace_type, session_name)
    pub fn from_workspace_path(
        config: &Config,
        full_path: &Path,
    ) -> Result<(Self, WorkspaceType, String)> {
        // Strip workspace_dir to get relative portion
        let relative = calculate_relative_path(&config.workspace_dir, full_path)?;

        // First component should be "git" or "jj"
        let mut components: Vec<_> = relative.components().collect();
        if components.is_empty() {
            return Err(eyre!(
                "Workspace path is empty after stripping workspace_dir"
            ));
        }

        let workspace_type = match components[0].as_os_str().to_str() {
            Some("git") => WorkspaceType::Git,
            Some("jj") => WorkspaceType::Jj,
            Some(other) => {
                return Err(eyre!(
                    "Expected 'git' or 'jj' as first component, got: {}",
                    other
                ));
            }
            None => return Err(eyre!("Invalid UTF-8 in workspace path component")),
        };

        // Remove first component (git/jj)
        components.remove(0);

        if components.is_empty() {
            return Err(eyre!("No path after workspace type in workspace path"));
        }

        // Last component is the session name
        let session = components
            .pop()
            .ok_or_else(|| eyre!("No session name in workspace path"))?
            .as_os_str()
            .to_str()
            .ok_or_else(|| eyre!("Session name contains invalid UTF-8"))?
            .to_string();

        // What's left is the relative_path
        let relative_path: PathBuf = components.iter().collect();

        Ok((Self { relative_path }, workspace_type, session))
    }

    /// Get the full path in base_repo_dir
    pub fn repo_path(&self, config: &Config) -> PathBuf {
        config.base_repo_dir.join(&self.relative_path)
    }

    /// Get the full path in git_dir (bare repo location)
    pub fn git_path(&self, config: &Config) -> PathBuf {
        config.git_dir.join(&self.relative_path)
    }

    /// Get the full path in jj_dir
    pub fn jj_path(&self, config: &Config) -> PathBuf {
        config.jj_dir.join(&self.relative_path)
    }

    /// Get the full path for a git workspace with given session
    pub fn git_workspace_path(&self, config: &Config, session: &str) -> PathBuf {
        config
            .workspace_dir
            .join("git")
            .join(&self.relative_path)
            .join(session)
    }

    /// Get the full path for a jj workspace with given session
    pub fn jj_workspace_path(&self, config: &Config, session: &str) -> PathBuf {
        config
            .workspace_dir
            .join("jj")
            .join(&self.relative_path)
            .join(session)
    }

    /// Get the underlying relative path
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }
}

/// Expand path with ~ support and canonicalize
pub fn expand_path(path: &Path) -> Result<PathBuf> {
    use eyre::Context;

    let expanded = if path.starts_with("~") {
        let home = std::env::var("HOME")
            .wrap_err("Failed to get HOME environment variable when expanding ~")?;
        PathBuf::from(home).join(path.strip_prefix("~")?)
    } else {
        path.to_owned()
    };

    // Canonicalize to get absolute path and resolve symlinks
    expanded
        .canonicalize()
        .wrap_err_with(|| format!("Failed to canonicalize path: {}", expanded.display()))
}

/// Calculate bare repo path from base_repo_dir and git_dir
pub fn calculate_bare_repo_path(
    base_repo_dir: &Path,
    current_repo_path: &Path,
    git_dir: &Path,
) -> Result<PathBuf> {
    let relative_path = current_repo_path.strip_prefix(base_repo_dir).map_err(|_| {
        eyre!(
            "Repository {} is not under base_repo_dir {}",
            current_repo_path.display(),
            base_repo_dir.display()
        )
    })?;

    let target_path = git_dir.join(relative_path);
    Ok(target_path)
}

/// Convert Path to str with a descriptive error message
pub fn path_to_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| eyre!("Path contains invalid UTF-8: {}", path.display()))
}

/// Calculate relative path from base directory to full path
pub fn calculate_relative_path(base_dir: &Path, full_path: &Path) -> Result<PathBuf> {
    full_path
        .strip_prefix(base_dir)
        .map(|p| p.to_path_buf())
        .map_err(|_| {
            eyre!(
                "Path {} is not under base directory {}",
                full_path.display(),
                base_dir.display()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_config() -> Config {
        use crate::config::AgentConfig;

        Config {
            base_repo_dir: PathBuf::from("/home/user/repos"),
            git_dir: PathBuf::from("/mnt/git"),
            jj_dir: PathBuf::from("/mnt/jj"),
            workspace_dir: PathBuf::from("/mnt/workspace"),
            agent: AgentConfig {
                user: "testuser".to_string(),
                group: "testgroup".to_string(),
            },
        }
    }

    #[test]
    fn test_repo_identifier_from_repo_path() {
        let config = make_test_config();
        let full_path = PathBuf::from("/home/user/repos/myproject");

        let id = RepoIdentifier::from_repo_path(&config, &full_path).unwrap();
        assert_eq!(id.relative_path(), Path::new("myproject"));
    }

    #[test]
    fn test_repo_identifier_from_git_path() {
        let config = make_test_config();
        let full_path = PathBuf::from("/mnt/git/work/project");

        let id = RepoIdentifier::from_git_path(&config, &full_path).unwrap();
        assert_eq!(id.relative_path(), Path::new("work/project"));
    }

    #[test]
    fn test_repo_identifier_from_jj_path() {
        let config = make_test_config();
        let full_path = PathBuf::from("/mnt/jj/myproject");

        let id = RepoIdentifier::from_jj_path(&config, &full_path).unwrap();
        assert_eq!(id.relative_path(), Path::new("myproject"));
    }

    #[test]
    fn test_repo_identifier_from_workspace_path_jj() {
        let config = make_test_config();
        let full_path = PathBuf::from("/mnt/workspace/jj/work/project/session1");

        let (id, workspace_type, session) =
            RepoIdentifier::from_workspace_path(&config, &full_path).unwrap();
        assert_eq!(id.relative_path(), Path::new("work/project"));
        assert_eq!(workspace_type, WorkspaceType::Jj);
        assert_eq!(session, "session1");
    }

    #[test]
    fn test_repo_identifier_from_workspace_path_git() {
        let config = make_test_config();
        let full_path = PathBuf::from("/mnt/workspace/git/myproject/my-session");

        let (id, workspace_type, session) =
            RepoIdentifier::from_workspace_path(&config, &full_path).unwrap();
        assert_eq!(id.relative_path(), Path::new("myproject"));
        assert_eq!(workspace_type, WorkspaceType::Git);
        assert_eq!(session, "my-session");
    }

    #[test]
    fn test_repo_identifier_from_workspace_path_invalid_type() {
        let config = make_test_config();
        let full_path = PathBuf::from("/mnt/workspace/invalid/myproject/session1");

        let result = RepoIdentifier::from_workspace_path(&config, &full_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_repo_identifier_path_builders() {
        let config = make_test_config();
        let id = RepoIdentifier {
            relative_path: PathBuf::from("work/project"),
        };

        assert_eq!(
            id.repo_path(&config),
            PathBuf::from("/home/user/repos/work/project")
        );
        assert_eq!(id.git_path(&config), PathBuf::from("/mnt/git/work/project"));
        assert_eq!(id.jj_path(&config), PathBuf::from("/mnt/jj/work/project"));
        assert_eq!(
            id.git_workspace_path(&config, "session1"),
            PathBuf::from("/mnt/workspace/git/work/project/session1")
        );
        assert_eq!(
            id.jj_workspace_path(&config, "session2"),
            PathBuf::from("/mnt/workspace/jj/work/project/session2")
        );
    }

    #[test]
    fn test_repo_identifier_roundtrip_repo_path() {
        let config = make_test_config();
        let original_path = PathBuf::from("/home/user/repos/myproject");

        let id = RepoIdentifier::from_repo_path(&config, &original_path).unwrap();
        let reconstructed = id.repo_path(&config);

        assert_eq!(original_path, reconstructed);
    }

    #[test]
    fn test_repo_identifier_roundtrip_git_path() {
        let config = make_test_config();
        let original_path = PathBuf::from("/mnt/git/work/project");

        let id = RepoIdentifier::from_git_path(&config, &original_path).unwrap();
        let reconstructed = id.git_path(&config);

        assert_eq!(original_path, reconstructed);
    }

    #[test]
    fn test_repo_identifier_roundtrip_workspace_path() {
        let config = make_test_config();
        let original_path = PathBuf::from("/mnt/workspace/jj/work/project/session1");

        let (id, workspace_type, session) =
            RepoIdentifier::from_workspace_path(&config, &original_path).unwrap();
        let reconstructed = match workspace_type {
            WorkspaceType::Jj => id.jj_workspace_path(&config, &session),
            WorkspaceType::Git => id.git_workspace_path(&config, &session),
        };

        assert_eq!(original_path, reconstructed);
    }

    #[test]
    fn test_calculate_bare_repo_path_simple() {
        let base = PathBuf::from("/home/user/repos");
        let current = PathBuf::from("/home/user/repos/myproject");
        let git_dir = PathBuf::from("/mnt/git-storage");

        let result = calculate_bare_repo_path(&base, &current, &git_dir).unwrap();
        assert_eq!(result, PathBuf::from("/mnt/git-storage/myproject"));
    }

    #[test]
    fn test_calculate_bare_repo_path_nested() {
        let base = PathBuf::from("/home/user/repos");
        let current = PathBuf::from("/home/user/repos/work/project/subdir");
        let git_dir = PathBuf::from("/mnt/git-storage");

        let result = calculate_bare_repo_path(&base, &current, &git_dir).unwrap();
        assert_eq!(
            result,
            PathBuf::from("/mnt/git-storage/work/project/subdir")
        );
    }

    #[test]
    fn test_calculate_bare_repo_path_not_under_base() {
        let base = PathBuf::from("/home/user/repos");
        let current = PathBuf::from("/somewhere/else/project");
        let git_dir = PathBuf::from("/mnt/git-storage");

        let result = calculate_bare_repo_path(&base, &current, &git_dir);
        assert!(result.is_err());
    }
}

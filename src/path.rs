use eyre::{Result, eyre};
use std::path::{Path, PathBuf};

use crate::config::Config;

/// Type of workspace (git or jj)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkspaceType {
    Git,
    Jj,
}

/// Information about a git worktree
#[derive(Debug, Clone)]
pub struct GitWorktreeInfo {
    pub path: PathBuf,
    pub id: Option<String>,
    pub is_main: bool,
    pub is_locked: bool,
}

/// A relative path identifier for a repository that can be resolved
/// against different base directories (git_dir, jj_dir, workspace_dir, etc.)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RepoIdentifier {
    /// The relative path from any base directory (e.g., "myproject" or "work/project")
    pub relative_path: PathBuf,
}

impl RepoIdentifier {
    /// Create from a path within base_repo_dir
    pub fn from_repo_path(config: &Config, full_path: &Path) -> Result<Self> {
        let relative_path = calculate_relative_path(&config.base_repo_dir, full_path)?;
        Ok(Self { relative_path })
    }

    /// Get the full path in base_repo_dir (source repo location)
    pub fn source_path(&self, config: &Config) -> PathBuf {
        config.base_repo_dir.join(&self.relative_path)
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

    pub fn workspace_path(&self, config: &Config, wtype: WorkspaceType, session: &str) -> PathBuf {
        match wtype {
            WorkspaceType::Git => self.git_workspace_path(config, session),
            WorkspaceType::Jj => self.jj_workspace_path(config, session),
        }
    }

    /// Get the underlying relative path
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Find all repository identifiers matching a search string.
    /// The search string can be a partial path like "fr/agent-box" or "agent-box".
    /// Returns all matching RepoIdentifiers.
    pub fn find_matching(config: &Config, search: &str) -> Result<Vec<Self>> {
        let search_path = Path::new(search);

        if !config.base_repo_dir.exists() {
            return Ok(Vec::new());
        }

        let mut matches = Vec::new();

        let is_repo = |path: &Path| path.join(".git").exists() || path.join(".jj").exists();

        // Walk the base_repo_dir to find all repos (with .git or .jj)
        // Skip descending into directories that are already repos
        let walker = walkdir::WalkDir::new(&config.base_repo_dir)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let path = e.path();
                // Always allow the base dir itself
                if path == config.base_repo_dir {
                    return true;
                }
                // Skip .git and .jj directories
                if let Some(name) = path.file_name() {
                    if name == ".git" || name == ".jj" {
                        return false;
                    }
                }
                // If parent is a repo, don't descend into children
                if let Some(parent) = path.parent() {
                    if parent != config.base_repo_dir && is_repo(parent) {
                        return false;
                    }
                }
                true
            });

        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Check if this looks like a repo (has .git or .jj directory)
            if !path.is_dir() || !is_repo(path) {
                continue;
            }

            // Get the relative path from base_repo_dir
            let Ok(relative_path) = path.strip_prefix(&config.base_repo_dir) else {
                continue;
            };

            // Check if this matches the search string
            // Match if the relative path ends with the search path or equals it
            if relative_path == search_path || relative_path.ends_with(search_path) {
                matches.push(Self {
                    relative_path: relative_path.to_path_buf(),
                });
            }
        }

        Ok(matches)
    }

    /// Helper function to discover repositories in a directory based on a filter predicate
    /// Stops descending into directories that are already repos.
    fn discover_repos_in_dir<F>(base_dir: &Path, is_repo: F) -> Result<Vec<Self>>
    where
        F: Fn(&Path) -> bool + Copy,
    {
        let mut repos = Vec::new();

        if !base_dir.exists() {
            return Ok(repos);
        }

        // Walk the directory to find all repos matching the predicate
        // Skip descending into directories that are already repos
        let walker = walkdir::WalkDir::new(base_dir)
            .follow_links(false)
            .into_iter()
            .filter_entry(move |e| {
                let path = e.path();
                // Always allow the base dir itself
                if path == base_dir {
                    return true;
                }
                // Skip .git and .jj directories
                if let Some(name) = path.file_name() {
                    if name == ".git" || name == ".jj" {
                        return false;
                    }
                }
                // If parent is a repo, don't descend into children
                if let Some(parent) = path.parent() {
                    if parent != base_dir && is_repo(parent) {
                        return false;
                    }
                }
                true
            });

        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();

            if !path.is_dir() || !is_repo(path) {
                continue;
            }

            // Get the relative path from base_dir
            let Ok(relative_path) = path.strip_prefix(base_dir) else {
                continue;
            };

            repos.push(Self {
                relative_path: relative_path.to_path_buf(),
            });
        }

        Ok(repos)
    }

    /// Discover all repositories in the base_repo_dir.
    /// Returns a vector of RepoIdentifiers for all repositories found (with .git or .jj).
    pub fn discover_repo_ids(config: &Config) -> Result<Vec<Self>> {
        Self::discover_repos_in_dir(&config.base_repo_dir, |path| {
            path.join(".git").exists() || path.join(".jj").exists()
        })
    }

    /// Get all JJ workspaces for this repository using JJ's workspace tracking
    pub fn jj_workspaces(&self, config: &Config) -> Result<Vec<String>> {
        let workspace_path = self.source_path(config);

        if !workspace_path.exists() {
            return Ok(Vec::new());
        }

        if !workspace_path.join(".jj").exists() {
            return Ok(Vec::new());
        }

        // Load the workspace to access the repo
        let jj_config = jj_lib::config::StackedConfig::with_defaults();
        let user_settings = jj_lib::settings::UserSettings::from_config(jj_config)?;
        let store_factories = jj_lib::repo::StoreFactories::default();
        let working_copy_factories = jj_lib::workspace::default_working_copy_factories();

        let workspace = jj_lib::workspace::Workspace::load(
            &user_settings,
            &workspace_path,
            &store_factories,
            &working_copy_factories,
        )?;

        let repo = workspace.repo_loader().load_at_head()?;

        // Get workspace names from the View's wc_commit_ids
        let workspace_names: Vec<String> = repo
            .view()
            .wc_commit_ids()
            .keys()
            .map(|name| name.as_str().to_owned())
            .collect();

        Ok(workspace_names)
    }

    /// Get all git worktrees for this repository
    pub fn git_worktrees(&self, config: &Config) -> Result<Vec<GitWorktreeInfo>> {
        let repo_path = self.source_path(config);

        if !repo_path.exists() {
            return Ok(Vec::new());
        }

        let repo = gix::open(&repo_path)?;
        let mut worktrees = Vec::new();

        // Add main worktree if it exists
        if let Some(wt) = repo.worktree() {
            worktrees.push(GitWorktreeInfo {
                path: wt.base().to_path_buf(),
                id: None,
                is_main: true,
                is_locked: false,
            });
        }

        // Add all linked worktrees
        for proxy in repo.worktrees()? {
            let path = proxy.base()?;
            let id = proxy.id().to_string();
            let is_locked = proxy.is_locked();

            worktrees.push(GitWorktreeInfo {
                path,
                id: Some(id),
                is_main: false,
                is_locked,
            });
        }

        Ok(worktrees)
    }
}

/// Expand path with ~ support and canonicalize if it exists
pub fn expand_path(path: &Path) -> Result<PathBuf> {
    use eyre::Context;

    let expanded = if path.starts_with("~") {
        let home = std::env::var("HOME")
            .wrap_err("Failed to get HOME environment variable when expanding ~")?;
        PathBuf::from(home).join(path.strip_prefix("~")?)
    } else {
        path.to_owned()
    };

    // Canonicalize to get absolute path and resolve symlinks if path exists
    // Otherwise just return the expanded path (useful for init command)
    if expanded.exists() {
        expanded
            .canonicalize()
            .wrap_err_with(|| format!("Failed to canonicalize path: {}", expanded.display()))
    } else {
        // For non-existent paths, make absolute if relative
        if expanded.is_relative() {
            let current_dir =
                std::env::current_dir().wrap_err("Failed to get current directory")?;
            Ok(current_dir.join(expanded))
        } else {
            Ok(expanded)
        }
    }
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
        use crate::config::DockerConfig;

        Config {
            base_repo_dir: PathBuf::from("/home/user/repos"),
            workspace_dir: PathBuf::from("/mnt/workspace"),
            docker: DockerConfig {
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
                env: Default::default(),
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
    fn test_repo_identifier_path_builders() {
        let config = make_test_config();
        let id = RepoIdentifier {
            relative_path: PathBuf::from("work/project"),
        };

        assert_eq!(
            id.source_path(&config),
            PathBuf::from("/home/user/repos/work/project")
        );
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
    fn test_find_matching_exact_match() {
        use crate::config::DockerConfig;

        let temp_dir = std::env::temp_dir().join(format!("ab-test-locate-{}", std::process::id()));
        let base_repo_dir = temp_dir.join("repos");

        // Create a mock repo with .git directory
        let repo_path = base_repo_dir.join("fr").join("agent-box");
        std::fs::create_dir_all(repo_path.join(".git")).unwrap();

        let config = Config {
            base_repo_dir: base_repo_dir.clone(),
            workspace_dir: PathBuf::from("/mnt/workspace"),
            docker: DockerConfig {
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
                env: Default::default(),
            },
        };

        // Test exact match
        let matches = RepoIdentifier::find_matching(&config, "fr/agent-box").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].relative_path(), Path::new("fr/agent-box"));

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_find_matching_partial_match() {
        use crate::config::DockerConfig;

        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-locate-partial-{}", std::process::id()));
        let base_repo_dir = temp_dir.join("repos");

        // Create a mock repo with .git directory
        let repo_path = base_repo_dir.join("fr").join("agent-box");
        std::fs::create_dir_all(repo_path.join(".git")).unwrap();

        let config = Config {
            base_repo_dir: base_repo_dir.clone(),
            workspace_dir: PathBuf::from("/mnt/workspace"),
            docker: DockerConfig {
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
                env: Default::default(),
            },
        };

        // Test partial match (searching for "agent-box" should match "fr/agent-box")
        let matches = RepoIdentifier::find_matching(&config, "agent-box").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].relative_path(), Path::new("fr/agent-box"));

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_find_matching_no_match() {
        use crate::config::DockerConfig;

        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-locate-nomatch-{}", std::process::id()));
        let base_repo_dir = temp_dir.join("repos");

        // Create a mock repo with .git directory
        let repo_path = base_repo_dir.join("fr").join("agent-box");
        std::fs::create_dir_all(repo_path.join(".git")).unwrap();

        let config = Config {
            base_repo_dir: base_repo_dir.clone(),
            workspace_dir: PathBuf::from("/mnt/workspace"),
            docker: DockerConfig {
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
                env: Default::default(),
            },
        };

        // Test no match
        let matches = RepoIdentifier::find_matching(&config, "nonexistent").unwrap();
        assert!(matches.is_empty());

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_find_matching_base_repo_dir_not_exists() {
        let config = make_test_config();

        // Test when base_repo_dir doesn't exist
        let matches = RepoIdentifier::find_matching(&config, "anything").unwrap();
        assert!(matches.is_empty());
    }
}

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

/// Represents a workspace with its repository identifier, type, and session name
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Workspace {
    pub repo_id: RepoIdentifier,
    pub workspace_type: WorkspaceType,
    pub session: String,
}

impl Workspace {
    /// Helper function to discover workspaces in a directory based on a filter predicate
    fn discover_workspaces_in_dir<F>(
        base_dir: &Path,
        workspace_type: WorkspaceType,
        is_workspace: F,
    ) -> Result<Vec<Self>>
    where
        F: Fn(&Path) -> bool,
    {
        let mut workspaces = Vec::new();

        if !base_dir.exists() {
            return Ok(workspaces);
        }

        // Walk the directory to find all workspaces matching the predicate
        for entry in walkdir::WalkDir::new(base_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            if !path.is_dir() || !is_workspace(path) {
                continue;
            }

            // Parse the path: base_dir/{repo_path}/{session}
            let Ok(relative) = path.strip_prefix(base_dir) else {
                continue;
            };

            // Split into components
            let components: Vec<_> = relative.components().collect();
            if components.is_empty() {
                continue;
            }

            // Last component is the session name
            let Some(session) = components
                .last()
                .and_then(|c| c.as_os_str().to_str())
                .map(|s| s.to_string())
            else {
                continue;
            };

            // Everything before the last component is the repo path
            let repo_path: PathBuf = components[..components.len() - 1].iter().collect();

            if repo_path.as_os_str().is_empty() {
                continue;
            }

            workspaces.push(Workspace {
                repo_id: RepoIdentifier {
                    relative_path: repo_path,
                },
                workspace_type,
                session,
            });
        }

        Ok(workspaces)
    }

    /// Discover all git worktree workspaces in workspace_dir/git
    pub fn discover_workspaces_git(config: &Config) -> Result<Vec<Self>> {
        let git_workspace_dir = config.workspace_dir.join("git");
        Self::discover_workspaces_in_dir(&git_workspace_dir, WorkspaceType::Git, |path| {
            // Check if this is a git worktree (has .git file)
            path.join(".git").exists()
        })
    }

    /// Discover all JJ workspaces in workspace_dir/jj
    pub fn discover_workspaces_jj(config: &Config) -> Result<Vec<Self>> {
        let jj_workspace_dir = config.workspace_dir.join("jj");
        Self::discover_workspaces_in_dir(&jj_workspace_dir, WorkspaceType::Jj, |path| {
            // Check if this is a jj workspace (has .jj/working_copy directory)
            path.join(".jj").join("working_copy").exists()
        })
    }
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

    /// Try to locate a repository identifier by walking the git_dir and matching against a path-like string.
    /// The search string can be a partial path like "fr/agent-box" or "agent-box".
    /// Returns the first matching RepoIdentifier, or None if no match is found.
    pub fn locate(config: &Config, search: &str) -> Result<Option<Self>> {
        let search_path = Path::new(search);

        if !config.git_dir.exists() {
            return Ok(None);
        }

        // Walk the git_dir to find all bare repos
        for entry in walkdir::WalkDir::new(&config.git_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Check if this looks like a git bare repo (has HEAD and refs/)
            if !path.is_dir() || !path.join("HEAD").exists() || !path.join("refs").is_dir() {
                continue;
            }

            // Get the relative path from git_dir
            let Ok(relative_path) = path.strip_prefix(&config.git_dir) else {
                continue;
            };

            // Check if this matches the search string
            // Match if the relative path ends with the search path or equals it
            if relative_path == search_path || relative_path.ends_with(search_path) {
                return Ok(Some(Self {
                    relative_path: relative_path.to_path_buf(),
                }));
            }
        }

        Ok(None)
    }

    /// Helper function to discover repositories in a directory based on a filter predicate
    fn discover_repos_in_dir<F>(base_dir: &Path, is_repo: F) -> Result<Vec<Self>>
    where
        F: Fn(&Path) -> bool,
    {
        let mut repos = Vec::new();

        if !base_dir.exists() {
            return Ok(repos);
        }

        // Walk the directory to find all repos matching the predicate
        for entry in walkdir::WalkDir::new(base_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
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

    /// Discover all git repositories in the git_dir.
    /// Returns a vector of RepoIdentifiers for all bare git repositories found.
    pub fn discover_git_repo_ids(config: &Config) -> Result<Vec<Self>> {
        Self::discover_repos_in_dir(&config.git_dir, |path| {
            // Check if this looks like a git bare repo:
            // - Has HEAD, refs/, and objects/
            // - Does NOT have commondir (which indicates a worktree)
            path.join("HEAD").exists()
                && path.join("refs").is_dir()
                && path.join("objects").is_dir()
                && !path.join("commondir").exists()
        })
    }

    /// Discover all JJ repositories in the jj_dir.
    /// Returns a vector of RepoIdentifiers for all JJ repositories found.
    pub fn discover_jj_repo_ids(config: &Config) -> Result<Vec<Self>> {
        Self::discover_repos_in_dir(&config.jj_dir, |path| {
            // Check if this looks like a JJ repo (has .jj directory)
            path.join(".jj").is_dir()
        })
    }

    /// Get all JJ workspaces for this repository using JJ's workspace tracking
    pub fn jj_workspaces(&self, config: &Config) -> Result<Vec<String>> {
        let workspace_path = self.jj_path(config);

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
        let bare_repo_path = self.git_path(config);

        if !bare_repo_path.exists() {
            return Ok(Vec::new());
        }

        let bare_repo = gix::open(&bare_repo_path)?;
        let mut worktrees = Vec::new();

        // Add main worktree if it exists
        if let Some(wt) = bare_repo.worktree() {
            worktrees.push(GitWorktreeInfo {
                path: wt.base().to_path_buf(),
                id: None,
                is_main: true,
                is_locked: false,
            });
        }

        // Add all linked worktrees
        for proxy in bare_repo.worktrees()? {
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
            git_dir: PathBuf::from("/mnt/git"),
            jj_dir: PathBuf::from("/mnt/jj"),
            workspace_dir: PathBuf::from("/mnt/workspace"),
            docker: DockerConfig {
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
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
    fn test_locate_exact_match() {
        use crate::config::DockerConfig;

        let temp_dir = std::env::temp_dir().join(format!("ab-test-locate-{}", std::process::id()));
        let git_dir = temp_dir.join("git");

        // Create a mock bare repo structure
        let repo_path = git_dir.join("fr").join("agent-box");
        std::fs::create_dir_all(&repo_path).unwrap();
        std::fs::write(repo_path.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::create_dir(repo_path.join("refs")).unwrap();

        let config = Config {
            base_repo_dir: PathBuf::from("/home/user/repos"),
            git_dir: git_dir.clone(),
            jj_dir: PathBuf::from("/mnt/jj"),
            workspace_dir: PathBuf::from("/mnt/workspace"),
            docker: DockerConfig {
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
            },
        };

        // Test exact match
        let result = RepoIdentifier::locate(&config, "fr/agent-box").unwrap();
        assert!(result.is_some());
        let id = result.unwrap();
        assert_eq!(id.relative_path(), Path::new("fr/agent-box"));

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_locate_partial_match() {
        use crate::config::DockerConfig;

        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-locate-partial-{}", std::process::id()));
        let git_dir = temp_dir.join("git");

        // Create a mock bare repo structure
        let repo_path = git_dir.join("fr").join("agent-box");
        std::fs::create_dir_all(&repo_path).unwrap();
        std::fs::write(repo_path.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::create_dir(repo_path.join("refs")).unwrap();

        let config = Config {
            base_repo_dir: PathBuf::from("/home/user/repos"),
            git_dir: git_dir.clone(),
            jj_dir: PathBuf::from("/mnt/jj"),
            workspace_dir: PathBuf::from("/mnt/workspace"),
            docker: DockerConfig {
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
            },
        };

        // Test partial match (searching for "agent-box" should match "fr/agent-box")
        let result = RepoIdentifier::locate(&config, "agent-box").unwrap();
        assert!(result.is_some());
        let id = result.unwrap();
        assert_eq!(id.relative_path(), Path::new("fr/agent-box"));

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_locate_no_match() {
        use crate::config::DockerConfig;

        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-locate-nomatch-{}", std::process::id()));
        let git_dir = temp_dir.join("git");

        // Create a mock bare repo structure
        let repo_path = git_dir.join("fr").join("agent-box");
        std::fs::create_dir_all(&repo_path).unwrap();
        std::fs::write(repo_path.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::create_dir(repo_path.join("refs")).unwrap();

        let config = Config {
            base_repo_dir: PathBuf::from("/home/user/repos"),
            git_dir: git_dir.clone(),
            jj_dir: PathBuf::from("/mnt/jj"),
            workspace_dir: PathBuf::from("/mnt/workspace"),
            docker: DockerConfig {
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
            },
        };

        // Test no match
        let result = RepoIdentifier::locate(&config, "nonexistent").unwrap();
        assert!(result.is_none());

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_locate_git_dir_not_exists() {
        let config = make_test_config();

        // Test when git_dir doesn't exist
        let result = RepoIdentifier::locate(&config, "anything").unwrap();
        assert!(result.is_none());
    }
}

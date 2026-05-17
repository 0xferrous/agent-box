use eyre::{Result, eyre};
use jj_lib::object_id::ObjectId;
use jj_lib::repo::Repo;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

/// Information about a JJ workspace
#[derive(Debug, Clone)]
pub struct JjWorkspaceInfo {
    pub name: String,
    pub commit_id: String,
    pub description: String,
    pub is_empty: bool,
}

/// A relative path identifier for a repository that can be resolved
/// against different base directories (git_dir, jj_dir, workspace_dir, etc.)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RepoIdentifier {
    /// The relative path from discovery base directory (e.g., "myproject" or "work/project")
    pub relative_path: PathBuf,
    /// Optional discovery base dir that this repo was found under.
    /// When present, source_path() resolves against this directory.
    pub discovery_base_dir: Option<PathBuf>,
}

impl RepoIdentifier {
    /// Create from a path within base_repo_dir
    pub fn from_repo_path(config: &Config, full_path: &Path) -> Result<Self> {
        let relative_path = calculate_relative_path(&config.base_repo_dir, full_path)?;
        Ok(Self {
            relative_path,
            discovery_base_dir: Some(config.base_repo_dir.clone()),
        })
    }

    /// Get the full path in base_repo_dir (source repo location)
    pub fn source_path(&self, config: &Config) -> PathBuf {
        self.discovery_base_dir
            .as_ref()
            .unwrap_or(&config.base_repo_dir)
            .join(&self.relative_path)
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

        // Get all repos, then filter by search
        let all_repos = Self::discover_repo_ids(config)?;

        let matches = all_repos
            .into_iter()
            .filter(|repo| {
                let rel = repo.relative_path();
                rel == search_path || rel.ends_with(search_path)
            })
            .collect();

        Ok(matches)
    }

    /// Helper function to discover repositories in a directory based on a filter predicate
    /// Stops descending into directories that are already repos.
    fn discover_repos_in_dir<F>(
        base_dir: &Path,
        is_repo: F,
        timeout: Duration,
        started_at: Instant,
    ) -> Result<Vec<Self>>
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
                if let Some(name) = path.file_name()
                    && (name == ".git" || name == ".jj")
                {
                    return false;
                }
                // If parent is a repo, don't descend into children
                if let Some(parent) = path.parent()
                    && parent != base_dir
                    && is_repo(parent)
                {
                    return false;
                }
                true
            });

        for entry in walker.filter_map(|e| e.ok()) {
            if started_at.elapsed() > timeout {
                return Err(eyre!(
                    "Repository discovery timed out after {}s while scanning {}. Narrow repo_discovery_dirs or increase repo_discovery_timeout_secs in your config.",
                    timeout.as_secs(),
                    base_dir.display()
                ));
            }
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
                discovery_base_dir: Some(base_dir.to_path_buf()),
            });
        }

        Ok(repos)
    }

    /// Discover all repositories in configured discovery directories.
    /// Returns RepoIdentifiers for all repositories found (with .git or .jj).
    pub fn discover_repo_ids(config: &Config) -> Result<Vec<Self>> {
        let discovery_dirs: Vec<&Path> = if config.repo_discovery_dirs.is_empty() {
            vec![config.base_repo_dir.as_path()]
        } else {
            config
                .repo_discovery_dirs
                .iter()
                .map(PathBuf::as_path)
                .collect()
        };

        let mut seen = BTreeSet::new();
        let mut repos = Vec::new();
        let timeout = Duration::from_secs(config.repo_discovery_timeout_secs);
        let started_at = Instant::now();
        let mut per_dir_timings: Vec<(PathBuf, Duration)> = Vec::new();

        for dir in discovery_dirs {
            let dir_started_at = Instant::now();
            let discovered = Self::discover_repos_in_dir(
                dir,
                |path| path.join(".git").exists() || path.join(".jj").exists(),
                timeout,
                started_at,
            );

            match discovered {
                Ok(found) => {
                    per_dir_timings.push((dir.to_path_buf(), dir_started_at.elapsed()));
                    for repo in found {
                        if seen
                            .insert((repo.discovery_base_dir.clone(), repo.relative_path.clone()))
                        {
                            repos.push(repo);
                        }
                    }
                }
                Err(e) => {
                    let current_elapsed = dir_started_at.elapsed();
                    let mut timing_parts: Vec<String> = per_dir_timings
                        .iter()
                        .map(|(p, d)| format!("{}: {}ms", p.display(), d.as_millis()))
                        .collect();
                    timing_parts.push(format!(
                        "{}: {}ms (timed out)",
                        dir.display(),
                        current_elapsed.as_millis()
                    ));

                    return Err(eyre!(
                        "{}\nPer-directory scan timings: {}",
                        e,
                        timing_parts.join(", ")
                    ));
                }
            }
        }

        Ok(repos)
    }

    /// Get all JJ workspaces for this repository using JJ's workspace tracking
    pub fn jj_workspaces(&self, config: &Config) -> Result<Vec<JjWorkspaceInfo>> {
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

        // Get workspace info from the View's wc_commit_ids
        let mut workspaces = Vec::new();
        for (name, commit_id) in repo.view().wc_commit_ids() {
            let commit = repo.store().get_commit(commit_id).ok();
            let description = commit
                .as_ref()
                .map(|c| c.description().trim().to_string())
                .unwrap_or_default();
            let is_empty = commit
                .as_ref()
                .and_then(|c| c.is_empty(repo.as_ref()).ok())
                .unwrap_or(false);
            workspaces.push(JjWorkspaceInfo {
                name: name.as_str().to_owned(),
                commit_id: commit_id.hex()[..8].to_string(),
                description,
                is_empty,
            });
        }

        Ok(workspaces)
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
        use crate::config::RuntimeConfig;
        use std::collections::HashMap;

        Config {
            base_repo_dir: PathBuf::from("/home/user/repos"),
            repo_discovery_dirs: vec![],
            repo_discovery_timeout_secs: 30,
            workspace_dir: PathBuf::from("/mnt/workspace"),
            default_profile: None,
            profiles: HashMap::new(),
            runtime: RuntimeConfig {
                backend: "podman".to_string(),
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
                skip_mounts: vec![],
                env: Default::default(),
                env_passthrough: vec![],
                ports: Default::default(),
                hosts: Default::default(),
                dns: Default::default(),
            },
            context: String::new(),
            context_path: "/tmp/context".to_string(),
            portal: crate::portal::PortalConfig::default(),
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
            discovery_base_dir: None,
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
        use crate::config::RuntimeConfig;

        let temp_dir = std::env::temp_dir().join(format!("ab-test-locate-{}", std::process::id()));
        let base_repo_dir = temp_dir.join("repos");

        // Create a mock repo with .git directory
        let repo_path = base_repo_dir.join("fr").join("agent-box");
        std::fs::create_dir_all(repo_path.join(".git")).unwrap();

        let config = Config {
            base_repo_dir: base_repo_dir.clone(),
            repo_discovery_dirs: vec![],
            repo_discovery_timeout_secs: 30,
            workspace_dir: PathBuf::from("/mnt/workspace"),
            default_profile: None,
            profiles: std::collections::HashMap::new(),
            runtime: RuntimeConfig {
                backend: "podman".to_string(),
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
                skip_mounts: vec![],
                env: Default::default(),
                env_passthrough: vec![],
                ports: Default::default(),
                hosts: Default::default(),
                dns: Default::default(),
            },
            context: String::new(),
            context_path: "/tmp/context".to_string(),
            portal: crate::portal::PortalConfig::default(),
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
        use crate::config::RuntimeConfig;

        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-locate-partial-{}", std::process::id()));
        let base_repo_dir = temp_dir.join("repos");

        // Create a mock repo with .git directory
        let repo_path = base_repo_dir.join("fr").join("agent-box");
        std::fs::create_dir_all(repo_path.join(".git")).unwrap();

        let config = Config {
            base_repo_dir: base_repo_dir.clone(),
            repo_discovery_dirs: vec![],
            repo_discovery_timeout_secs: 30,
            workspace_dir: PathBuf::from("/mnt/workspace"),
            default_profile: None,
            profiles: std::collections::HashMap::new(),
            runtime: RuntimeConfig {
                backend: "podman".to_string(),
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
                skip_mounts: vec![],
                env: Default::default(),
                env_passthrough: vec![],
                ports: Default::default(),
                hosts: Default::default(),
                dns: Default::default(),
            },
            context: String::new(),
            context_path: "/tmp/context".to_string(),
            portal: crate::portal::PortalConfig::default(),
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
        use crate::config::RuntimeConfig;

        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-locate-nomatch-{}", std::process::id()));
        let base_repo_dir = temp_dir.join("repos");

        // Create a mock repo with .git directory
        let repo_path = base_repo_dir.join("fr").join("agent-box");
        std::fs::create_dir_all(repo_path.join(".git")).unwrap();

        let config = Config {
            base_repo_dir: base_repo_dir.clone(),
            repo_discovery_dirs: vec![],
            repo_discovery_timeout_secs: 30,
            workspace_dir: PathBuf::from("/mnt/workspace"),
            default_profile: None,
            profiles: std::collections::HashMap::new(),
            runtime: RuntimeConfig {
                backend: "podman".to_string(),
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
                skip_mounts: vec![],
                env: Default::default(),
                env_passthrough: vec![],
                ports: Default::default(),
                hosts: Default::default(),
                dns: Default::default(),
            },
            context: String::new(),
            context_path: "/tmp/context".to_string(),
            portal: crate::portal::PortalConfig::default(),
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

    #[test]
    fn test_discover_repo_ids_uses_repo_discovery_dirs() {
        use crate::config::RuntimeConfig;

        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-discovery-dirs-{}", std::process::id()));
        let base_repo_dir = temp_dir.join("base");
        let discover_dir = temp_dir.join("discover");

        std::fs::create_dir_all(base_repo_dir.join("ignored")).unwrap();
        std::fs::create_dir_all(discover_dir.join("fr").join("agent-box").join(".git")).unwrap();

        let config = Config {
            base_repo_dir: base_repo_dir.clone(),
            repo_discovery_dirs: vec![discover_dir.clone()],
            repo_discovery_timeout_secs: 30,
            workspace_dir: PathBuf::from("/mnt/workspace"),
            default_profile: None,
            profiles: std::collections::HashMap::new(),
            runtime: RuntimeConfig {
                backend: "podman".to_string(),
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
                skip_mounts: vec![],
                env: Default::default(),
                env_passthrough: vec![],
                ports: Default::default(),
                hosts: Default::default(),
                dns: Default::default(),
            },
            context: String::new(),
            context_path: "/tmp/context".to_string(),
            portal: crate::portal::PortalConfig::default(),
        };

        let repos = RepoIdentifier::discover_repo_ids(&config).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].relative_path(), Path::new("fr/agent-box"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_discover_repo_ids_keeps_duplicates_across_discovery_dirs() {
        use crate::config::RuntimeConfig;

        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-discovery-dupes-{}", std::process::id()));
        let d1 = temp_dir.join("d1");
        let d2 = temp_dir.join("d2");

        std::fs::create_dir_all(d1.join("fr").join("agent-box").join(".git")).unwrap();
        std::fs::create_dir_all(d2.join("fr").join("agent-box").join(".git")).unwrap();

        let config = Config {
            base_repo_dir: temp_dir.join("base"),
            repo_discovery_dirs: vec![d1.clone(), d2.clone()],
            repo_discovery_timeout_secs: 30,
            workspace_dir: PathBuf::from("/mnt/workspace"),
            default_profile: None,
            profiles: std::collections::HashMap::new(),
            runtime: RuntimeConfig {
                backend: "podman".to_string(),
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
                skip_mounts: vec![],
                env: Default::default(),
                env_passthrough: vec![],
                ports: Default::default(),
                hosts: Default::default(),
                dns: Default::default(),
            },
            context: String::new(),
            context_path: "/tmp/context".to_string(),
            portal: crate::portal::PortalConfig::default(),
        };

        let repos = RepoIdentifier::discover_repo_ids(&config).unwrap();
        assert_eq!(repos.len(), 2);
        assert_ne!(repos[0].source_path(&config), repos[1].source_path(&config));

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}

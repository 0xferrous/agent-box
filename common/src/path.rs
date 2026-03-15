use eyre::{Result, eyre};
use jj_lib::object_id::ObjectId;
use jj_lib::repo::Repo;
use std::path::{Path, PathBuf};

use crate::config::Config;

/// Type of workspace (git or jj)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkspaceType {
    Git,
    Jj,
}

impl WorkspaceType {
    /// Returns the directory name used for this workspace type.
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkspaceType::Git => "git",
            WorkspaceType::Jj => "jj",
        }
    }
}

/// Status of a workspace's source repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceStatus {
    /// Source repo still exists at the reconstructed path.
    Healthy,
    /// Source repo was not found at the reconstructed path.
    /// This can happen if the repo was deleted or if base_repo_dir changed.
    Unresolved,
}

impl WorkspaceStatus {
    /// Returns a human-readable label for this status.
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkspaceStatus::Healthy => "healthy",
            WorkspaceStatus::Unresolved => "unresolved",
        }
    }
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
    /// The relative path from any base directory (e.g., "myproject" or "work/project")
    pub relative_path: PathBuf,
}

impl RepoIdentifier {
    /// Create from a repo path within base_repo_dir.
    ///
    /// Callers must pass canonicalized paths (symlinks resolved, no `.`/`..`).
    /// `find_git_root_from` and `discover_repos_in_dir` handle canonicalization
    /// at their respective boundaries, so callers using those functions satisfy
    /// this contract automatically.
    pub fn from_repo_path(config: &Config, full_path: &Path) -> Result<Self> {
        // Validate that the path looks canonical (no `.` or `..` components).
        // This catches the most common programming mistakes but cannot detect
        // unresolved symlinks without hitting the filesystem, which would be
        // inappropriate for a debug_assert.
        debug_assert!(
            full_path.components().all(|c| !matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )),
            "from_repo_path expects a canonical path, got: {}",
            full_path.display()
        );
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
            .join(WorkspaceType::Git.as_str())
            .join(&self.relative_path)
            .join(session)
    }

    /// Get the full path for a jj workspace with given session
    pub fn jj_workspace_path(&self, config: &Config, session: &str) -> PathBuf {
        config
            .workspace_dir
            .join(WorkspaceType::Jj.as_str())
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

    /// Helper function to discover repositories in a directory based on a filter predicate.
    /// Stops descending into directories that are already repos.
    /// Canonicalizes discovered paths before computing relative paths to ensure
    /// consistent workspace paths regardless of symlinks. base_dir is assumed
    /// to already be canonical (guaranteed by expand_path in load_config).
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
            let path = entry.path();

            if !path.is_dir() || !is_repo(path) {
                continue;
            }

            // Canonicalize to resolve symlinks before computing the relative
            // path, ensuring deduplication. Warn and skip on failure since
            // one dangling symlink should not abort discovery of all repos.
            let canonical = match path.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    eprintln!(
                        "warning: skipping repo, failed to canonicalize path: {}: {e}",
                        path.display()
                    );
                    continue;
                }
            };

            // Get the relative path from base_dir. After canonicalization,
            // a symlinked repo may resolve to a path outside base_dir.
            let Ok(relative_path) = canonical.strip_prefix(base_dir) else {
                eprintln!(
                    "warning: skipping repo at {} (canonical path {} is outside base_repo_dir {})",
                    path.display(),
                    canonical.display(),
                    base_dir.display()
                );
                continue;
            };

            repos.push(Self {
                relative_path: relative_path.to_path_buf(),
            });
        }

        Ok(repos)
    }

    /// Discover all repositories under configured discovery directories.
    ///
    /// Priority 1: If base_repo_dir was explicitly set AND is not "/", scan it
    /// (backward compatibility with existing configurations).
    /// Priority 2: Otherwise, return an error with guidance on how to configure
    /// discovery directories.
    pub fn discover_repo_ids(config: &Config) -> Result<Vec<Self>> {
        // Only scan base_repo_dir if it was explicitly configured and is not
        // the root filesystem. Without the "/" guard, a user who explicitly
        // writes `base_repo_dir = "/"` would trigger a WalkDir scan of the
        // entire filesystem.
        if config.base_repo_dir_explicit && config.base_repo_dir != Path::new("/") {
            return Self::discover_repos_in_dir(&config.base_repo_dir, |path| {
                path.join(".git").exists() || path.join(".jj").exists()
            });
        }

        // Default out-of-box state: no discovery directories configured.
        // Return a helpful error guiding the user toward configuration.
        //
        // The `repo_discovery_dirs` config key mentioned in the error message
        // is an intentional forward reference to Phase 2 (issue #20). The key
        // does not exist yet; this guides users toward the preferred solution
        // once it ships.
        eyre::bail!(
            "no repository discovery directories configured\n\n\
             To use the -r flag, add one of the following to your config:\n\n\
             \x20 repo_discovery_dirs = [\"~/repos\", \"~/work\"]\n\n\
             or:\n\n\
             \x20 base_repo_dir = \"/home/user/repos\""
        )
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

/// Information about a discovered workspace group (one entry per repo, not per session).
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    /// Full reconstructed source repo path.
    pub source_path: PathBuf,
    /// Type of workspace (git or jj).
    pub workspace_type: WorkspaceType,
    /// Number of session directories found.
    pub session_count: usize,
    /// Whether the source repo still exists.
    pub status: WorkspaceStatus,
    /// The relative path from workspace type dir (e.g., "home/user/repos/myproject").
    pub relative_path: PathBuf,
}

impl WorkspaceInfo {
    /// Returns the on-disk directory for this workspace group
    /// (e.g., `{workspace_dir}/git/home/user/repos/myproject`).
    /// Note: this method name intentionally mirrors `Config::workspace_dir` but returns
    /// a subdirectory specific to this workspace group, not the top-level workspace dir.
    pub fn workspace_dir(&self, config: &Config) -> PathBuf {
        config
            .workspace_dir
            .join(self.workspace_type.as_str())
            .join(&self.relative_path)
    }
}

/// Scan the workspace directory tree and discover all workspace groups.
/// Each group represents one repo (identified by its relative path under
/// {workspace_dir}/git/ or {workspace_dir}/jj/) and counts the number
/// of session directories within it.
///
/// Session directories are identified by their markers:
/// - Git: a `.git` *file* (not directory) indicating a linked worktree.
/// - JJ: a `.jj/working_copy/` directory.
///
/// This function is used by both `ab dbg list` and `ab dbg remove --unresolved`.
pub fn scan_workspaces(config: &Config) -> Result<Vec<WorkspaceInfo>> {
    let mut results = Vec::new();

    // Scan both git and jj workspace type directories
    for wtype in [WorkspaceType::Git, WorkspaceType::Jj] {
        let type_dir = config.workspace_dir.join(wtype.as_str());
        if !type_dir.exists() {
            continue;
        }

        // Collect session directories grouped by their repo relative path.
        // Key: relative path from type_dir to the repo directory
        // Value: count of sessions found
        let mut repo_sessions: std::collections::BTreeMap<PathBuf, usize> =
            std::collections::BTreeMap::new();

        let mut iter = walkdir::WalkDir::new(&type_dir)
            .follow_links(false)
            .into_iter();

        // Use manual iteration with skip_current_dir() to avoid descending
        // into session worktrees (which could contain submodule .git files
        // that would produce false-positive matches).
        while let Some(entry_result) = iter.next() {
            let entry = match entry_result {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("warning: skipping entry during workspace scan: {e}");
                    continue;
                }
            };

            let path = entry.path();

            // Skip the type_dir root itself
            if path == type_dir {
                continue;
            }

            // Only process directories
            if !entry.file_type().is_dir() {
                continue;
            }

            // Skip .git and .jj directories themselves to avoid descending
            // into git internals or jj store directories.
            if let Some(name) = path.file_name()
                && (name == ".git" || name == ".jj")
            {
                iter.skip_current_dir();
                continue;
            }

            // Check for git session marker: .git file (not directory)
            if wtype == WorkspaceType::Git {
                let dot_git = path.join(".git");
                if dot_git.is_file() {
                    // This directory is a git linked worktree (session).
                    // The session name is this directory's name; the repo
                    // relative path is everything between type_dir and this
                    // directory's parent.
                    if let Some(parent) = path.parent()
                        && let Ok(rel) = parent.strip_prefix(&type_dir)
                    {
                        *repo_sessions.entry(rel.to_path_buf()).or_insert(0) += 1;
                    }
                    // Stop descending into this session worktree to avoid
                    // false positives from submodule .git files inside it.
                    iter.skip_current_dir();
                    continue;
                }
                // Skip directories that have a .git *directory* (full
                // git repo placed inside workspace tree, not a session)
                if dot_git.is_dir() {
                    iter.skip_current_dir();
                    continue;
                }
            }

            // Check for jj session marker: .jj/working_copy/ directory
            if wtype == WorkspaceType::Jj && path.join(".jj").join("working_copy").is_dir() {
                if let Some(parent) = path.parent()
                    && let Ok(rel) = parent.strip_prefix(&type_dir)
                {
                    *repo_sessions.entry(rel.to_path_buf()).or_insert(0) += 1;
                }
                // Stop descending into this session workspace
                iter.skip_current_dir();
                continue;
            }
        }

        // Convert grouped sessions into WorkspaceInfo entries
        for (relative_path, session_count) in repo_sessions {
            // Reconstruct the source repo path using base_repo_dir.
            // When base_repo_dir is "/", this prepends "/" to the relative
            // path, reconstructing the original absolute path.
            let source_path = config.base_repo_dir.join(&relative_path);
            // Use is_dir() rather than exists() because a regular file
            // at the source path is not a valid repository.
            let status = if source_path.is_dir() {
                WorkspaceStatus::Healthy
            } else {
                WorkspaceStatus::Unresolved
            };

            results.push(WorkspaceInfo {
                source_path,
                workspace_type: wtype,
                session_count,
                status,
                relative_path,
            });
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_config() -> Config {
        make_test_config_with("/home/user/repos", true, "/mnt/workspace")
    }

    /// Build a test Config with the given base_repo_dir, explicit flag, and workspace_dir.
    /// All other fields use sensible defaults.
    fn make_test_config_with(
        base_repo_dir: impl Into<PathBuf>,
        base_repo_dir_explicit: bool,
        workspace_dir: impl Into<PathBuf>,
    ) -> Config {
        use crate::config::RuntimeConfig;
        use std::collections::HashMap;

        Config {
            base_repo_dir: base_repo_dir.into(),
            base_repo_dir_explicit,
            workspace_dir: workspace_dir.into(),
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
        let temp_dir = std::env::temp_dir().join(format!("ab-test-locate-{}", std::process::id()));
        let base_repo_dir = temp_dir.join("repos");

        // Create a mock repo with .git directory
        let repo_path = base_repo_dir.join("fr").join("agent-box");
        std::fs::create_dir_all(repo_path.join(".git")).unwrap();

        let config = make_test_config_with(base_repo_dir.clone(), true, "/mnt/workspace");

        // Test exact match
        let matches = RepoIdentifier::find_matching(&config, "fr/agent-box").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].relative_path(), Path::new("fr/agent-box"));

        // Cleanup uses .ok() to ignore errors. If the test panics, temp dirs
        // are left behind; this matches the existing test cleanup pattern.
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_find_matching_partial_match() {
        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-locate-partial-{}", std::process::id()));
        let base_repo_dir = temp_dir.join("repos");

        // Create a mock repo with .git directory
        let repo_path = base_repo_dir.join("fr").join("agent-box");
        std::fs::create_dir_all(repo_path.join(".git")).unwrap();

        let config = make_test_config_with(base_repo_dir.clone(), true, "/mnt/workspace");

        // Test partial match (searching for "agent-box" should match "fr/agent-box")
        let matches = RepoIdentifier::find_matching(&config, "agent-box").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].relative_path(), Path::new("fr/agent-box"));

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_find_matching_no_match() {
        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-locate-nomatch-{}", std::process::id()));
        let base_repo_dir = temp_dir.join("repos");

        // Create a mock repo with .git directory
        let repo_path = base_repo_dir.join("fr").join("agent-box");
        std::fs::create_dir_all(repo_path.join(".git")).unwrap();

        let config = make_test_config_with(base_repo_dir.clone(), true, "/mnt/workspace");

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
    fn test_nested_directory_mirroring() {
        // With base_repo_dir = "/", verify that from_repo_path produces
        // a relative path mirroring the full filesystem path, and that
        // source_path and git_workspace_path reconstruct correctly.
        let config = make_test_config_with("/", false, "/mnt/workspace");

        let repo_path = PathBuf::from("/home/user/repos/myproject");
        let id = RepoIdentifier::from_repo_path(&config, &repo_path).unwrap();

        // Relative path should mirror the full path under "/"
        assert_eq!(id.relative_path(), Path::new("home/user/repos/myproject"));

        // source_path should reconstruct the original path
        assert_eq!(
            id.source_path(&config),
            PathBuf::from("/home/user/repos/myproject")
        );

        // git_workspace_path should nest under the workspace dir
        assert_eq!(
            id.git_workspace_path(&config, "my-session"),
            PathBuf::from("/mnt/workspace/git/home/user/repos/myproject/my-session")
        );
    }

    #[test]
    fn test_nested_mirroring_with_explicit_base() {
        // With an explicit base_repo_dir, verify existing behavior is unchanged:
        // relative_path is just the repo name, not the full filesystem path.
        let config = make_test_config();
        let repo_path = PathBuf::from("/home/user/repos/myproject");
        let id = RepoIdentifier::from_repo_path(&config, &repo_path).unwrap();

        assert_eq!(id.relative_path(), Path::new("myproject"));
        assert_eq!(
            id.git_workspace_path(&config, "session1"),
            PathBuf::from("/mnt/workspace/git/myproject/session1")
        );
    }

    #[test]
    fn test_discover_fallback_to_explicit_base_repo_dir() {
        // When base_repo_dir is explicitly set (not "/"), discover_repo_ids
        // should scan it (backward compat).
        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-discover-fallback-{}", std::process::id()));
        let base_repo_dir = temp_dir.join("repos");

        // Create a mock repo with .git directory
        let repo_path = base_repo_dir.join("my-project");
        std::fs::create_dir_all(repo_path.join(".git")).unwrap();

        let config = make_test_config_with(
            base_repo_dir.canonicalize().unwrap(),
            true,
            "/mnt/workspace",
        );

        let repos = RepoIdentifier::discover_repo_ids(&config).unwrap();
        assert_eq!(repos.len(), 1);

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_discover_no_dirs_configured() {
        // At defaults (base_repo_dir = "/", not explicit), discover_repo_ids
        // should return an error rather than scanning the whole filesystem.
        let config = make_test_config_with("/", false, "/mnt/workspace");

        let result = RepoIdentifier::discover_repo_ids(&config);
        assert!(
            result.is_err(),
            "discover_repo_ids should error when no dirs configured"
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("no repository discovery directories configured"),
            "error should mention no discovery dirs configured, got: {err_msg}"
        );
    }

    #[test]
    fn test_discover_explicit_root_base_repo_dir() {
        // Even when base_repo_dir = "/" is set explicitly, discover_repo_ids
        // should return an error (explicit "/" does not trigger a filesystem scan).
        let config = make_test_config_with("/", true, "/mnt/workspace");

        let result = RepoIdentifier::discover_repo_ids(&config);
        assert!(
            result.is_err(),
            "discover_repo_ids should error when base_repo_dir is /"
        );
        // Verify the error message matches the same guidance as the non-explicit case.
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("no repository discovery directories configured"),
            "error should mention no discovery dirs configured, got: {err_msg}",
        );
    }

    /// Helper to create a fake git session worktree marker (.git file)
    /// inside the given directory. Real git linked worktrees have a .git
    /// file (not directory) pointing to the main repo's worktrees dir.
    fn create_git_session_marker(session_dir: &Path) {
        std::fs::create_dir_all(session_dir).unwrap();
        // Write a .git file (not directory) to simulate a linked worktree
        std::fs::write(
            session_dir.join(".git"),
            "gitdir: /fake/main/.git/worktrees/session",
        )
        .unwrap();
    }

    #[test]
    fn test_dbg_list_finds_workspaces() {
        // Create a workspace directory structure mirroring a repo path with
        // session subdirectories containing .git files.
        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-dbg-list-{}", std::process::id()));
        let workspace_dir = temp_dir.join("workspaces");
        let source_repo = temp_dir.join("source-repo");

        // Create the source repo so it appears "healthy"
        std::fs::create_dir_all(&source_repo).unwrap();

        // Create workspace structure: {workspace_dir}/git/{relative_path}/{session}/.git
        // Use base_repo_dir = temp_dir so the relative path is "source-repo"
        let repo_ws_dir = workspace_dir
            .join(WorkspaceType::Git.as_str())
            .join("source-repo");
        create_git_session_marker(&repo_ws_dir.join("session-1"));
        create_git_session_marker(&repo_ws_dir.join("session-2"));

        let config = make_test_config_with(
            temp_dir.canonicalize().unwrap(),
            true,
            workspace_dir.canonicalize().unwrap(),
        );

        let workspaces = scan_workspaces(&config).unwrap();
        assert_eq!(workspaces.len(), 1, "should find exactly one repo group");
        assert_eq!(workspaces[0].session_count, 2);
        assert_eq!(workspaces[0].workspace_type, WorkspaceType::Git);
        assert_eq!(workspaces[0].status, WorkspaceStatus::Healthy);

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_dbg_list_detects_unresolved() {
        // Same structure but with the source repo path not existing.
        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-dbg-unresolved-{}", std::process::id()));
        let workspace_dir = temp_dir.join("workspaces");

        // Create workspace structure but do NOT create the source repo
        let repo_ws_dir = workspace_dir
            .join(WorkspaceType::Git.as_str())
            .join("gone-repo");
        create_git_session_marker(&repo_ws_dir.join("session-1"));

        let config = make_test_config_with(
            temp_dir.canonicalize().unwrap(),
            true,
            workspace_dir.canonicalize().unwrap(),
        );

        let workspaces = scan_workspaces(&config).unwrap();
        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].status, WorkspaceStatus::Unresolved);

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_dbg_list_ignores_submodule_git_files() {
        // Create a session worktree with a subdirectory that has its own
        // .git file (simulating an initialized submodule). The walker should
        // not count the submodule as a separate session.
        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-dbg-submodule-{}", std::process::id()));
        let workspace_dir = temp_dir.join("workspaces");
        let source_repo = temp_dir.join("my-repo");
        std::fs::create_dir_all(&source_repo).unwrap();

        // Create a session with a .git file marker
        let session_dir = workspace_dir
            .join(WorkspaceType::Git.as_str())
            .join("my-repo")
            .join("session-1");
        create_git_session_marker(&session_dir);

        // Add a fake submodule inside the session (has its own .git file)
        let submodule_dir = session_dir.join("vendor").join("submod");
        std::fs::create_dir_all(&submodule_dir).unwrap();
        std::fs::write(
            submodule_dir.join(".git"),
            "gitdir: /fake/parent/.git/modules/submod",
        )
        .unwrap();

        let config = make_test_config_with(
            temp_dir.canonicalize().unwrap(),
            true,
            workspace_dir.canonicalize().unwrap(),
        );

        let workspaces = scan_workspaces(&config).unwrap();
        assert_eq!(workspaces.len(), 1, "should find exactly one repo group");
        // The submodule's .git file should NOT be counted as a second session
        assert_eq!(
            workspaces[0].session_count, 1,
            "submodule .git file should not be counted as a session"
        );

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    /// Helper to create a fake jj session workspace marker (.jj/working_copy/)
    /// inside the given directory. Real jj workspaces contain a .jj directory
    /// with a working_copy subdirectory.
    fn create_jj_session_marker(session_dir: &Path) {
        std::fs::create_dir_all(session_dir.join(".jj").join("working_copy")).unwrap();
    }

    #[test]
    fn test_dbg_list_finds_jj_workspaces() {
        // Create a workspace directory structure mirroring jj layout with
        // .jj/working_copy/ markers and verify scan_workspaces identifies
        // them with workspace type Jj.
        let temp_dir = std::env::temp_dir().join(format!("ab-test-dbg-jj-{}", std::process::id()));
        let workspace_dir = temp_dir.join("workspaces");
        let source_repo = temp_dir.join("my-jj-repo");

        // Create the source repo so it appears "healthy"
        std::fs::create_dir_all(&source_repo).unwrap();

        // Create jj workspace structure:
        // {workspace_dir}/jj/{relative_path}/{session}/.jj/working_copy/
        let repo_ws_dir = workspace_dir
            .join(WorkspaceType::Jj.as_str())
            .join("my-jj-repo");
        create_jj_session_marker(&repo_ws_dir.join("session-a"));
        create_jj_session_marker(&repo_ws_dir.join("session-b"));
        create_jj_session_marker(&repo_ws_dir.join("session-c"));

        let config = make_test_config_with(
            temp_dir.canonicalize().unwrap(),
            true,
            workspace_dir.canonicalize().unwrap(),
        );

        let workspaces = scan_workspaces(&config).unwrap();
        assert_eq!(workspaces.len(), 1, "should find exactly one jj repo group");
        assert_eq!(workspaces[0].session_count, 3);
        assert_eq!(workspaces[0].workspace_type, WorkspaceType::Jj);
        assert_eq!(workspaces[0].status, WorkspaceStatus::Healthy);

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    #[cfg(unix)]
    fn test_discover_repos_in_dir_skips_uncanonicalizable() {
        // Create a scan directory containing a dangling symlink alongside a
        // real repo. Verify discover_repos_in_dir skips the broken symlink
        // without erroring and still finds the valid repo.
        //
        // Note: dangling symlinks require Unix symlink support. This test
        // uses std::os::unix::fs::symlink, which is available on Linux and
        // macOS but not Windows.
        let temp_dir =
            std::env::temp_dir().join(format!("ab-test-discover-dangling-{}", std::process::id()));
        let base_repo_dir = temp_dir.join("repos");
        std::fs::create_dir_all(&base_repo_dir).unwrap();

        // Create a valid repo
        let valid_repo = base_repo_dir.join("valid-repo");
        std::fs::create_dir_all(valid_repo.join(".git")).unwrap();

        // Create a dangling symlink pointing to a nonexistent target
        let dangling_link = base_repo_dir.join("broken-link");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/nonexistent/target/path", &dangling_link).unwrap();
        }

        let config = make_test_config_with(
            base_repo_dir.canonicalize().unwrap(),
            true,
            "/mnt/workspace",
        );

        // discover_repos_in_dir should succeed and find only the valid repo,
        // skipping the dangling symlink gracefully.
        let repos = RepoIdentifier::discover_repo_ids(&config).unwrap();
        assert_eq!(repos.len(), 1, "should find exactly one valid repo");
        assert_eq!(repos[0].relative_path(), Path::new("valid-repo"));

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "from_repo_path expects a canonical path")]
    fn test_from_repo_path_rejects_non_canonical_in_debug() {
        // In debug builds, from_repo_path should panic when given a path
        // containing ".." components, which indicates the caller failed to
        // canonicalize before calling.
        let config = make_test_config();
        let non_canonical = PathBuf::from("/home/user/repos/../repos/myproject");
        // This should trigger the debug_assert and panic.
        let _id = RepoIdentifier::from_repo_path(&config, &non_canonical);
    }
}

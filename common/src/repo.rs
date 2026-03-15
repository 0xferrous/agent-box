use eyre::{OptionExt, Result, WrapErr, bail, eyre};
use gix::repository::Kind;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::path::{RepoIdentifier, WorkspaceType, path_to_str};

/// Find the git root directory for session mode, resolving linked worktrees
/// to the main repo root. This ensures that running `ab new` or `ab spawn`
/// from inside a session workspace creates workspaces for the source repo,
/// not for the worktree itself.
///
/// Canonicalizes the returned path to ensure consistent workspace paths
/// regardless of whether the repo was accessed via a symlink.
pub fn find_git_root() -> Result<PathBuf> {
    let current_dir =
        std::env::current_dir().wrap_err("Failed to get current working directory")?;
    find_git_root_from(&current_dir)
}

/// Find the git root directory from an arbitrary path, resolving linked
/// worktrees to the main repo root. Returns a canonicalized path.
///
/// NOTE: jj workspace resolution (detecting `jj workspace add` workspaces
/// and resolving to the main workspace root) is deferred. The relevant API
/// to investigate is `jj_lib::workspace::Workspace::load`, which can
/// determine the repo root and enumerate sibling workspaces.
///
/// For linked worktrees, common_dir() returns the main repo's .git directory,
/// which is opened directly to find the main repo root. For normal repos and
/// submodules, workdir() is used directly.
pub fn find_git_root_from(path: &Path) -> Result<PathBuf> {
    let repo = gix::discover(path)
        .wrap_err_with(|| format!("failed to discover git repository in {}", path.display()))?;
    let root = match repo.kind() {
        Kind::WorkTree { is_linked: true } => {
            // Linked worktree: resolve to main repo root.
            // common_dir() returns the main repo's .git directory.
            // Open it directly rather than re-discovering, since we
            // already know the exact git directory location.
            let common = repo.common_dir().canonicalize().wrap_err_with(|| {
                format!(
                    "failed to canonicalize common_dir: {}",
                    repo.common_dir().display()
                )
            })?;
            let main_repo = gix::open(&common).wrap_err_with(|| {
                format!(
                    "failed to open main repo from common_dir: {}",
                    common.display()
                )
            })?;
            main_repo
                .workdir()
                .ok_or_else(|| {
                    eyre!(
                        "linked worktree's main repository at {} is bare \
                    and has no working directory; use a non-bare clone instead",
                        common.display()
                    )
                })
                .map(|p| p.to_path_buf())?
        }
        Kind::Bare => {
            bail!(
                "bare repository at {} has no working directory; \
                 use a non-bare clone instead",
                repo.git_dir().display()
            )
        }
        _ => {
            // Normal repo or submodule: use workdir directly.
            // Submodules are intentionally treated as independent repos
            // and get their own workspaces. If a user runs `ab new` from
            // inside a submodule, the submodule root is used, not the
            // parent superproject.
            repo.workdir()
                .ok_or_eyre("repository has no working directory")
                .map(|p| p.to_path_buf())?
        }
    };
    // Canonicalize to ensure consistent workspace paths regardless of symlinks
    root.canonicalize()
        .wrap_err_with(|| format!("failed to canonicalize repo root: {}", root.display()))
}

/// Find the git working directory from the current directory without resolving
/// linked worktrees. Used by local mode (`--local`) which should preserve
/// whatever directory the user is in, even if it is a linked worktree.
///
/// Does not canonicalize the return value since local mode does not compute
/// workspace paths, so symlink deduplication is not needed.
pub fn find_git_workdir() -> Result<PathBuf> {
    let current_dir =
        std::env::current_dir().wrap_err("Failed to get current working directory")?;
    find_git_workdir_from(&current_dir)
}

/// Find the git working directory from an arbitrary path without resolving
/// linked worktrees. Returns the workdir as-is (no canonicalization).
pub fn find_git_workdir_from(path: &Path) -> Result<PathBuf> {
    let repo = gix::discover(path)
        .wrap_err_with(|| format!("failed to discover git repository in {}", path.display()))?;
    match repo.kind() {
        Kind::Bare => {
            bail!(
                "bare repository at {} has no working directory; \
                 use a non-bare clone instead",
                repo.git_dir().display()
            )
        }
        _ => repo
            .workdir()
            .ok_or_eyre("repository has no working directory")
            .map(|p| p.to_path_buf()),
    }
}

/// Prompt user to select from a list of repos
fn prompt_select_repo(repos: Vec<RepoIdentifier>, prompt: &str) -> Result<RepoIdentifier> {
    let options: Vec<String> = repos
        .iter()
        .map(|r| r.relative_path().display().to_string())
        .collect();

    let selected = inquire::Select::new(prompt, options)
        .prompt()
        .map_err(|e| eyre::eyre!("Failed to get selection: {}", e))?;

    repos
        .into_iter()
        .find(|r| r.relative_path().display().to_string() == selected)
        .ok_or_else(|| eyre::eyre!("Selected repository not found"))
}

/// Locate a repository by search string, prompting user if multiple matches found
/// Returns the selected RepoIdentifier or an error if none found
pub fn locate_repo(config: &Config, search: Option<&str>) -> Result<RepoIdentifier> {
    let matches = match search {
        Some(s) => RepoIdentifier::find_matching(config, s)?,
        None => RepoIdentifier::discover_repo_ids(config)?,
    };

    match matches.len() {
        0 => bail!(
            "Could not find repository{}",
            search
                .map(|s| format!(" matching '{}'", s))
                .unwrap_or_default()
        ),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => {
            let prompt = match search {
                Some(s) => format!("Multiple repositories match '{}'. Select one:", s),
                None => "Select a repository:".to_string(),
            };
            prompt_select_repo(matches, &prompt)
        }
    }
}

/// Resolve repo argument to a RepoIdentifier
/// - If None: find git root from cwd and compute RepoId from it
/// - If Some: use locate_repo to find the repo_id (prompts if multiple matches)
pub fn resolve_repo_id(config: &Config, repo_name: Option<&str>) -> Result<RepoIdentifier> {
    match repo_name {
        Some(name) => locate_repo(config, Some(name)),
        None => {
            let git_root =
                find_git_root().wrap_err("failed to determine repository root for session mode")?;
            RepoIdentifier::from_repo_path(config, &git_root)
        }
    }
}

/// Create a new workspace (git worktree or jj workspace)
pub fn new_workspace(
    config: &Config,
    repo_name: Option<&str>,
    session_name: Option<&str>,
    workspace_type: crate::path::WorkspaceType,
) -> Result<()> {
    // Resolve repo_id from repo_name argument
    let repo_id = resolve_repo_id(config, repo_name)?;

    // Get session name
    let session = get_session_name(session_name)?;

    // Calculate paths
    let source_path = repo_id.source_path(config);
    let workspace_path = repo_id.workspace_path(config, workspace_type, &session);

    println!(
        "Creating new {} workspace:",
        match workspace_type {
            crate::path::WorkspaceType::Git => "git worktree",
            crate::path::WorkspaceType::Jj => "jj workspace",
        }
    );
    println!("  Source: {}", source_path.display());
    println!("  Workspace: {}", workspace_path.display());
    println!("  Session: {}", session);

    // Run the appropriate CLI command
    match workspace_type {
        crate::path::WorkspaceType::Git => {
            create_git_worktree(config, &repo_id, &session)?;
        }
        crate::path::WorkspaceType::Jj => {
            create_jj_workspace(config, &repo_id, &session)?;
        }
    }

    println!(
        "\n✓ Successfully created workspace at: {}",
        workspace_path.display()
    );

    Ok(())
}

/// Create a new jj workspace from an existing colocated jj repo
fn create_jj_workspace(config: &Config, repo_id: &RepoIdentifier, session: &str) -> Result<()> {
    let source_path = repo_id.source_path(config);
    let workspace_path = repo_id.jj_workspace_path(config, session);

    // Verify that source is a colocated jj repo
    let jj_dir = source_path.join(".jj");
    if !jj_dir.exists() {
        bail!(
            "Source is not a colocated jj repository (no .jj directory found at {})\n\
             Please initialize jj in your repository first with: jj git init --colocate",
            source_path.display()
        );
    }

    // Create parent directory (jj workspace add will create the workspace directory itself)
    if let Some(parent) = workspace_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    println!("Creating jj workspace from colocated repo...");

    // Use jj workspace add from the colocated repo
    let output = std::process::Command::new("jj")
        .current_dir(&source_path)
        .args([
            "workspace",
            "add",
            "--name",
            session,
            path_to_str(&workspace_path)?,
        ])
        .output()?;

    if !output.status.success() {
        bail!(
            "Failed to create jj workspace: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("  ✓ JJ workspace created successfully");

    Ok(())
}

/// Create a new git worktree from a git repository
fn create_git_worktree(config: &Config, repo_id: &RepoIdentifier, session: &str) -> Result<()> {
    let source_path = repo_id.source_path(config);
    let workspace_path = repo_id.git_workspace_path(config, session);

    // Create parent directory (git worktree add will create the workspace directory itself)
    if let Some(parent) = workspace_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Check if branch exists
    let check_output = std::process::Command::new("git")
        .current_dir(&source_path)
        .args(["rev-parse", "--verify", &format!("refs/heads/{}", session)])
        .output()?;

    let branch_exists = check_output.status.success();

    // Create worktree using git worktree add
    let mut args = vec!["worktree", "add"];

    // If branch doesn't exist, create it with -b flag
    if !branch_exists {
        args.push("-b");
        args.push(session);
        args.push(path_to_str(&workspace_path)?);
        println!("  Creating new branch: {}", session);
    } else {
        args.push(path_to_str(&workspace_path)?);
        args.push(session);
        println!("  Using existing branch: {}", session);
    }

    let output = std::process::Command::new("git")
        .current_dir(&source_path)
        .args(&args)
        .output()?;

    if !output.status.success() {
        bail!(
            "Failed to create git worktree: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("  ✓ Git worktree created successfully");

    Ok(())
}

/// Get session name from argument or prompt
fn get_session_name(session_name: Option<&str>) -> Result<String> {
    match session_name {
        Some(name) => {
            let trimmed = name.trim();
            if trimmed.contains(char::is_whitespace) {
                bail!("Session name cannot contain whitespace: '{}'", name);
            }
            if trimmed.is_empty() {
                bail!("Session name cannot be empty");
            }
            Ok(trimmed.to_string())
        }
        None => {
            let validator = |input: &str| {
                let trimmed = input.trim();
                if trimmed.is_empty() {
                    return Ok(inquire::validator::Validation::Invalid(
                        "Session name cannot be empty".into(),
                    ));
                }
                if trimmed.contains(char::is_whitespace) {
                    return Ok(inquire::validator::Validation::Invalid(
                        "Session name cannot contain spaces".into(),
                    ));
                }
                Ok(inquire::validator::Validation::Valid)
            };

            let name = inquire::Text::new("Session name:")
                .with_help_message("Enter a name for this workspace session (no spaces)")
                .with_validator(validator)
                .prompt()
                .map_err(|e| eyre::eyre!("Failed to get session name: {}", e))?;

            Ok(name.trim().to_string())
        }
    }
}

/// Remove all workspaces for a given repo ID
pub fn remove_repo(config: &Config, repo_id: &RepoIdentifier, dry_run: bool) -> Result<()> {
    let paths_to_remove: Vec<(&str, PathBuf)> = vec![
        (
            "Git worktrees",
            config
                .workspace_dir
                .join(WorkspaceType::Git.as_str())
                .join(repo_id.relative_path()),
        ),
        (
            "JJ workspaces",
            config
                .workspace_dir
                .join(WorkspaceType::Jj.as_str())
                .join(repo_id.relative_path()),
        ),
    ];

    println!("Repository: {}", repo_id.relative_path().display());
    println!("\nThe following directories will be removed:");

    let mut found_any = false;
    for (label, path) in &paths_to_remove {
        if path.exists() {
            found_any = true;
            println!("  [{}] {}", label, path.display());
        }
    }

    if !found_any {
        println!("  (none - no directories found)");
        return Ok(());
    }

    if dry_run {
        println!("\n[DRY RUN] No files were actually deleted.");
        return Ok(());
    }

    // Remove all existing directories
    for (label, path) in &paths_to_remove {
        if path.exists() {
            println!("\nRemoving {}: {}", label, path.display());
            std::fs::remove_dir_all(path)?;
            println!("  ✓ Removed");
        }
    }

    println!("\n✓ All workspaces and repositories removed successfully");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Helper: create a temporary directory with a unique name for test isolation.
    fn temp_test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ab-test-{name}-{}", std::process::id()))
    }

    /// Helper: run git init in the given directory and make an initial commit
    /// so that worktrees can be created (git worktree add requires at least
    /// one commit).
    fn git_init_with_commit(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        let output = Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git config user.email failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git config user.name failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        // Create an initial commit so worktree add works
        let output = Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn test_find_git_root_from_main_worktree() {
        let tmp = temp_test_dir("root-main");
        let repo_dir = tmp.join("my-repo");
        git_init_with_commit(&repo_dir);

        let result = find_git_root_from(&repo_dir).unwrap();
        // The result should be the canonicalized repo directory
        let expected = repo_dir.canonicalize().unwrap();
        assert_eq!(result, expected);

        // Cleanup
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_find_git_root_from_linked_worktree() {
        // Verify that find_git_root_from resolves a linked worktree back
        // to the main repo root, not the worktree directory itself.
        let tmp = temp_test_dir("root-linked");
        let repo_dir = tmp.join("my-repo");
        let worktree_dir = tmp.join("my-worktree");
        git_init_with_commit(&repo_dir);

        // Create a linked worktree
        Command::new("git")
            .args([
                "worktree",
                "add",
                worktree_dir.to_str().unwrap(),
                "-b",
                "test-branch",
            ])
            .current_dir(&repo_dir)
            .output()
            .unwrap();

        let result = find_git_root_from(&worktree_dir).unwrap();
        let expected = repo_dir.canonicalize().unwrap();
        assert_eq!(
            result, expected,
            "linked worktree should resolve to the main repo root"
        );

        // Cleanup
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_find_git_workdir_from_linked_worktree() {
        // Verify that find_git_workdir_from returns the linked worktree
        // directory itself, not the main repo root. This is needed for
        // local mode which preserves the user's working directory.
        let tmp = temp_test_dir("workdir-linked");
        let repo_dir = tmp.join("my-repo");
        let worktree_dir = tmp.join("my-worktree");
        git_init_with_commit(&repo_dir);

        // Create a linked worktree
        Command::new("git")
            .args([
                "worktree",
                "add",
                worktree_dir.to_str().unwrap(),
                "-b",
                "test-branch",
            ])
            .current_dir(&repo_dir)
            .output()
            .unwrap();

        let result = find_git_workdir_from(&worktree_dir).unwrap();
        // workdir_from does NOT canonicalize, so compare against
        // the path as gix returns it (which may or may not be canonical).
        // The key assertion: the result should point to the worktree dir,
        // not the main repo dir.
        let canonical_worktree = worktree_dir.canonicalize().unwrap();
        let canonical_result = result.canonicalize().unwrap();
        assert_eq!(
            canonical_result, canonical_worktree,
            "find_git_workdir_from should return the worktree directory, not the main repo"
        );

        // Cleanup
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_find_git_root_from_linked_worktree_for_config() {
        // Verify that find_git_root() (the wrapper used by load_config())
        // resolves to the main repo root when called from a linked worktree,
        // confirming repo-local config is shared across worktrees.
        // We test this via find_git_root_from since find_git_root() uses cwd.
        let tmp = temp_test_dir("root-config");
        let repo_dir = tmp.join("my-repo");
        let worktree_dir = tmp.join("my-worktree");
        git_init_with_commit(&repo_dir);

        Command::new("git")
            .args([
                "worktree",
                "add",
                worktree_dir.to_str().unwrap(),
                "-b",
                "config-branch",
            ])
            .current_dir(&repo_dir)
            .output()
            .unwrap();

        // Both paths should resolve to the same main repo root
        let from_main = find_git_root_from(&repo_dir).unwrap();
        let from_worktree = find_git_root_from(&worktree_dir).unwrap();
        assert_eq!(
            from_main, from_worktree,
            "find_git_root_from should resolve to the same root from main and linked worktree"
        );

        // Cleanup
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_find_git_root_from_linked_worktree_of_bare_repo() {
        // Verify that find_git_root_from returns a clear error when called
        // from a linked worktree whose main repo is bare (bare repos have
        // no working directory).
        let tmp = temp_test_dir("root-bare");
        let bare_dir = tmp.join("bare.git");
        let worktree_dir = tmp.join("my-worktree");

        std::fs::create_dir_all(&bare_dir).unwrap();
        Command::new("git")
            .args(["init", "--bare"])
            .current_dir(&bare_dir)
            .output()
            .unwrap();

        // Create a linked worktree from the bare repo. Bare repos need
        // a commit to create worktrees, so create one via a detached HEAD.
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&bare_dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&bare_dir)
            .output()
            .unwrap();

        // Create a temporary non-bare clone to make a commit, then push
        let clone_dir = tmp.join("clone");
        Command::new("git")
            .args([
                "clone",
                bare_dir.to_str().unwrap(),
                clone_dir.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&clone_dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&clone_dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(&clone_dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["push", "origin", "HEAD:main"])
            .current_dir(&clone_dir)
            .output()
            .unwrap();

        // Now create a linked worktree from the bare repo
        let wt_result = Command::new("git")
            .args(["worktree", "add", worktree_dir.to_str().unwrap(), "main"])
            .current_dir(&bare_dir)
            .output()
            .unwrap();

        if !wt_result.status.success() {
            // Some git versions do not support creating worktrees from bare
            // repos. Skip the test with a clear message rather than silently
            // passing, so CI logs show why it was not exercised.
            eprintln!(
                "SKIPPED: git worktree add from bare repo failed (git may not support this). \
                 stderr: {}",
                String::from_utf8_lossy(&wt_result.stderr)
            );
            std::fs::remove_dir_all(&tmp).ok();
            return;
        }

        // Worktree was created; verify find_git_root_from returns an error
        // because the main repo is bare and has no working directory.
        assert!(
            worktree_dir.exists(),
            "worktree directory should exist after successful git worktree add"
        );
        let result = find_git_root_from(&worktree_dir);
        assert!(
            result.is_err(),
            "find_git_root_from should fail for linked worktree of bare repo"
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("bare") || err_msg.contains("no working directory"),
            "error should mention bare or no working directory, got: {err_msg}",
        );

        // Cleanup
        std::fs::remove_dir_all(&tmp).ok();
    }
}

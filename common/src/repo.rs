use eyre::{OptionExt, Result, WrapErr, bail, eyre};
use gix::repository::Kind;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::path::RepoIdentifier;
use crate::path::path_to_str;

/// Find the git root directory from current directory.
/// Set `resolve_linked_worktree_to_main_repo=true` to resolve linked worktrees
/// to the main repository root.
pub fn find_git_root(resolve_linked_worktree_to_main_repo: bool) -> Result<PathBuf> {
    let current_dir =
        std::env::current_dir().wrap_err("Failed to get current working directory")?;
    find_git_root_from(&current_dir, resolve_linked_worktree_to_main_repo)
}

/// Find git root from an arbitrary path.
/// When `resolve_linked_worktree_to_main_repo` is true, linked worktrees are
/// resolved to the main repository root via common_dir.
pub fn find_git_root_from(
    path: &Path,
    resolve_linked_worktree_to_main_repo: bool,
) -> Result<PathBuf> {
    let repo = gix::discover(path)
        .wrap_err_with(|| format!("Failed to discover git repository in {}", path.display()))?;

    let root = match repo.kind() {
        Kind::WorkTree { is_linked: true } if resolve_linked_worktree_to_main_repo => {
            let common = repo.common_dir().canonicalize().wrap_err_with(|| {
                format!(
                    "Failed to canonicalize common_dir: {}",
                    repo.common_dir().display()
                )
            })?;
            let main_repo = gix::open(&common).wrap_err_with(|| {
                format!(
                    "Failed to open main repo from common_dir: {}",
                    common.display()
                )
            })?;
            main_repo
                .workdir()
                .ok_or_else(|| {
                    eyre!(
                        "Linked worktree's main repository at {} is bare and has no working directory",
                        common.display()
                    )
                })
                .map(|p| p.to_path_buf())?
        }
        Kind::Bare => {
            bail!(
                "Bare repository at {} has no working directory",
                repo.git_dir().display()
            )
        }
        _ => repo
            .workdir()
            .ok_or_eyre("Repository has no working directory")
            .map(|p| p.to_path_buf())?,
    };

    root.canonicalize()
        .wrap_err_with(|| format!("Failed to canonicalize repo root: {}", root.display()))
}

/// Prompt user to select from a list of repos using inquire.
fn prompt_select_repo(
    config: &Config,
    repos: Vec<RepoIdentifier>,
    prompt: &str,
) -> Result<RepoIdentifier> {
    let options: Vec<String> = repos
        .iter()
        .map(|r| r.source_path(config).display().to_string())
        .collect();

    let selected = inquire::Select::new(prompt, options)
        .prompt()
        .map_err(|e| eyre::eyre!("Failed to get selection: {}", e))?;

    repos
        .into_iter()
        .find(|r| r.source_path(config).display().to_string() == selected)
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
            prompt_select_repo(config, matches, &prompt)
        }
    }
}

/// Pick from all discovered repositories using the same interactive selector as locate_repo.
pub fn pick_discovered_repo(config: &Config) -> Result<RepoIdentifier> {
    let repos = RepoIdentifier::discover_repo_ids(config)?;
    if repos.is_empty() {
        bail!("No repositories discovered");
    }
    prompt_select_repo(config, repos, "Select a repository:")
}

/// Resolve repo argument to a RepoIdentifier
/// - If None: find git root from cwd and compute RepoId from it
/// - If Some: use locate_repo to find the repo_id (prompts if multiple matches)
pub fn resolve_repo_id(config: &Config, repo_name: Option<&str>) -> Result<RepoIdentifier> {
    let repo_id = match repo_name {
        Some(name) => locate_repo(config, Some(name)),
        None => {
            // Prefer git root resolution (handles linked worktrees), then fall
            // back to jj workspace root for jj-only repos.
            let repo_root = find_git_root(true).or_else(|_| find_jj_workspace_root(true))?;
            RepoIdentifier::from_repo_path(config, &repo_root)
        }
    };
    println!("debug: {repo_id:?}");
    repo_id
}

/// Find enclosing jj workspace root from current directory.
///
/// When `resolve_to_original_root` is true:
/// - If `.jj/repo` is a file, resolve to the original workspace root.
///
/// When false:
/// - Return the current workspace root (the ancestor containing `.jj`) even
///   if it points to an original root.
fn find_jj_workspace_root(resolve_to_original_root: bool) -> Result<PathBuf> {
    let current = std::env::current_dir().wrap_err("Failed to get current working directory")?;
    find_jj_workspace_root_from(&current, resolve_to_original_root)
}

fn find_jj_workspace_root_from(path: &Path, resolve_to_original_root: bool) -> Result<PathBuf> {
    for ancestor in path.ancestors() {
        let jj_dir = ancestor.join(".jj");
        if !jj_dir.is_dir() {
            continue;
        }

        let repo_entry = jj_dir.join("repo");

        // Original workspace: `.jj/repo` is a directory
        if repo_entry.is_dir() {
            return ancestor.canonicalize().wrap_err_with(|| {
                format!(
                    "Failed to canonicalize jj workspace root: {}",
                    ancestor.display()
                )
            });
        }

        // Linked workspace: `.jj/repo` is a file that points to
        // `<original_root>/.jj/repo`.
        if repo_entry.is_file() {
            if !resolve_to_original_root {
                return ancestor.canonicalize().wrap_err_with(|| {
                    format!(
                        "Failed to canonicalize jj workspace root: {}",
                        ancestor.display()
                    )
                });
            }

            let raw = std::fs::read_to_string(&repo_entry)
                .wrap_err_with(|| format!("Failed to read {}", repo_entry.display()))?;
            let target = PathBuf::from(raw.trim());
            let target_abs = if target.is_absolute() {
                target
            } else {
                jj_dir.join(target)
            };

            let original_root = target_abs
                .parent()
                .and_then(|p| p.parent())
                .ok_or_else(|| {
                    eyre!(
                        "Malformed jj repo pointer in {}: {}",
                        repo_entry.display(),
                        target_abs.display()
                    )
                })?;

            return original_root.canonicalize().wrap_err_with(|| {
                format!(
                    "Failed to canonicalize resolved jj workspace root: {}",
                    original_root.display()
                )
            });
        }

        // `.jj` exists but no recognizable `repo` entry; keep walking.
    }

    bail!("Failed to discover git repository or jj workspace from current directory")
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
                .join("git")
                .join(repo_id.relative_path()),
        ),
        (
            "JJ workspaces",
            config
                .workspace_dir
                .join("jj")
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
    use crate::config::RuntimeConfig;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn cwd_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn make_test_config(base_repo_dir: PathBuf) -> Config {
        Config {
            workspace_dir: PathBuf::from("/workspaces"),
            base_repo_dir,
            repo_discovery_dirs: vec![],
            repo_discovery_timeout_secs: 5,
            default_profile: None,
            profiles: HashMap::new(),
            runtime: RuntimeConfig {
                backend: "podman".to_string(),
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: Default::default(),
                env: vec![],
                env_passthrough: vec![],
                ports: vec![],
                hosts: vec![],
                dns: vec![],
                skip_mounts: vec![],
            },
            context: String::new(),
            context_path: "/tmp/context".to_string(),
            portal: crate::portal::PortalConfig::default(),
        }
    }

    #[test]
    fn test_find_jj_workspace_root_from_nested_dir() {
        let _guard = cwd_test_lock().lock().unwrap();
        let temp = std::env::temp_dir().join(format!("ab-test-jj-root-{}", std::process::id()));
        let repo = temp.join("repos").join("fr").join("agent-box");
        let nested = repo.join("deep").join("child");
        std::fs::create_dir_all(repo.join(".jj").join("repo")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();

        let old_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&nested).unwrap();

        let found = find_jj_workspace_root(true).unwrap();
        assert_eq!(found, repo.canonicalize().unwrap());

        std::env::set_current_dir(old_cwd).unwrap();
        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_resolve_repo_id_falls_back_to_jj_workspace_root() {
        let _guard = cwd_test_lock().lock().unwrap();
        let temp = std::env::temp_dir().join(format!(
            "ab-test-resolve-jj-fallback-{}",
            std::process::id()
        ));
        let base_repo_dir = temp.join("repos");
        let repo = base_repo_dir.join("fr").join("agent-box");
        let nested = repo.join("subdir");

        std::fs::create_dir_all(repo.join(".jj").join("repo")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();

        let config = make_test_config(base_repo_dir.clone());

        let old_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&nested).unwrap();

        let repo_id = resolve_repo_id(&config, None).unwrap();

        std::env::set_current_dir(old_cwd).unwrap();
        std::fs::remove_dir_all(&temp).ok();

        assert_eq!(repo_id.relative_path(), Path::new("fr/agent-box"));
    }

    #[test]
    fn test_find_jj_workspace_root_from_linked_workspace_repo_file() {
        let _guard = cwd_test_lock().lock().unwrap();
        let temp = std::env::temp_dir().join(format!("ab-test-jj-linked-{}", std::process::id()));

        let original_root = temp.join("repos").join("fr").join("agent-box");
        let linked_ws = temp.join("ws").join("agent-box-ws");
        let nested = linked_ws.join("nested");

        std::fs::create_dir_all(original_root.join(".jj").join("repo")).unwrap();
        std::fs::create_dir_all(linked_ws.join(".jj")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();

        // Simulate jj linked workspace pointer: .jj/repo file pointing to
        // <original_root>/.jj/repo (relative path).
        std::fs::write(
            linked_ws.join(".jj").join("repo"),
            original_root.join(".jj").join("repo").display().to_string(),
        )
        .unwrap();

        let old_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&nested).unwrap();

        let found = find_jj_workspace_root(true).unwrap();
        assert_eq!(found, original_root.canonicalize().unwrap());

        let found_current = find_jj_workspace_root_from(&nested, false).unwrap();
        assert_eq!(found_current, linked_ws.canonicalize().unwrap());

        std::env::set_current_dir(old_cwd).unwrap();
        std::fs::remove_dir_all(&temp).ok();
    }
}

use eyre::{OptionExt, Result, WrapErr, bail};
use std::path::PathBuf;

use crate::config::Config;
use crate::path::RepoIdentifier;
use crate::path::path_to_str;

/// Find the git root directory by traversing up from the current directory
fn find_git_root() -> Result<PathBuf> {
    let current_dir =
        std::env::current_dir().wrap_err("Failed to get current working directory")?;

    let repo = gix::discover(&current_dir).wrap_err_with(|| {
        format!(
            "Failed to discover git repository in {}",
            current_dir.display()
        )
    })?;

    // Get the work tree path
    repo.workdir()
        .ok_or_eyre("Cannot work with a bare repository")
        .map(|p: &std::path::Path| p.to_path_buf())
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
    let repo_id = match repo_name {
        Some(name) => locate_repo(config, Some(name)),
        None => {
            let git_root = find_git_root()?;
            RepoIdentifier::from_repo_path(config, &git_root)
        }
    };
    println!("debug: {repo_id:?}");
    repo_id
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

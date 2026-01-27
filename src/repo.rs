use eyre::{OptionExt, Result, WrapErr, bail};
use std::path::{Path, PathBuf};

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

/// Create a new workspace (git worktree or jj workspace) from the current git repository
pub fn new_workspace(
    config: &Config,
    repo_name: Option<&str>,
    session_name: Option<&str>,
    workspace_type: crate::path::WorkspaceType,
) -> Result<()> {
    // Find git root
    let git_root = find_git_root()?;

    // Determine repo_id from git_root or use provided repo_name
    let repo_id = if let Some(name) = repo_name {
        RepoIdentifier {
            relative_path: PathBuf::from(name),
        }
    } else {
        RepoIdentifier::from_repo_path(config, &git_root)?
    };

    // Get session name
    let session = get_session_name(session_name)?;

    // Calculate workspace path
    let workspace_path = repo_id.workspace_path(config, workspace_type, &session);

    println!(
        "Creating new {} workspace:",
        match workspace_type {
            crate::path::WorkspaceType::Git => "git worktree",
            crate::path::WorkspaceType::Jj => "jj workspace",
        }
    );
    println!("  Git root: {}", git_root.display());
    println!("  Workspace: {}", workspace_path.display());
    println!("  Session: {}", session);

    // Create workspace directory
    std::fs::create_dir_all(&workspace_path)?;

    // Run the appropriate CLI command from git_root
    match workspace_type {
        crate::path::WorkspaceType::Git => {
            create_git_worktree_from_repo(&workspace_path, &git_root, &session)?;
        }
        crate::path::WorkspaceType::Jj => {
            create_jj_workspace_from_repo(&workspace_path, &git_root)?;
        }
    }

    println!(
        "\n✓ Successfully created workspace at: {}",
        workspace_path.display()
    );

    Ok(())
}

/// Create a new jj workspace from a git repository
fn create_jj_workspace_from_repo(workspace_path: &Path, git_root: &Path) -> Result<()> {
    println!("Initializing jj workspace...");

    // Initialize jj workspace using jj git init command with --no-colocate
    let output = std::process::Command::new("jj")
        .args(&[
            "git",
            "init",
            "--git-repo",
            path_to_str(git_root)?,
            "--no-colocate",
            path_to_str(workspace_path)?,
        ])
        .output()?;

    if !output.status.success() {
        bail!(
            "Failed to initialize jj workspace: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("  ✓ JJ workspace created successfully");

    Ok(())
}

/// Create a new git worktree from a git repository
fn create_git_worktree_from_repo(
    workspace_path: &Path,
    git_root: &Path,
    branch: &str,
) -> Result<()> {
    // Check if branch exists
    let check_output = std::process::Command::new("git")
        .current_dir(git_root)
        .args(&["rev-parse", "--verify", &format!("refs/heads/{}", branch)])
        .output()?;

    let branch_exists = check_output.status.success();

    // Create worktree using git worktree add
    let mut args = vec!["worktree", "add"];

    // If branch doesn't exist, create it with -b flag
    if !branch_exists {
        args.push("-b");
        args.push(branch);
        args.push(path_to_str(workspace_path)?);
        println!("  Creating new branch: {}", branch);
    } else {
        args.push(path_to_str(workspace_path)?);
        args.push(branch);
        println!("  Using existing branch: {}", branch);
    }

    let output = std::process::Command::new("git")
        .current_dir(git_root)
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

/// Remove all workspaces and repositories for a given repo ID
pub fn remove_repo(config: &Config, repo_id: &RepoIdentifier, dry_run: bool) -> Result<()> {
    let paths_to_remove = vec![
        ("Git bare repo", repo_id.git_path(config)),
        ("JJ workspace", repo_id.jj_path(config)),
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

/// Interactively clean repositories and all their artifacts
pub fn clean_repos(config: &Config) -> Result<()> {
    use std::collections::BTreeSet;

    // Discover all git and jj repos
    let git_repos = RepoIdentifier::discover_git_repo_ids(config)?;
    let jj_repos = RepoIdentifier::discover_jj_repo_ids(config)?;

    // Collect all unique repo identifiers
    let all_repos: BTreeSet<_> = git_repos.into_iter().chain(jj_repos.into_iter()).collect();

    if all_repos.is_empty() {
        println!("No repositories found.");
        return Ok(());
    }

    // Create options for multi-select
    let options: Vec<String> = all_repos
        .iter()
        .map(|r| r.relative_path().display().to_string())
        .collect();

    // Prompt user to select repositories to delete
    let selected = inquire::MultiSelect::new(
        "Select repositories to delete (use Space to select, Enter to confirm):",
        options,
    )
    .prompt()?;

    if selected.is_empty() {
        println!("No repositories selected. Cancelled.");
        return Ok(());
    }

    println!("\nThe following repositories will be deleted:");
    for repo_name in &selected {
        println!("  - {}", repo_name);
    }

    // Final confirmation
    let confirm = inquire::Confirm::new("Are you sure you want to delete these repositories?")
        .with_default(false)
        .prompt()?;

    if !confirm {
        println!("Cancelled.");
        return Ok(());
    }

    // Delete each selected repository
    for repo_name in selected {
        // Find the RepoIdentifier for this repo
        let repo_id = all_repos
            .iter()
            .find(|r| r.relative_path().display().to_string() == repo_name)
            .ok_or_eyre("Failed to find repository")?;

        println!("\n{}", "=".repeat(60));
        remove_repo(config, repo_id, false)?;
    }

    println!("\n{}", "=".repeat(60));
    println!("✓ Cleanup complete!");

    Ok(())
}

/// List all repositories and show which have git/jj repos
pub fn list_repos(config: &Config) -> Result<()> {
    use crate::path::Workspace;
    use std::collections::{BTreeMap, BTreeSet};

    // Discover all git and jj repos
    let git_repos = RepoIdentifier::discover_git_repo_ids(config)?;
    let jj_repos = RepoIdentifier::discover_jj_repo_ids(config)?;

    // Discover all workspaces
    let git_workspaces = Workspace::discover_workspaces_git(config)?;
    let jj_workspaces = Workspace::discover_workspaces_jj(config)?;

    // Group workspaces by repo_id
    let mut git_ws_map: BTreeMap<&RepoIdentifier, Vec<&str>> = BTreeMap::new();
    for ws in &git_workspaces {
        git_ws_map.entry(&ws.repo_id).or_default().push(&ws.session);
    }

    let mut jj_ws_map: BTreeMap<&RepoIdentifier, Vec<&str>> = BTreeMap::new();
    for ws in &jj_workspaces {
        jj_ws_map.entry(&ws.repo_id).or_default().push(&ws.session);
    }

    // Collect all unique repo identifiers using chain and collect
    let all_repos: BTreeSet<_> = git_repos.into_iter().chain(jj_repos.into_iter()).collect();

    if all_repos.is_empty() {
        println!("No repositories found.");
        return Ok(());
    }

    // Calculate the maximum width needed for the repository column
    let max_width = all_repos
        .iter()
        .map(|r| r.relative_path().display().to_string().len())
        .max()
        .unwrap_or(10)
        .max(10); // Minimum width of "Repository" header

    println!("Repositories:");
    println!(
        "{:<width$} {:<8} {:<8} {:<30} {:<30}",
        "Repository",
        "Git",
        "JJ",
        "Git Workspaces",
        "JJ Workspaces",
        width = max_width
    );
    println!("{}", "-".repeat(max_width + 78));

    for repo_id in all_repos {
        let has_git = repo_id.git_path(config).exists();
        let has_jj = repo_id.jj_path(config).exists();

        let git_sessions = git_ws_map
            .get(&repo_id)
            .map(|sessions| sessions.join(", "))
            .unwrap_or_default();

        let jj_sessions = jj_ws_map
            .get(&repo_id)
            .map(|sessions| sessions.join(", "))
            .unwrap_or_default();

        println!(
            "{:<width$} {:<8} {:<8} {:<30} {:<30}",
            repo_id.relative_path().display(),
            has_git,
            has_jj,
            git_sessions,
            jj_sessions,
            width = max_width
        );
    }

    Ok(())
}

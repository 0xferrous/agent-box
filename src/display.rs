use eyre::Result;
use std::path::Path;

use crate::config::Config;
use crate::path::RepoIdentifier;
use crate::repo::get_repo_path;

/// Display git worktrees for a repository
pub fn display_git_worktrees(repo_id: &RepoIdentifier, config: &Config) -> Result<()> {
    println!("\n=== Git Worktrees ===\n");

    let worktrees = repo_id.git_worktrees(config)?;

    if worktrees.is_empty() {
        println!("  (No worktrees found)");
        return Ok(());
    }

    for worktree in worktrees {
        if worktree.is_main {
            println!("{} (main)", worktree.path.display());
        } else {
            let locked = if worktree.is_locked { " [locked]" } else { "" };
            let id = worktree.id.as_deref().unwrap_or("unknown");
            println!("{} [{}]{}", worktree.path.display(), id, locked);
        }
    }

    Ok(())
}

/// Display JJ workspace information for current repository
pub fn display_jj_workspace_info(config: &Config, repo_path: &Path) -> Result<()> {
    let repo_id = RepoIdentifier::from_repo_path(config, repo_path)?;
    let jj_workspace_path = repo_id.jj_path(config);

    println!("\n=== JJ Workspace ===\n");
    println!("JJ workspace path:   {}", jj_workspace_path.display());

    if jj_workspace_path.exists() {
        println!("Status:              Initialized");
    } else {
        println!("Status:              Not initialized");
    }

    Ok(())
}

/// Display all JJ workspaces for a specific repository
pub fn display_all_jj_workspaces(config: &Config, repo_path: &Path) -> Result<()> {
    println!("\n=== All JJ Workspaces ===\n");

    let repo_id = RepoIdentifier::from_repo_path(config, repo_path)?;
    let workspace_names = repo_id.jj_workspaces(config)?;

    if workspace_names.is_empty() {
        println!("  (No JJ workspaces found)");
        return Ok(());
    }

    for workspace_name in workspace_names {
        println!("  {}", workspace_name);
    }

    Ok(())
}

/// Display current repository information
pub fn display_current_repo_info(config: &Config) -> Result<()> {
    println!("\n=== Current Repository ===\n");

    let repo = match gix::discover(&std::env::current_dir()?) {
        Ok(repo) => repo,
        Err(_) => {
            println!("Not in a git repository");
            return Ok(());
        }
    };

    let repo_path = get_repo_path(&repo);
    println!("Current repo path:   {}", repo_path.display());

    let repo_id = RepoIdentifier::from_repo_path(config, &repo_path)?;
    let bare_repo_path = repo_id.git_path(config);
    println!("Bare repo location:  {}", bare_repo_path.display());

    if bare_repo_path.exists() {
        if let Err(e) = display_git_worktrees(&repo_id, config) {
            eprintln!("  Error displaying git worktrees: {}", e);
        }
    } else {
        println!("(Bare repo does not exist yet - run 'ab export')");
    }

    if let Err(e) = display_jj_workspace_info(config, &repo_path) {
        eprintln!("\nCould not display JJ workspace info: {}", e);
    }

    if let Err(e) = display_all_jj_workspaces(config, &repo_path) {
        eprintln!("\nCould not display JJ workspaces: {}", e);
    }

    Ok(())
}

/// Show repository information and list workspaces
pub fn info(config: &Config) -> Result<()> {
    println!("=== Agent Box Configuration ===\n");
    println!("Git bare repos dir:  {}", config.git_dir.display());
    println!("JJ workspaces dir:   {}", config.jj_dir.display());
    println!("Workspace dir:       {}", config.workspace_dir.display());
    println!("Base repo dir:       {}", config.base_repo_dir.display());

    display_current_repo_info(config)?;

    Ok(())
}

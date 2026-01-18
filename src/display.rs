use eyre::Result;
use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::path::calculate_bare_repo_path;
use crate::repo::get_repo_path;

/// Display git worktrees for a bare repository
pub fn display_git_worktrees(bare_repo_path: &Path) -> Result<()> {
    println!("\n=== Git Worktrees ===\n");

    let bare_repo = gix::open(bare_repo_path)?;

    // List main worktree if it exists
    if let Some(wt) = bare_repo.worktree() {
        println!("{} (main)", wt.base().display());
    }

    // List all linked worktrees
    let worktrees = bare_repo.worktrees()?;
    if worktrees.is_empty() && bare_repo.worktree().is_none() {
        println!("  (No worktrees found)");
    }

    for proxy in worktrees {
        let base = proxy.base()?;
        let locked = if proxy.is_locked() { " [locked]" } else { "" };
        println!("{} [{}]{}", base.display(), proxy.id(), locked);
    }

    Ok(())
}

/// Display JJ workspace information for current repository
pub fn display_jj_workspace_info(config: &Config, repo_path: &Path) -> Result<()> {
    let jj_workspace_path =
        calculate_bare_repo_path(&config.base_repo_dir, repo_path, &config.jj_dir)?;

    println!("\n=== JJ Workspace ===\n");
    println!("JJ workspace path:   {}", jj_workspace_path.display());

    if jj_workspace_path.exists() {
        println!("Status:              Initialized");
    } else {
        println!("Status:              Not initialized");
    }

    Ok(())
}

/// Display all JJ workspaces found in the jj_dir
pub fn display_all_jj_workspaces(config: &Config) -> Result<()> {
    println!("\n=== All JJ Workspaces ===\n");

    if !config.jj_dir.exists() {
        println!(
            "  JJ workspaces directory does not exist: {}",
            config.jj_dir.display()
        );
        return Ok(());
    }

    let mut found_workspaces = false;
    for entry in fs::read_dir(&config.jj_dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join(".jj").exists() {
            println!("  {}", path.display());
            found_workspaces = true;
        }
    }

    if !found_workspaces {
        println!("  (No JJ workspaces found)");
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

    let bare_repo_path =
        calculate_bare_repo_path(&config.base_repo_dir, &repo_path, &config.git_dir)?;
    println!("Bare repo location:  {}", bare_repo_path.display());

    if bare_repo_path.exists() {
        if let Err(e) = display_git_worktrees(&bare_repo_path) {
            eprintln!("  Error displaying git worktrees: {}", e);
        }
    } else {
        println!("(Bare repo does not exist yet - run 'ab export')");
    }

    if let Err(e) = display_jj_workspace_info(config, &repo_path) {
        eprintln!("\nCould not display JJ workspace info: {}", e);
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
    display_all_jj_workspaces(config)?;

    Ok(())
}

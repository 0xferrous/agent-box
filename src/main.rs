use clap::{Parser, Subcommand};

mod config;
mod display;
mod docker;
mod path;
mod repo;

use config::load_config_or_exit;
use display::info;
use docker::{MountMode, spawn_container, spawn_oneoff_container};
use repo::{clean_repos, new_workspace, remove_repo, resolve_repo_id};

use crate::path::WorkspaceType;

#[derive(Parser)]
#[command(name = "ab")]
#[command(about = "Agent Box - Git repository management tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show repository information and list workspaces
    Info,
    /// Create a new workspace (jj or git worktree)
    New {
        /// Repository name (defaults to current directory's git repo)
        repo_name: Option<String>,
        /// Session/workspace name
        #[arg(long, short)]
        session: Option<String>,
        /// Create a git worktree
        #[arg(long)]
        git: bool,
        /// Create a jj workspace
        #[arg(long)]
        jj: bool,
    },
    /// Spawn a new docker container for a workspace
    Spawn {
        /// Session name
        session: String,
        /// Repository identifier (defaults to current directory's git repo)
        #[arg(long, short)]
        repo: Option<String>,
        /// Override entrypoint from config
        #[arg(long)]
        entrypoint: Option<String>,
        #[arg(long, conflicts_with = "jj")]
        git: bool,
        #[arg(long, conflicts_with = "git", default_value_t = true)]
        jj: bool,
        /// Create workspace if it doesn't exist (equivalent to running `ab new` first)
        #[arg(long, short)]
        new: bool,
    },
    /// Remove all workspaces and repositories for a given repo ID
    Remove {
        /// Repository identifier (e.g., "fr/agent-box" or "agent-box")
        repo: String,
        /// Show what would be deleted without actually deleting
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation prompt
        #[arg(long, short)]
        force: bool,
    },
    /// Interactively clean repositories and their artifacts
    Clean,
    /// Spawn a one-off container with the current directory mounted
    Oneoff {
        /// Mount as read-write (default is read-only)
        #[arg(short = 'w', long)]
        write: bool,
        /// Override entrypoint from config
        #[arg(long)]
        entrypoint: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    let config = load_config_or_exit();

    match cli.command {
        Commands::Info => {
            if let Err(e) = info(&config) {
                eprintln!("Error getting repository info: {}", e);
                std::process::exit(1);
            }
        }
        Commands::New {
            repo_name,
            session,
            git,
            jj,
        } => {
            let workspace_type = if git {
                WorkspaceType::Git
            } else if jj {
                WorkspaceType::Jj
            } else {
                // Default to jj if neither specified
                WorkspaceType::Jj
            };

            if let Err(e) = new_workspace(
                &config,
                repo_name.as_deref(),
                session.as_deref(),
                workspace_type,
            ) {
                eprintln!("Error creating new workspace: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Spawn {
            repo,
            session,
            entrypoint,
            git,
            jj: _,
            new: create_new,
        } => {
            let wtype = if git {
                WorkspaceType::Git
            } else {
                WorkspaceType::Jj
            };

            // Create workspace first if --new flag is set
            if create_new {
                if let Err(e) = new_workspace(&config, repo.as_deref(), Some(&session), wtype) {
                    eprintln!("Error creating new workspace: {}", e);
                    std::process::exit(1);
                }
            }

            // Resolve repo_id from repo argument
            let repo_id = match resolve_repo_id(&config, repo.as_deref()) {
                Ok(id) => id,
                Err(e) => {
                    eprintln!("Error resolving repository: {}", e);
                    std::process::exit(1);
                }
            };

            let runtime = tokio::runtime::Runtime::new().unwrap();
            if let Err(e) = runtime.block_on(spawn_container(
                &config,
                &repo_id,
                wtype,
                &session,
                entrypoint.as_deref(),
            )) {
                eprintln!("Error spawning container: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Remove {
            repo,
            dry_run,
            force,
        } => {
            use path::RepoIdentifier;

            // Locate the repository identifier
            let repo_id = match RepoIdentifier::locate(&config, &repo) {
                Ok(Some(id)) => id,
                Ok(None) => {
                    eprintln!("Error: Could not find repository matching '{}'", repo);
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Error locating repository: {}", e);
                    std::process::exit(1);
                }
            };

            // Show what will be removed (always, even if --force is used)
            if let Err(e) = remove_repo(&config, &repo_id, true) {
                eprintln!("Error listing files to remove: {}", e);
                std::process::exit(1);
            }

            // If dry-run, we're done
            if dry_run {
                return;
            }

            // Prompt for confirmation unless --force is used
            if !force {
                println!("\nAre you sure you want to remove these directories? [y/N] ");
                use std::io::{self, BufRead};
                let stdin = io::stdin();
                let mut line = String::new();
                stdin.lock().read_line(&mut line).unwrap();
                let answer = line.trim().to_lowercase();
                if answer != "y" && answer != "yes" {
                    println!("Cancelled.");
                    return;
                }
            }

            // Actually remove
            if let Err(e) = remove_repo(&config, &repo_id, false) {
                eprintln!("Error removing repository: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Clean => {
            if let Err(e) = clean_repos(&config) {
                eprintln!("Error cleaning repositories: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Oneoff { write, entrypoint } => {
            let cwd = std::env::current_dir().expect("Failed to get current directory");
            let mode = if write { MountMode::Rw } else { MountMode::Ro };
            let runtime = tokio::runtime::Runtime::new().unwrap();
            if let Err(e) = runtime.block_on(spawn_oneoff_container(
                &config,
                &cwd,
                mode,
                entrypoint.as_deref(),
            )) {
                eprintln!("Error spawning container: {}", e);
                std::process::exit(1);
            }
        }
    }
}

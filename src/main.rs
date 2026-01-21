use clap::{Parser, Subcommand};

mod config;
mod display;
mod docker;
mod path;
mod repo;

use config::load_config_or_exit;
use display::info;
use docker::spawn_container;
use repo::{convert_to_worktree, export_repo, init_jj, new_workspace, new_git_worktree};

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
    /// Export git repository, convert to worktree, and initialize jj workspace
    Export {
        /// Skip converting to worktree and initializing jj workspace
        #[arg(long)]
        no_convert: bool,
    },
    /// Initialize jj workspace backed by git bare repo
    InitJj,
    /// Convert current repo to worktree of bare repo
    ConvertToWorktree,
    /// Show repository information and list workspaces
    Info,
    /// Create a new workspace (jj or git worktree) for an existing repository
    New {
        /// Repository name to search for
        repo_name: Option<String>,
        /// Session/workspace name
        #[arg(long, short)]
        session: Option<String>,
        /// Create a git worktree instead of jj workspace
        #[arg(long, conflicts_with = "jj")]
        git: bool,
        /// Create a jj workspace (default)
        #[arg(long, conflicts_with = "git", default_value_t = true)]
        jj: bool,
    },
    /// Spawn a new docker container for a workspace
    Spawn {
        /// Repository identifier (e.g., "fr/agent-box" or "agent-box")
        repo: String,
        /// Session name
        session: String,
        /// Override entrypoint from config
        #[arg(long)]
        entrypoint: Option<String>,
        #[arg(long, conflicts_with = "jj")]
        git: bool,
        #[arg(long, conflicts_with = "git", default_value_t = true)]
        jj: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let config = load_config_or_exit();

    match cli.command {
        Commands::Export { no_convert } => {
            if let Err(e) = export_repo(&config, no_convert) {
                eprintln!("Error exporting repository: {}", e);
                std::process::exit(1);
            }
        }
        Commands::InitJj => {
            if let Err(e) = init_jj(&config) {
                eprintln!("Error initializing jj workspace: {}", e);
                std::process::exit(1);
            }
        }
        Commands::ConvertToWorktree => {
            if let Err(e) = convert_to_worktree(&config) {
                eprintln!("Error converting to worktree: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Info => {
            if let Err(e) = info(&config) {
                eprintln!("Error getting repository info: {}", e);
                std::process::exit(1);
            }
        }
        Commands::New { repo_name, session, git, jj: _ } => {
            if git {
                if let Err(e) = new_git_worktree(&config, repo_name.as_deref(), session.as_deref()) {
                    eprintln!("Error creating new git worktree: {}", e);
                    std::process::exit(1);
                }
            } else {
                if let Err(e) = new_workspace(&config, repo_name.as_deref(), session.as_deref()) {
                    eprintln!("Error creating new jj workspace: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Spawn {
            repo,
            session,
            entrypoint,
            git,
            jj: _,
        } => {
            use path::RepoIdentifier;
            let wtype = if git {
                WorkspaceType::Git
            } else {
                WorkspaceType::Jj
            };

            // Locate the repository identifier
            let repo_id = match RepoIdentifier::locate(&config, &repo) {
                Ok(Some(id)) => {
                    eprintln!(
                        "DEBUG: Located repository: {}",
                        id.relative_path().display()
                    );
                    id
                }
                Ok(None) => {
                    eprintln!("Error: Could not find repository matching '{}'", repo);
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Error locating repository: {}", e);
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
    }
}

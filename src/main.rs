use clap::{Parser, Subcommand};

mod config;
mod display;
mod path;
mod repo;

use config::load_config_or_exit;
use display::info;
use repo::{convert_to_worktree, export_repo, init_jj, new_workspace};

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
    /// Create a new jj workspace for an existing repository
    New {
        /// Repository name to search for
        repo_name: Option<String>,
        /// Session/workspace name
        #[arg(long, short)]
        session: Option<String>,
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
        Commands::New { repo_name, session } => {
            if let Err(e) = new_workspace(&config, repo_name.as_deref(), session.as_deref()) {
                eprintln!("Error creating new workspace: {}", e);
                std::process::exit(1);
            }
        }
    }
}

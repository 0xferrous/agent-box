use eyre::Result;

use crate::config::Config;
use crate::path::RepoIdentifier;

// ANSI color codes
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";

/// Show repository information and list workspaces
pub fn info(config: &Config) -> Result<()> {
    let cwd = std::env::current_dir()?;

    let repo = gix::discover(&cwd).ok();
    let repo_path = repo
        .as_ref()
        .and_then(|r| r.workdir().map(|p| p.to_path_buf()));

    let Some(repo_path) = repo_path else {
        eprintln!("Not in a git repository");
        return Ok(());
    };

    let repo_id = RepoIdentifier::from_repo_path(config, &repo_path)?;

    // Git worktrees
    println!("{BOLD}Git Worktrees:{RESET}");
    match repo_id.git_worktrees(config) {
        Ok(worktrees) if worktrees.is_empty() => {
            println!("  {DIM}(none){RESET}");
        }
        Ok(worktrees) => {
            for wt in worktrees {
                let path = wt.path.display();
                if wt.is_main {
                    println!("  {CYAN}{path}{RESET} {DIM}(main){RESET}");
                } else {
                    let id = wt.id.as_deref().unwrap_or("?");
                    let locked = if wt.is_locked {
                        format!(" {YELLOW}[locked]{RESET}")
                    } else {
                        String::new()
                    };
                    println!("  {CYAN}{path}{RESET} {GREEN}[{id}]{RESET}{locked}");
                }
            }
        }
        Err(e) => {
            eprintln!("  {DIM}Error: {e}{RESET}");
        }
    }

    println!();

    // JJ workspaces
    println!("{BOLD}JJ Workspaces:{RESET}");
    match repo_id.jj_workspaces(config) {
        Ok(workspaces) if workspaces.is_empty() => {
            println!("  {DIM}(none){RESET}");
        }
        Ok(workspaces) => {
            // Find max name length for alignment
            let max_name_len = workspaces.iter().map(|w| w.name.len()).max().unwrap_or(0);

            for ws in workspaces {
                let name = &ws.name;
                let commit = &ws.commit_id;
                let padding = " ".repeat(max_name_len - name.len());

                let desc = if ws.description.is_empty() {
                    format!("{DIM}(no description){RESET}")
                } else {
                    let first_line = ws.description.lines().next().unwrap_or("");
                    first_line.to_string()
                };

                let empty_marker = if ws.is_empty {
                    format!(" {DIM}(empty){RESET}")
                } else {
                    String::new()
                };

                println!(
                    "  {GREEN}{name}{RESET}{padding}  {MAGENTA}{commit}{RESET}  {desc}{empty_marker}"
                );
            }
        }
        Err(e) => {
            eprintln!("  {DIM}Error: {e}{RESET}");
        }
    }

    Ok(())
}

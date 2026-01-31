use clap::{Parser, Subcommand};

mod config;
mod display;
mod path;
mod repo;
mod runtime;

use config::{
    collect_profiles_to_apply, load_config, resolve_profiles, validate_config,
    validate_config_or_err,
};
use display::info;
use repo::{locate_repo, new_workspace, remove_repo, resolve_repo_id};
use runtime::{build_container_config, create_runtime};

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
    /// Spawn a new container for a workspace
    Spawn {
        /// Session name (mutually exclusive with --local)
        #[arg(
            long,
            short,
            conflicts_with = "local",
            required_unless_present = "local"
        )]
        session: Option<String>,
        /// Use current directory as both source and workspace (mutually exclusive with --session)
        #[arg(long, short, conflicts_with = "session")]
        local: bool,
        /// Repository identifier (defaults to current directory's git repo)
        #[arg(long, short)]
        repo: Option<String>,
        /// Override entrypoint from config
        #[arg(long, short)]
        entrypoint: Option<String>,
        /// Command to run in the container (passed to entrypoint)
        #[arg(long, short)]
        command: Option<Vec<String>>,
        #[arg(long, conflicts_with = "jj")]
        git: bool,
        #[arg(long, conflicts_with = "git", default_value_t = true)]
        jj: bool,
        /// Create workspace if it doesn't exist (equivalent to running `ab new` first)
        #[arg(long, short, conflicts_with = "local")]
        new: bool,
        /// Additional mount (home-relative). Format: [MODE:]PATH or [MODE:]SRC:DST
        /// MODE is ro, rw, or o (default: rw). Paths use ~ for home directory.
        /// Example: -m ~/.config/git -m ro:~/secrets -m rw:~/data:/app/data
        #[arg(long, short = 'm', value_name = "MOUNT")]
        mount: Vec<String>,
        /// Additional mount (absolute). Format: [MODE:]PATH or [MODE:]SRC:DST
        /// MODE is ro, rw, or o (default: rw). Same path used on host and container.
        /// Example: -M /nix/store -M ro:/etc/hosts
        #[arg(long = "Mount", short = 'M', value_name = "MOUNT")]
        mount_abs: Vec<String>,
        /// Additional profiles to apply (can be specified multiple times).
        /// Profiles are applied after the default_profile (if set) and in order specified.
        /// Example: -p git -p rust
        #[arg(long, short = 'p', value_name = "PROFILE")]
        profile: Vec<String>,
        /// Don't skip mounts that are already covered by parent mounts
        #[arg(long)]
        no_skip: bool,
    },
    /// Debug commands (hidden from main help)
    #[command(hide = true)]
    Dbg {
        #[command(subcommand)]
        command: DbgCommands,
    },
}

#[derive(Subcommand)]
enum DbgCommands {
    /// Locate a repository by partial path match (or list all if no search given)
    Locate {
        /// Repository search string (e.g., "agent-box" or "fr/agent-box")
        repo: Option<String>,
    },
    /// Remove all workspaces for a given repo ID
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
    /// Validate configuration (profiles, extends, default_profile)
    Validate,
    /// Show resolved/merged configuration from profiles
    Resolve {
        /// Profiles to apply (can be specified multiple times).
        /// If none specified, shows resolution with just default_profile (if set).
        /// Example: -p git -p rust
        #[arg(long, short = 'p', value_name = "PROFILE")]
        profile: Vec<String>,
    },
    /// Check if a path exists in a container image
    CheckPath {
        /// Container image to check (e.g., "nixos/nix:latest")
        image: String,
        /// Path to check in the image (e.g., "/nix/store")
        path: String,
    },
    /// List all paths (directories) in a container image
    ListPaths {
        /// Container image to inspect (e.g., "nixos/nix:latest")
        image: String,
        /// Root path to start listing from (defaults to "/")
        #[arg(long, short = 'p')]
        root_path: Option<String>,
        /// Filter paths containing this string
        #[arg(long, short = 'f')]
        filter: Option<String>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> eyre::Result<()> {
    let cli = Cli::parse();
    let config = load_config()?;

    match cli.command {
        Commands::Info => {
            info(&config)?;
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

            new_workspace(
                &config,
                repo_name.as_deref(),
                session.as_deref(),
                workspace_type,
            )?;
        }
        Commands::Spawn {
            repo,
            session,
            local,
            entrypoint,
            command,
            git,
            jj: _,
            new: create_new,
            mount,
            mount_abs,
            profile,
            no_skip,
        } => {
            let wtype = if git {
                WorkspaceType::Git
            } else {
                WorkspaceType::Jj
            };

            // Create workspace first if --new flag is set (only valid for session mode)
            if create_new {
                let session_name = session
                    .as_ref()
                    .expect("session required when --new is set");
                new_workspace(&config, repo.as_deref(), Some(session_name), wtype)?;
            }

            // Resolve repo_id from repo argument
            let repo_id = resolve_repo_id(&config, repo.as_deref())?;

            // Build container configuration
            let (workspace_path, source_path) = if local {
                let path = repo_id.source_path(&config);
                (path.clone(), path)
            } else {
                let session_name = session.as_ref().expect("session required");
                let workspace_path = repo_id.workspace_path(&config, wtype, session_name);
                let source_path = repo_id.source_path(&config);
                (workspace_path, source_path)
            };

            // Validate config before resolving profiles
            validate_config_or_err(&config)?;

            // Resolve profiles (default + CLI-specified)
            let resolved_profile = resolve_profiles(&config, &profile)?;

            // Parse CLI mount arguments
            let cli_mounts = runtime::parse_cli_mounts(&mount, &mount_abs)?;

            let container_config = match build_container_config(
                &config,
                &workspace_path,
                &source_path,
                local,
                entrypoint.as_deref(),
                &resolved_profile,
                &cli_mounts,
                command,
                !no_skip,
            ) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("Error building container config: {}", e);
                    std::process::exit(1);
                }
            };

            // Get the appropriate runtime backend
            let container_runtime = create_runtime(&config);

            // Spawn the container
            container_runtime.spawn_container(&container_config)?;
        }
        Commands::Dbg { command } => match command {
            DbgCommands::Locate { repo } => {
                let repo_id = locate_repo(&config, repo.as_deref())?;
                println!("{}", repo_id.relative_path().display());
            }
            DbgCommands::Remove {
                repo,
                dry_run,
                force,
            } => {
                // Locate the repository identifier
                let repo_id = locate_repo(&config, Some(&repo))?;

                // Show what will be removed (always, even if --force is used)
                remove_repo(&config, &repo_id, true)?;

                // If dry-run, we're done
                if dry_run {
                    return Ok(());
                }

                // Prompt for confirmation unless --force is used
                if !force {
                    let confirmed =
                        inquire::Confirm::new("Are you sure you want to remove these directories?")
                            .with_default(false)
                            .prompt()
                            .unwrap_or(false);

                    if !confirmed {
                        println!("Cancelled.");
                        return Ok(());
                    }
                }

                // Actually remove
                remove_repo(&config, &repo_id, false)?;
            }
            DbgCommands::Validate => {
                let result = validate_config(&config);

                // Print errors
                if !result.errors.is_empty() {
                    eprintln!("Errors:");
                    for error in &result.errors {
                        eprintln!("  ✗ {}", error);
                    }
                }

                // Print warnings
                if !result.warnings.is_empty() {
                    if !result.errors.is_empty() {
                        eprintln!();
                    }
                    eprintln!("Warnings:");
                    for warning in &result.warnings {
                        eprintln!("  ⚠ {}", warning);
                    }
                }

                // Print summary
                if result.is_ok() {
                    if result.has_warnings() {
                        println!(
                            "\nConfiguration valid with {} warning(s).",
                            result.warnings.len()
                        );
                    } else {
                        println!("Configuration valid. No errors or warnings.");
                    }

                    // Print profile summary
                    if !config.profiles.is_empty() {
                        println!("\nProfiles defined: {}", config.profiles.len());
                        for (name, profile) in &config.profiles {
                            let extends_info = if profile.extends.is_empty() {
                                String::new()
                            } else {
                                format!(" (extends: {})", profile.extends.join(", "))
                            };
                            println!("  - {}{}", name, extends_info);
                        }
                    }

                    if let Some(ref default) = config.default_profile {
                        println!("\nDefault profile: {}", default);
                    }
                } else {
                    eprintln!(
                        "\nConfiguration invalid: {} error(s), {} warning(s).",
                        result.errors.len(),
                        result.warnings.len()
                    );
                    std::process::exit(1);
                }
            }
            DbgCommands::Resolve { profile } => {
                // Validate config first
                validate_config_or_err(&config)?;

                // Show which profiles will be applied
                let profiles_applied = collect_profiles_to_apply(&config, &profile);

                if profiles_applied.is_empty() {
                    println!("No profiles to apply (no default_profile set, no -p flags)");
                    println!("\nBase runtime config:");
                } else {
                    println!(
                        "Profiles applied (in order): {}",
                        profiles_applied.join(" → ")
                    );
                    println!("\nResolved config:");
                }

                // Resolve profiles
                let resolved = resolve_profiles(&config, &profile)?;

                // Show mounts
                println!("\n  Mounts:");
                if resolved.mounts.is_empty() {
                    println!("    (none)");
                } else {
                    for m in &resolved.mounts {
                        match m.to_resolved_mounts() {
                            Ok(resolved_mounts) if resolved_mounts.is_empty() => {
                                // Path was filtered out (doesn't exist)
                                println!("    {} -> FILTERED (path does not exist)", m);
                            }
                            Ok(resolved_mounts) if resolved_mounts.len() == 1 => {
                                println!("    {} -> {}", m, resolved_mounts[0].to_bind_string());
                            }
                            Ok(resolved_mounts) => {
                                // Multiple resolved_mounts (symlink chain)
                                println!("    {} ->", m);
                                for rm in resolved_mounts {
                                    println!("      {}", rm.to_bind_string());
                                }
                            }
                            Err(e) => println!("    {} -> ERROR: {}", m, e),
                        }
                    }
                }

                // Show env
                println!("\n  Environment:");
                if resolved.env.is_empty() {
                    println!("    (none)");
                } else {
                    for e in &resolved.env {
                        println!("    {}", e);
                    }
                }
            }
            DbgCommands::CheckPath { image, path } => {
                let runtime = create_runtime(&config);

                println!("Checking if path exists in image...");
                println!("  Image: {}", image);
                println!("  Path: {}", path);
                println!();

                match runtime.path_exists_in_image(&image, &path) {
                    Ok(true) => {
                        println!("✓ Path exists in image");
                        std::process::exit(0);
                    }
                    Ok(false) => {
                        println!("✗ Path does not exist in image");
                        std::process::exit(1);
                    }
                    Err(e) => {
                        eprintln!("Error checking path: {}", e);
                        std::process::exit(2);
                    }
                }
            }
            DbgCommands::ListPaths {
                image,
                root_path,
                filter,
            } => {
                let runtime = create_runtime(&config);

                let root = root_path.as_deref();
                let root_display = root.unwrap_or("/");

                println!("Listing paths in image...");
                println!("  Image: {}", image);
                println!("  Root: {}", root_display);
                if let Some(f) = &filter {
                    println!("  Filter: {}", f);
                }
                println!();

                match runtime.list_paths_in_image(&image, root) {
                    Ok(paths) => {
                        let filtered_paths: Vec<_> = if let Some(f) = &filter {
                            paths
                                .into_iter()
                                .filter(|p| p.contains(f.as_str()))
                                .collect()
                        } else {
                            paths
                        };

                        if filtered_paths.is_empty() {
                            println!("No directories found");
                        } else {
                            println!("Found {} directories:", filtered_paths.len());
                            for path in filtered_paths {
                                println!("  {}", path);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error listing paths: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        },
    }

    Ok(())
}

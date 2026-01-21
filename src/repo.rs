use eyre::{OptionExt, Result, bail};
use nix::sys::stat::{Mode, stat};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::Config;
use crate::path::{
    RepoIdentifier, calculate_relative_path, path_to_str,
};

/// RAII guard for temporary directory that automatically cleans up on drop
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(path: PathBuf) -> Result<Self> {
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if self.path.exists() {
            if let Err(e) = fs::remove_dir_all(&self.path) {
                eprintln!(
                    "Warning: Failed to clean up temporary directory {}: {}",
                    self.path.display(),
                    e
                );
            }
        }
    }
}

/// Get repository path from a gix repository
pub fn get_repo_path(repo: &gix::Repository) -> PathBuf {
    if let Some(work_tree) = repo.workdir() {
        work_tree.to_path_buf()
    } else {
        repo.git_dir().to_path_buf()
    }
}

/// Configure a git repository for shared group access
/// Sets core.sharedRepository = group to ensure proper permissions on all git files
fn configure_shared_repository(repo_path: &Path) -> Result<()> {
    use std::io::Write;

    // Directly append to the config file
    // This is simpler than trying to parse and manipulate with gix config API
    let config_path = repo_path.join("config");

    // Read existing config to check if sharedRepository already exists
    let existing = fs::read_to_string(&config_path)?;

    if !existing.contains("sharedRepository") {
        // Append the setting to the config file
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&config_path)?;

        writeln!(file, "[core]")?;
        writeln!(file, "\tsharedRepository = group")?;
    }

    Ok(())
}

/// Set up directory with setgid bit
pub fn setup_directory_with_setgid(dir_path: &Path) -> Result<()> {
    if let Some(parent) = dir_path.parent() {
        fs::create_dir_all(parent)?;

        // Check if setgid bit is set on parent directory
        let file_stat = stat(parent)?;
        let current_mode = Mode::from_bits_truncate(file_stat.st_mode);

        if !current_mode.contains(Mode::S_ISGID) {
            let new_mode = current_mode | Mode::S_ISGID;
            println!("Setting setgid bit on directory: {}", parent.display());

            let permissions = fs::Permissions::from_mode(new_mode.bits());
            fs::set_permissions(parent, permissions)?;

            println!(
                "  Mode changed: {:o} -> {:o}",
                current_mode.bits(),
                new_mode.bits()
            );
        }
    }
    Ok(())
}

/// Discover repository in current directory
pub fn discover_repo() -> Result<gix::Repository> {
    use eyre::Context;

    let current_dir =
        std::env::current_dir().wrap_err("Failed to get current working directory")?;
    let repo = gix::discover(&current_dir).wrap_err_with(|| {
        format!(
            "Failed to discover git repository in {}",
            current_dir.display()
        )
    })?;
    Ok(repo)
}

/// Export git repository to bare repo
pub fn export_repo(config: &Config, no_convert: bool) -> Result<()> {
    let repo = discover_repo()?;

    // Check for uncommitted changes (only if not bare)
    if repo.workdir().is_some() {
        use gix::status::{Item, index_worktree};

        let status_iter = repo.status(gix::progress::Discard)?.into_iter(None)?;

        // Check for any tracked file changes (staged or unstaged)
        // We allow untracked files
        for item in status_iter {
            let item = item?;
            match item {
                Item::IndexWorktree(index_worktree::Item::DirectoryContents { .. }) => {
                    // Untracked files/directories - allowed
                    continue;
                }
                Item::IndexWorktree(index_worktree::Item::Modification { .. })
                | Item::IndexWorktree(index_worktree::Item::Rewrite { .. })
                | Item::TreeIndex(_) => {
                    // Staged or unstaged changes to tracked files - not allowed
                    bail!(
                        "Cannot export: repository has uncommitted changes to tracked files. Please commit or stash all changes first."
                    );
                }
            }
        }
    }

    // Get the work tree path (or git dir for bare repos)
    let repo_path = get_repo_path(&repo);

    let repo_id = RepoIdentifier::from_repo_path(config, &repo_path)?;
    let target_path = repo_id.git_path(config);

    println!("Exporting repository:");
    println!("  Source: {}", repo_path.display());
    println!("  Target: {}", target_path.display());

    // Create parent directories if they don't exist
    setup_directory_with_setgid(&target_path)?;

    // Setup progress reporting
    let progress = prodash::tree::Root::new();
    let sub_progress = progress.add_child("Cloning");

    // Setup line renderer for CLI output
    let render_handle = prodash::render::line(
        std::io::stderr(),
        Arc::downgrade(&progress),
        prodash::render::line::Options::default(),
    );

    // Clone with progress - pass Path references
    let _result = gix::prepare_clone_bare(repo_path.as_path(), target_path.as_path())?
        .fetch_only(sub_progress, &std::sync::atomic::AtomicBool::new(false))?;

    // Shutdown renderer
    drop(render_handle);

    // Configure the bare repository for shared group access
    // This ensures pack files and other git objects get proper group permissions
    configure_shared_repository(&target_path)?;

    println!("\nSuccessfully exported to: {}", target_path.display());

    // Convert to worktree and init jj by default unless --no-convert is specified
    if !no_convert {
        println!("\nConverting to worktree...");
        convert_to_worktree(config)?;

        println!("\nInitializing jj workspace...");
        init_jj(config)?;
    }

    Ok(())
}

/// Initialize jj workspace backed by git bare repo
pub fn init_jj(config: &Config) -> Result<()> {
    let repo = discover_repo()?;

    // Get the work tree path (or git dir for bare repos)
    let repo_path = get_repo_path(&repo);

    let repo_id = RepoIdentifier::from_repo_path(config, &repo_path)?;
    let bare_repo_path = repo_id.git_path(config);
    let jj_workspace_path = repo_id.jj_path(config);

    println!("Initializing jj workspace:");
    println!("  Git bare repo: {}", bare_repo_path.display());
    println!("  JJ workspace: {}", jj_workspace_path.display());

    // Create jj workspace directory
    setup_directory_with_setgid(&jj_workspace_path)?;

    fs::create_dir_all(&jj_workspace_path)?;

    // Initialize jj workspace with external git repo
    let config = jj_lib::config::StackedConfig::with_defaults();
    let user_settings = jj_lib::settings::UserSettings::from_config(config)?;

    let (_workspace, _repo) = jj_lib::workspace::Workspace::init_external_git(
        &user_settings,
        &jj_workspace_path,
        &bare_repo_path,
    )?;

    println!(
        "Successfully initialized jj workspace at: {}",
        jj_workspace_path.display()
    );

    Ok(())
}

/// Convert current repo to worktree of bare repo
pub fn convert_to_worktree(config: &Config) -> Result<()> {
    let repo = discover_repo()?;

    // Get the work tree path (error if bare repo)
    let repo_path = repo
        .workdir()
        .ok_or_eyre("Cannot convert a bare repository to worktree")?
        .to_path_buf();

    let repo_id = RepoIdentifier::from_repo_path(config, &repo_path)?;
    let bare_repo_path = repo_id.git_path(config);

    if !bare_repo_path.exists() {
        bail!(
            "Bare repository does not exist at: {}. Run 'ab export' first.",
            bare_repo_path.display()
        );
    }

    // Get current branch name
    let head = repo.head()?;
    let branch_name = if let Some(reference) = head.referent_name() {
        reference.as_bstr().to_string()
    } else {
        bail!("Repository is in detached HEAD state. Cannot convert to worktree.");
    };

    println!("Converting repository to worktree:");
    println!("  Current repo: {}", repo_path.display());
    println!("  Bare repo: {}", bare_repo_path.display());
    println!("  Branch: {}", branch_name);

    // Create temporary directory for worktree with RAII cleanup
    let temp_dir_path = std::env::temp_dir().join(format!("ab-worktree-{}", std::process::id()));
    let temp_dir = TempDir::new(temp_dir_path)?;

    println!(
        "Creating temporary worktree at: {}",
        temp_dir.path().display()
    );

    // Create worktree at temp location
    let output = std::process::Command::new("git")
        .args(&[
            "--git-dir",
            path_to_str(&bare_repo_path)?,
            "worktree",
            "add",
            path_to_str(temp_dir.path())?,
            &branch_name,
        ])
        .output()?;

    if !output.status.success() {
        bail!(
            "Failed to create worktree: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Remove current .git directory
    println!("Removing current .git directory");
    let current_git_dir = repo_path.join(".git");
    fs::remove_dir_all(&current_git_dir)?;

    // Copy .git file from temp to current location
    // Use copy instead of rename because temp and repo might be on different filesystems
    let temp_git_file = temp_dir.path().join(".git");
    let new_git_file = repo_path.join(".git");
    println!("Copying .git file from temp to current location");
    fs::copy(&temp_git_file, &new_git_file)?;

    // Use git worktree repair to fix all paths automatically
    println!("Repairing worktree paths with git worktree repair");
    let repair_output = std::process::Command::new("git")
        .args(&[
            "--git-dir",
            path_to_str(&bare_repo_path)?,
            "worktree",
            "repair",
            path_to_str(&repo_path)?,
        ])
        .output()?;

    if !repair_output.status.success() {
        eprintln!(
            "Warning: git worktree repair reported issues: {}",
            String::from_utf8_lossy(&repair_output.stderr)
        );
        eprintln!("Stdout: {}", String::from_utf8_lossy(&repair_output.stdout));
    } else {
        println!("  Worktree paths repaired successfully");
    }

    // Temp directory will be automatically cleaned up when temp_dir goes out of scope
    println!("Cleaning up temporary directory");

    println!("\nSuccessfully converted to worktree!");
    println!("  Worktree location: {}", repo_path.display());
    println!("  Backed by bare repo: {}", bare_repo_path.display());

    Ok(())
}

/// Create a new jj workspace for an existing bare repository
pub fn new_workspace(
    config: &Config,
    repo_name: Option<&str>,
    session_name: Option<&str>,
) -> Result<()> {
    // Step 1: Search for bare repos and get selection
    let bare_repo_path = find_and_select_bare_repo(config, repo_name)?;

    // Step 2: Get session name
    let session = get_session_name(session_name)?;

    // Step 3: Calculate paths
    let relative_path = calculate_relative_path(&config.git_dir, &bare_repo_path)?;
    let jj_repo_path = config.jj_dir.join(&relative_path);
    let workspace_path = config
        .workspace_dir
        .join("jj")
        .join(&relative_path)
        .join(&session);

    println!("\nPaths calculated:");
    println!("  Bare repo: {}", bare_repo_path.display());
    println!("  JJ repo: {}", jj_repo_path.display());
    println!("  New workspace: {}", workspace_path.display());

    // Step 4: Verify jj repo exists
    verify_jj_repo_exists(&jj_repo_path)?;

    // Step 5: Create workspace
    create_jj_workspace_at_path(&workspace_path, &jj_repo_path, &session)?;

    println!(
        "\nSuccessfully created workspace at: {}",
        workspace_path.display()
    );
    Ok(())
}

/// Recursively search for bare repositories by directory name
fn find_bare_repos_by_name(git_dir: &Path, search_name: &str) -> Result<Vec<PathBuf>> {
    let mut matches = Vec::new();

    fn visit_dirs(dir: &Path, search_name: &str, matches: &mut Vec<PathBuf>) -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        // Check if current directory is a bare git repo
        if dir.join("HEAD").exists() && dir.join("refs").exists() {
            // Match on directory name only (not full path)
            if let Some(dir_name) = dir.file_name() {
                if dir_name.to_string_lossy() == search_name {
                    matches.push(dir.to_path_buf());
                }
            }
            // Don't recurse into git repos
            return Ok(());
        }

        // Recurse into subdirectories
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, search_name, matches)?;
            }
        }

        Ok(())
    }

    visit_dirs(git_dir, search_name, &mut matches)?;
    Ok(matches)
}

/// Find and select a bare repository
fn find_and_select_bare_repo(config: &Config, repo_name: Option<&str>) -> Result<PathBuf> {
    // Prompt for repo name if not provided
    let name = match repo_name {
        Some(n) => n.to_string(),
        None => inquire::Text::new("Repository name:")
            .with_help_message("Enter the name of the repository to create a workspace for")
            .prompt()?,
    };

    // Search for matching repos
    let matches = find_bare_repos_by_name(&config.git_dir, &name)?;

    match matches.len() {
        0 => bail!("No repository found with name '{}'", name),
        1 => Ok(matches[0].clone()),
        _ => {
            // Multiple matches - prompt user to select
            let options: Vec<String> = matches.iter().map(|p| p.display().to_string()).collect();

            let selection =
                inquire::Select::new("Multiple repositories found. Select one:", options)
                    .prompt()?;

            // Find the selected path
            matches
                .iter()
                .find(|p| p.display().to_string() == selection)
                .cloned()
                .ok_or_eyre("Failed to find selected repository")
        }
    }
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

/// Verify that a jj repository exists at the given path
fn verify_jj_repo_exists(jj_repo_path: &Path) -> Result<()> {
    if !jj_repo_path.exists() {
        bail!(
            "JJ repository does not exist at: {}\nPlease run 'ab init-jj' first.",
            jj_repo_path.display()
        );
    }

    let jj_dir = jj_repo_path.join(".jj");
    if !jj_dir.exists() {
        bail!(
            "Directory exists but is not a JJ repository: {}\nMissing .jj directory",
            jj_repo_path.display()
        );
    }

    Ok(())
}

/// Create a new jj workspace at the specified path
fn create_jj_workspace_at_path(
    workspace_path: &Path,
    jj_repo_path: &Path,
    session: &str,
) -> Result<()> {
    println!("Creating JJ workspace:");
    println!("  JJ repo: {}", jj_repo_path.display());
    println!("  Workspace path: {}", workspace_path.display());
    println!("  Session name: {}", session);

    // Setup directory with setgid bit
    setup_directory_with_setgid(workspace_path)?;

    // Create workspace directory
    fs::create_dir_all(workspace_path)?;

    // Load the existing jj workspace to get the repo
    let config = jj_lib::config::StackedConfig::with_defaults();
    let user_settings = jj_lib::settings::UserSettings::from_config(config)?;
    let store_factories = jj_lib::repo::StoreFactories::default();
    let working_copy_factories = jj_lib::workspace::default_working_copy_factories();

    let existing_workspace = jj_lib::workspace::Workspace::load(
        &user_settings,
        jj_repo_path,
        &store_factories,
        &working_copy_factories,
    )?;

    // Create workspace name
    let workspace_name = jj_lib::ref_name::WorkspaceNameBuf::from(session);

    // Get the repo directory path (.jj directory)
    let repo_path = existing_workspace.repo_path();

    // Load the repo at head
    let repo = existing_workspace.repo_loader().load_at_head()?;

    // Initialize new workspace with existing repo
    let (_new_workspace, _repo) = jj_lib::workspace::Workspace::init_workspace_with_existing_repo(
        workspace_path,
        repo_path,
        &repo,
        &*jj_lib::workspace::default_working_copy_factory(),
        workspace_name,
    )?;

    Ok(())
}

/// Create a new git worktree for an existing bare repository
pub fn new_git_worktree(
    config: &Config,
    repo_name: Option<&str>,
    session_name: Option<&str>,
) -> Result<()> {
    // Step 1: Search for bare repos and get selection
    let bare_repo_path = find_and_select_bare_repo(config, repo_name)?;

    // Step 2: Get session name (which will also be the branch name)
    let session = get_session_name(session_name)?;

    // Step 3: Calculate paths
    let relative_path = calculate_relative_path(&config.git_dir, &bare_repo_path)?;
    let workspace_path = config
        .workspace_dir
        .join("git")
        .join(&relative_path)
        .join(&session);

    println!("\nPaths calculated:");
    println!("  Bare repo: {}", bare_repo_path.display());
    println!("  New worktree: {}", workspace_path.display());
    println!("  Branch: {}", session);

    // Step 4: Verify bare repo exists
    if !bare_repo_path.exists() {
        bail!(
            "Bare repository does not exist at: {}",
            bare_repo_path.display()
        );
    }

    // Step 5: Create worktree with branch name matching session
    create_git_worktree_at_path(&workspace_path, &bare_repo_path, &session)?;

    println!(
        "\nSuccessfully created git worktree at: {}",
        workspace_path.display()
    );
    Ok(())
}

/// Create a new git worktree at the specified path
fn create_git_worktree_at_path(
    workspace_path: &Path,
    bare_repo_path: &Path,
    branch: &str,
) -> Result<()> {
    println!("Creating git worktree:");
    println!("  Bare repo: {}", bare_repo_path.display());
    println!("  Worktree path: {}", workspace_path.display());
    println!("  Branch: {}", branch);

    // Setup directory with setgid bit
    setup_directory_with_setgid(workspace_path)?;

    // Check if branch exists
    let check_output = std::process::Command::new("git")
        .args(&[
            "--git-dir",
            path_to_str(bare_repo_path)?,
            "rev-parse",
            "--verify",
            &format!("refs/heads/{}", branch),
        ])
        .output()?;

    let branch_exists = check_output.status.success();

    // Create worktree using git worktree add
    let mut args = vec![
        "--git-dir",
        path_to_str(bare_repo_path)?,
        "worktree",
        "add",
    ];

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
        .args(&args)
        .output()?;

    if !output.status.success() {
        bail!(
            "Failed to create git worktree: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("  Git worktree created successfully");

    Ok(())
}

/// Remove all workspaces and repositories for a given repo ID
pub fn remove_repo(config: &Config, repo_id: &RepoIdentifier, dry_run: bool) -> Result<()> {
    let paths_to_remove = vec![
        ("Git bare repo", repo_id.git_path(config)),
        ("JJ workspace", repo_id.jj_path(config)),
        ("Git worktrees", config.workspace_dir.join("git").join(repo_id.relative_path())),
        ("JJ workspaces", config.workspace_dir.join("jj").join(repo_id.relative_path())),
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
            fs::remove_dir_all(path)?;
            println!("  ✓ Removed");
        }
    }

    println!("\n✓ All workspaces and repositories removed successfully");

    Ok(())
}

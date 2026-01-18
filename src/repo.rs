use eyre::{OptionExt, Result, bail};
use nix::sys::stat::{Mode, stat, umask};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::Config;
use crate::path::{calculate_bare_repo_path, path_to_str};

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
                eprintln!("Warning: Failed to clean up temporary directory {}: {}", self.path.display(), e);
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

/// Discover repository with umask setup
pub fn discover_repo_with_umask() -> Result<gix::Repository> {
    use eyre::Context;

    // Set umask to 0002 so user and group have same permissions, others read-only
    // This gives: directories 0775 (rwxrwxr-x), files 0664 (rw-rw-r--)
    umask(Mode::from_bits_truncate(0o002));

    let current_dir = std::env::current_dir()
        .wrap_err("Failed to get current working directory")?;
    let repo = gix::discover(&current_dir)
        .wrap_err_with(|| format!("Failed to discover git repository in {}", current_dir.display()))?;
    Ok(repo)
}

/// Export git repository to bare repo
pub fn export_repo(config: &Config, no_convert: bool) -> Result<()> {
    let repo = discover_repo_with_umask()?;

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

    let target_path = calculate_bare_repo_path(&config.base_repo_dir, &repo_path, &config.git_dir)?;

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
    let repo = discover_repo_with_umask()?;

    // Get the work tree path (or git dir for bare repos)
    let repo_path = get_repo_path(&repo);

    // Calculate bare repo path (same as export)
    let bare_repo_path =
        calculate_bare_repo_path(&config.base_repo_dir, &repo_path, &config.git_dir)?;

    // Calculate jj workspace path using same relative path but with jj_dir as base
    let jj_workspace_path =
        calculate_bare_repo_path(&config.base_repo_dir, &repo_path, &config.jj_dir)?;

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
    let repo = discover_repo_with_umask()?;

    // Get the work tree path (error if bare repo)
    let repo_path = repo
        .workdir()
        .ok_or_eyre("Cannot convert a bare repository to worktree")?
        .to_path_buf();

    // Calculate bare repo path
    let bare_repo_path =
        calculate_bare_repo_path(&config.base_repo_dir, &repo_path, &config.git_dir)?;

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

    println!("Creating temporary worktree at: {}", temp_dir.path().display());

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

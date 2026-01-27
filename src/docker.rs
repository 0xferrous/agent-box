use eyre::{Context, Result};
use std::path::PathBuf;

use crate::config::Config;
use crate::path::{RepoIdentifier, WorkspaceType, expand_path};

/// Mount mode for one-off containers
#[derive(Debug, Clone, Copy)]
pub enum MountMode {
    Ro,
    Rw,
}

/// Configuration for running a container
#[derive(Debug)]
pub struct ContainerConfig {
    pub image: String,
    pub entrypoint: Option<Vec<String>>,
    pub user: String,
    pub working_dir: String,
    pub mounts: Vec<String>,
    pub env: Vec<String>,
}

pub async fn spawn_container(
    config: &Config,
    repo_id: &RepoIdentifier,
    wtype: WorkspaceType,
    session: &str,
    entrypoint_override: Option<&str>,
) -> Result<()> {
    let workspace_path = repo_id.workspace_path(config, wtype, session);

    if !workspace_path.exists() {
        return Err(eyre::eyre!(
            "Workspace path does not exist: {}",
            workspace_path.display()
        ));
    }

    // Build container configuration
    let container_config =
        build_container_config(config, repo_id, wtype, &workspace_path, entrypoint_override)?;

    // Run the container
    run_container(&container_config).await
}

/// Spawn a one-off container with the given directory mounted
pub async fn spawn_oneoff_container(
    config: &Config,
    dir: &PathBuf,
    mode: MountMode,
    entrypoint_override: Option<&str>,
) -> Result<()> {
    let container_config = build_oneoff_config(config, dir, mode, entrypoint_override)?;
    run_container(&container_config).await
}

/// Build container configuration for a one-off container
fn build_oneoff_config(
    config: &Config,
    dir: &PathBuf,
    mode: MountMode,
    entrypoint_override: Option<&str>,
) -> Result<ContainerConfig> {
    let dir_str = dir
        .canonicalize()
        .wrap_err("Failed to canonicalize directory")?
        .to_string_lossy()
        .to_string();

    let mode_str = match mode {
        MountMode::Ro => "ro",
        MountMode::Rw => "rw",
    };

    let mut binds = vec![format!("{}:{}:{}", dir_str, dir_str, mode_str)];
    add_config_mounts(config, &mut binds)?;

    let uid = nix::unistd::getuid().as_raw();
    let gid = nix::unistd::getgid().as_raw();

    let username = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_string());

    let entrypoint = entrypoint_override
        .map(|s| vec![s.to_string()])
        .or_else(|| config.docker.entrypoint.clone());

    let env = vec![
        format!("USER={}", username),
        format!("HOME=/home/{}", username),
    ];

    Ok(ContainerConfig {
        image: config.docker.image.clone(),
        entrypoint,
        user: format!("{}:{}", uid, gid),
        working_dir: dir_str,
        mounts: binds,
        env,
    })
}

/// Add config-defined mounts to the binds vector
fn add_config_mounts(config: &Config, binds: &mut Vec<String>) -> Result<()> {
    let pb_to_str = |pb: &PathBuf| {
        pb.canonicalize()
            .expect(&format!("couldnt canonicalize: {pb:?}"))
            .to_string_lossy()
            .to_string()
    };
    let mount_path = |path: &str, mode: &str| format!("{path}:{path}:{mode}");
    let mount_path_custom = |src: &str, dst: &str, mode: &str| format!("{src}:{dst}:{mode}");
    let mount_path_ro = |path: &str| mount_path(path, "ro");
    let mount_path_rw = |path: &str| mount_path(path, "rw");

    // Process read-only absolute mounts
    for dir in &config.docker.mounts.ro.absolute {
        let expanded = expand_path(&PathBuf::from(dir))
            .wrap_err(format!("Failed to expand ro.absolute path: {}", dir))?;

        if !expanded.exists() {
            return Err(eyre::eyre!(
                "Read-only absolute mount does not exist: {}",
                dir
            ));
        }

        binds.push(mount_path_ro(&pb_to_str(&expanded)));
    }

    // Process read-only home_relative mounts
    for dir in &config.docker.mounts.ro.home_relative {
        let (host_path, container_path) = resolve_home_relative_mount(dir)?;

        let host_pathbuf = PathBuf::from(&host_path);
        if !host_pathbuf.exists() {
            return Err(eyre::eyre!(
                "Read-only home_relative mount does not exist: {}",
                dir
            ));
        }

        binds.push(mount_path_custom(&host_path, &container_path, "ro"));
    }

    // Process read-write absolute mounts
    for dir in &config.docker.mounts.rw.absolute {
        let expanded = expand_path(&PathBuf::from(dir))
            .wrap_err(format!("Failed to expand rw.absolute path: {}", dir))?;

        if !expanded.exists() {
            return Err(eyre::eyre!(
                "Read-write absolute mount does not exist: {}",
                dir
            ));
        }

        binds.push(mount_path_rw(&pb_to_str(&expanded)));
    }

    // Process read-write home_relative mounts
    for dir in &config.docker.mounts.rw.home_relative {
        let (host_path, container_path) = resolve_home_relative_mount(dir)?;

        let host_pathbuf = PathBuf::from(&host_path);
        if !host_pathbuf.exists() {
            return Err(eyre::eyre!(
                "Read-write home_relative mount does not exist: {}",
                dir
            ));
        }

        binds.push(mount_path_custom(&host_path, &container_path, "rw"));
    }

    Ok(())
}

/// Build container configuration from workspace and config
fn build_container_config(
    config: &Config,
    repo_id: &RepoIdentifier,
    _wtype: WorkspaceType,
    workspace_path: &PathBuf,
    entrypoint_override: Option<&str>,
) -> Result<ContainerConfig> {
    let pb_to_str = |pb: &PathBuf| {
        pb.canonicalize()
            .expect(&format!("couldnt canonicalize: {pb:?}"))
            .to_string_lossy()
            .to_string()
    };
    let mount_path_rw = |path: &str| format!("{path}:{path}:rw");

    let workspace_path_str = pb_to_str(&workspace_path);

    // Mount source repo's .git and .jj directories (not the whole source)
    let source_path = repo_id.source_path(config);
    let source_git = source_path.join(".git");
    let source_jj = source_path.join(".jj");

    let mut binds = vec![mount_path_rw(&workspace_path_str)];

    if source_git.exists() {
        binds.push(mount_path_rw(&pb_to_str(&source_git)));
    }
    if source_jj.exists() {
        binds.push(mount_path_rw(&pb_to_str(&source_jj)));
    }

    add_config_mounts(config, &mut binds)?;

    let uid = nix::unistd::getuid().as_raw();
    let gid = nix::unistd::getgid().as_raw();

    let username = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_string());

    let entrypoint = entrypoint_override
        .map(|s| vec![s.to_string()])
        .or_else(|| config.docker.entrypoint.clone());

    let env = vec![
        format!("USER={}", username),
        format!("HOME=/home/{}", username),
    ];

    Ok(ContainerConfig {
        image: config.docker.image.clone(),
        entrypoint,
        user: format!("{}:{}", uid, gid),
        working_dir: workspace_path_str,
        mounts: binds,
        env,
    })
}

/// Run a container with the given configuration using Docker CLI
async fn run_container(config: &ContainerConfig) -> Result<()> {
    eprintln!("DEBUG: Creating container with:");
    eprintln!("  Image: {}", config.image);
    eprintln!("  Entrypoint: {:?}", config.entrypoint);
    eprintln!("  User: {}", config.user);
    eprintln!("  Working dir: {}", config.working_dir);
    eprintln!("  Mounts: {} volumes", config.mounts.len());
    eprintln!("  Env vars: {} variables", config.env.len());

    let mut args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "-it".to_string(),
        "--user".to_string(),
        config.user.clone(),
        "--workdir".to_string(),
        config.working_dir.clone(),
    ];

    // Add mounts
    for mount in &config.mounts {
        args.push("-v".to_string());
        args.push(mount.clone());
    }

    // Add environment variables
    for env in &config.env {
        args.push("-e".to_string());
        args.push(env.clone());
    }

    // Add entrypoint if specified
    if let Some(entrypoint) = &config.entrypoint {
        args.push("--entrypoint".to_string());
        args.push(entrypoint.join(" "));
    }

    // Add image
    args.push(config.image.clone());

    eprintln!("DEBUG: Running: docker {}", args.join(" "));

    // Execute docker run with inherited stdio
    let status = std::process::Command::new("docker")
        .args(&args)
        .status()
        .wrap_err("Failed to execute docker command")?;

    if !status.success() {
        return Err(eyre::eyre!(
            "Docker container exited with status: {}",
            status
        ));
    }

    Ok(())
}

/// Resolve a home_relative mount path
/// Takes a host path (e.g., "~/dev/patched") and returns (host_path, container_path)
/// where container_path is relative to the container user's home directory
fn resolve_home_relative_mount(host_path: &str) -> Result<(String, String)> {
    // Expand the host path
    let expanded_host = expand_path(&PathBuf::from(host_path)).wrap_err(format!(
        "Failed to expand home_relative path: {}",
        host_path
    ))?;

    // Get the host's home directory
    let host_home = std::env::var("HOME").wrap_err("Failed to get HOME environment variable")?;
    let host_home_path = PathBuf::from(&host_home);

    // Get the current user for container path
    let container_user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_string());

    // Get the relative path from host's home
    let rel_path = expanded_host
        .strip_prefix(&host_home_path)
        .wrap_err(format!(
            "Path {} is not relative to home directory {}",
            expanded_host.display(),
            host_home_path.display()
        ))?;

    // Construct container path
    let container_path = PathBuf::from("/home").join(container_user).join(rel_path);

    // Canonicalize and convert to strings
    let host_str = expanded_host
        .canonicalize()
        .wrap_err(format!(
            "Failed to canonicalize path: {}",
            expanded_host.display()
        ))?
        .to_string_lossy()
        .to_string();
    let container_str = container_path.to_string_lossy().to_string();

    Ok((host_str, container_str))
}

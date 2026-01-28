pub mod docker;
pub mod podman;

use docker::ContainerBackend;
use eyre::Result;
use std::path::PathBuf;

use crate::config::Config;

/// Configuration for running a container
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    pub image: String,
    pub entrypoint: Option<Vec<String>>,
    pub user: String,
    pub working_dir: String,
    pub mounts: Vec<String>,
    pub env: Vec<String>,
}

/// Enum of available container runtimes
pub enum Runtime {
    Docker(docker::DockerRuntime),
    Podman(podman::PodmanRuntime),
}

impl Runtime {
    /// Spawn a container with the given configuration
    pub fn spawn_container(&self, config: &ContainerConfig) -> Result<()> {
        match self {
            Runtime::Docker(rt) => rt.spawn_container(config),
            Runtime::Podman(rt) => rt.spawn_container(config),
        }
    }

    /// Get the name of this runtime (e.g., "docker", "podman")
    pub fn name(&self) -> &str {
        match self {
            Runtime::Docker(rt) => rt.name(),
            Runtime::Podman(rt) => rt.name(),
        }
    }
}

/// Factory to create the appropriate container runtime
pub fn create_runtime(config: &Config) -> Runtime {
    match config.runtime.backend.as_str() {
        "podman" => Runtime::Podman(podman::PodmanRuntime::new()),
        _ => Runtime::Docker(docker::DockerRuntime::new()),
    }
}

/// Build container configuration from workspace and source paths
/// - workspace_path: the directory to mount as working directory (rw)
/// - source_path: the source repo to mount .git/.jj from
/// - local: if true, workspace and source are the same, so don't double-mount
pub fn build_container_config(
    config: &Config,
    workspace_path: &PathBuf,
    source_path: &PathBuf,
    local: bool,
    entrypoint_override: Option<&str>,
) -> Result<ContainerConfig> {
    let pb_to_str = |pb: &PathBuf| {
        pb.canonicalize()
            .expect(&format!("couldnt canonicalize: {pb:?}"))
            .to_string_lossy()
            .to_string()
    };
    let mount_path_rw = |path: &str| format!("{path}:{path}:rw");

    let workspace_path_str = pb_to_str(workspace_path);

    let mut binds = vec![mount_path_rw(&workspace_path_str)];

    // Mount source repo's .git and .jj directories only if not local
    // (in local mode, workspace IS the source, so they're already included)
    if !local {
        let source_git = source_path.join(".git");
        let source_jj = source_path.join(".jj");

        if source_git.exists() {
            binds.push(mount_path_rw(&pb_to_str(&source_git)));
        }
        if source_jj.exists() {
            binds.push(mount_path_rw(&pb_to_str(&source_jj)));
        }
    }

    // Check for overlay mounts and validate backend
    let has_overlay_mounts = !config.runtime.mounts.o.absolute.is_empty()
        || !config.runtime.mounts.o.home_relative.is_empty();

    if has_overlay_mounts && config.runtime.backend != "podman" {
        return Err(eyre::eyre!(
            "Overlay mounts are only supported with Podman backend, but '{}' is configured",
            config.runtime.backend
        ));
    }

    add_config_mounts(config, &mut binds)?;

    let uid = nix::unistd::getuid().as_raw();
    let gid = nix::unistd::getgid().as_raw();

    let username = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_string());

    let entrypoint = entrypoint_override
        .map(|s| vec![s.to_string()])
        .or_else(|| config.runtime.entrypoint.clone());

    let mut env = vec![
        format!("USER={}", username),
        format!("HOME=/home/{}", username),
    ];
    env.extend(config.runtime.env.iter().cloned());

    Ok(ContainerConfig {
        image: config.runtime.image.clone(),
        entrypoint,
        user: format!("{}:{}", uid, gid),
        working_dir: workspace_path_str,
        mounts: binds,
        env,
    })
}

/// Add config-defined mounts to the binds vector
fn add_config_mounts(config: &Config, binds: &mut Vec<String>) -> Result<()> {
    use eyre::WrapErr;

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
    let mount_path_overlay = |path: &str| format!("{path}:{path}:O");

    // Process read-only absolute mounts
    for dir in &config.runtime.mounts.ro.absolute {
        let expanded = crate::path::expand_path(&PathBuf::from(dir))
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
    for dir in &config.runtime.mounts.ro.home_relative {
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
    for dir in &config.runtime.mounts.rw.absolute {
        let expanded = crate::path::expand_path(&PathBuf::from(dir))
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
    for dir in &config.runtime.mounts.rw.home_relative {
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

    // Process overlay absolute mounts
    for dir in &config.runtime.mounts.o.absolute {
        let expanded = crate::path::expand_path(&PathBuf::from(dir))
            .wrap_err(format!("Failed to expand o.absolute path: {}", dir))?;

        if !expanded.exists() {
            return Err(eyre::eyre!(
                "Overlay absolute mount does not exist: {}",
                dir
            ));
        }

        binds.push(mount_path_overlay(&pb_to_str(&expanded)));
    }

    // Process overlay home_relative mounts
    for dir in &config.runtime.mounts.o.home_relative {
        let (host_path, container_path) = resolve_home_relative_mount(dir)?;

        let host_pathbuf = PathBuf::from(&host_path);
        if !host_pathbuf.exists() {
            return Err(eyre::eyre!(
                "Overlay home_relative mount does not exist: {}",
                dir
            ));
        }

        binds.push(mount_path_custom(&host_path, &container_path, "O"));
    }

    Ok(())
}

/// Resolve a home_relative mount path
/// Takes a host path (e.g., "~/dev/patched") and returns (host_path, container_path)
/// where container_path is relative to the container user's home directory
fn resolve_home_relative_mount(host_path: &str) -> Result<(String, String)> {
    use eyre::WrapErr;

    // Expand the host path
    let expanded_host = crate::path::expand_path(&PathBuf::from(host_path)).wrap_err(format!(
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

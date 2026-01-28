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
    let mount_path_custom = |src: &str, dst: &str, mode: &str| format!("{src}:{dst}:{mode}");

    let mounts = &config.runtime.mounts;

    // (mount_specs, home_relative, mode)
    let mount_groups: [(&[String], bool, &str); 6] = [
        (&mounts.ro.absolute, false, "ro"),
        (&mounts.ro.home_relative, true, "ro"),
        (&mounts.rw.absolute, false, "rw"),
        (&mounts.rw.home_relative, true, "rw"),
        (&mounts.o.absolute, false, "O"),
        (&mounts.o.home_relative, true, "O"),
    ];

    for (specs, home_relative, mode) in mount_groups {
        for mount_spec in specs {
            let (host_path, container_path) = resolve_mount(mount_spec, home_relative)?;

            if !PathBuf::from(&host_path).exists() {
                return Err(eyre::eyre!("Mount does not exist: {}", mount_spec));
            }

            binds.push(mount_path_custom(&host_path, &container_path, mode));
        }
    }

    Ok(())
}

/// Resolve a mount spec into (host_path, container_path).
///
/// Mount spec can be:
/// - A single path: uses same path for host and container (with home translation if `home_relative`)
/// - A `source:dest` mapping: explicit different paths
///
/// Paths must be absolute (`/...`) or home-relative (`~/...`).
///
/// The `home_relative` flag controls how single-path specs are handled:
/// - `home_relative = false` (absolute): `/home/host/.config` → `/home/host/.config` (same path)
/// - `home_relative = true`: `/home/host/.config` → `/home/container/.config` (home prefix replaced)
///
/// With explicit `source:dest` mapping, `~` expands to host home for source, container home for dest.
fn resolve_mount(mount_spec: &str, home_relative: bool) -> Result<(String, String)> {
    use eyre::WrapErr;

    let host_home = std::env::var("HOME").wrap_err("Failed to get HOME environment variable")?;
    let container_user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_string());
    let container_home = format!("/home/{}", container_user);

    let (host_expanded, container_path) =
        resolve_mount_with_homes(mount_spec, home_relative, &host_home, &container_home)?;

    // Canonicalize host path (must exist)
    let host_canonical = PathBuf::from(&host_expanded)
        .canonicalize()
        .wrap_err(format!(
            "Failed to canonicalize host path: {}",
            host_expanded
        ))?
        .to_string_lossy()
        .to_string();

    // If container path was derived from host path (no explicit dest, not home_relative),
    // we need to update it to use the canonical path
    let container_path = if container_path == host_expanded {
        host_canonical.clone()
    } else if home_relative && !mount_spec.contains(':') {
        // Re-derive with canonical path for home_relative
        if let Some(suffix) = host_canonical.strip_prefix(&host_home) {
            format!("{}{}", container_home, suffix)
        } else {
            host_canonical.clone()
        }
    } else {
        container_path
    };

    Ok((host_canonical, container_path))
}

/// Inner mount resolution logic, takes home directories as parameters for testability.
/// Returns (host_expanded, container_path) where host_expanded is NOT canonicalized.
fn resolve_mount_with_homes(
    mount_spec: &str,
    home_relative: bool,
    host_home: &str,
    container_home: &str,
) -> Result<(String, String)> {
    use eyre::WrapErr;

    // Split on ':' to check for explicit source:dest mapping
    let (host_spec, container_spec, has_explicit_dest) = match mount_spec.find(':') {
        Some(idx) => (&mount_spec[..idx], &mount_spec[idx + 1..], true),
        None => (mount_spec, mount_spec, false),
    };

    // Expand host path (~ -> host home)
    let host_expanded = expand_mount_path(host_spec, host_home)
        .wrap_err_with(|| format!("Invalid host path in mount: {}", mount_spec))?;

    // Determine container path
    let container_path = if has_explicit_dest {
        // Explicit dest: expand ~ to container home
        expand_mount_path(container_spec, container_home)
            .wrap_err_with(|| format!("Invalid container path in mount: {}", mount_spec))?
    } else if home_relative {
        // No explicit dest + home_relative: replace host home prefix with container home
        if let Some(suffix) = host_expanded.strip_prefix(host_home) {
            format!("{}{}", container_home, suffix)
        } else {
            // Path not under host home, use as-is
            host_expanded.clone()
        }
    } else {
        // No explicit dest + absolute: same path on both sides
        host_expanded.clone()
    };

    Ok((host_expanded, container_path))
}

/// Expand a mount path. Paths must be absolute (`/...`) or home-relative (`~/...`).
fn expand_mount_path(path: &str, home: &str) -> Result<String> {
    if path.starts_with('~') {
        Ok(path.replacen('~', home, 1))
    } else if path.starts_with('/') {
        Ok(path.to_string())
    } else {
        Err(eyre::eyre!(
            "Path must be absolute (/...) or home-relative (~/...): {}",
            path
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST_HOME: &str = "/home/hostuser";
    const CONTAINER_HOME: &str = "/home/containeruser";

    #[test]
    fn test_expand_mount_path_tilde() {
        assert_eq!(
            expand_mount_path("~/.config", HOST_HOME).unwrap(),
            "/home/hostuser/.config"
        );
    }

    #[test]
    fn test_expand_mount_path_absolute() {
        assert_eq!(
            expand_mount_path("/nix/store", HOST_HOME).unwrap(),
            "/nix/store"
        );
    }

    #[test]
    fn test_expand_mount_path_relative_rejected() {
        assert!(expand_mount_path(".config", HOST_HOME).is_err());
        assert!(expand_mount_path("config/git", HOST_HOME).is_err());
    }

    #[test]
    fn test_resolve_absolute_single_path() {
        // absolute (home_relative=false): same path on both sides
        let (host, container) =
            resolve_mount_with_homes("/nix/store", false, HOST_HOME, CONTAINER_HOME).unwrap();
        assert_eq!(host, "/nix/store");
        assert_eq!(container, "/nix/store");
    }

    #[test]
    fn test_resolve_absolute_single_path_with_tilde() {
        // absolute with ~: expands to host home, container gets same absolute path
        let (host, container) =
            resolve_mount_with_homes("~/.config", false, HOST_HOME, CONTAINER_HOME).unwrap();
        assert_eq!(host, "/home/hostuser/.config");
        assert_eq!(container, "/home/hostuser/.config"); // same path, NOT translated
    }

    #[test]
    fn test_resolve_home_relative_single_path() {
        // home_relative=true: host home prefix replaced with container home
        let (host, container) =
            resolve_mount_with_homes("~/.config", true, HOST_HOME, CONTAINER_HOME).unwrap();
        assert_eq!(host, "/home/hostuser/.config");
        assert_eq!(container, "/home/containeruser/.config"); // translated!
    }

    #[test]
    fn test_resolve_home_relative_path_not_under_home() {
        // home_relative=true but path not under home: use as-is
        let (host, container) =
            resolve_mount_with_homes("/nix/store", true, HOST_HOME, CONTAINER_HOME).unwrap();
        assert_eq!(host, "/nix/store");
        assert_eq!(container, "/nix/store");
    }

    #[test]
    fn test_resolve_explicit_mapping_absolute() {
        // Explicit source:dest mapping
        let (host, container) = resolve_mount_with_homes(
            "/host/path:/container/path",
            false,
            HOST_HOME,
            CONTAINER_HOME,
        )
        .unwrap();
        assert_eq!(host, "/host/path");
        assert_eq!(container, "/container/path");
    }

    #[test]
    fn test_resolve_explicit_mapping_with_tilde() {
        // Explicit mapping with ~ on dest side expands to container home
        let (host, container) = resolve_mount_with_homes(
            "/run/user/1000/gnupg:~/.gnupg",
            true,
            HOST_HOME,
            CONTAINER_HOME,
        )
        .unwrap();
        assert_eq!(host, "/run/user/1000/gnupg");
        assert_eq!(container, "/home/containeruser/.gnupg");
    }

    #[test]
    fn test_resolve_explicit_mapping_tilde_both_sides() {
        // ~ on both sides: host ~ -> host home, container ~ -> container home
        let (host, container) =
            resolve_mount_with_homes("~/.foo:~/.bar", false, HOST_HOME, CONTAINER_HOME).unwrap();
        assert_eq!(host, "/home/hostuser/.foo");
        assert_eq!(container, "/home/containeruser/.bar");
    }
}

pub mod docker;
pub mod podman;

use docker::ContainerBackend;
use eyre::Result;
use std::path::{Path, PathBuf};

use crate::config::{Config, Mount, MountMode, ResolvedMount, ResolvedProfile};

/// Pretty print a command with arguments, grouping flags with their values
pub(crate) fn print_command(command: &str, args: &[String]) {
    eprintln!("DEBUG: Running command:");
    eprintln!("  {} \\", command);
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let continuation = if i < args.len() - 1 { " \\" } else { "" };

        // Check if this is a flag with a value (flag starts with -, next arg doesn't)
        if arg.starts_with('-') && i + 1 < args.len() && !args[i + 1].starts_with('-') {
            eprintln!("    {} {}{}", arg, args[i + 1], continuation);
            i += 2; // Skip both the flag and its value
        } else {
            eprintln!("    {}{}", arg, continuation);
            i += 1;
        }
    }
}

/// Configuration for running a container
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    pub image: String,
    pub entrypoint: Option<Vec<String>>,
    pub command: Option<Vec<String>>,
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
}

/// Factory to create the appropriate container runtime
pub fn create_runtime(config: &Config) -> Runtime {
    match config.runtime.backend.as_str() {
        "podman" => Runtime::Podman(podman::PodmanRuntime::new()),
        _ => Runtime::Docker(docker::DockerRuntime::new()),
    }
}

/// Parse mode from string prefix (e.g., "ro:", "rw:", "o:")
fn parse_mode_prefix(s: &str) -> Option<(MountMode, &str)> {
    if let Some(rest) = s.strip_prefix("ro:") {
        Some((MountMode::Ro, rest))
    } else if let Some(rest) = s.strip_prefix("rw:") {
        Some((MountMode::Rw, rest))
    } else if let Some(rest) = s.strip_prefix("o:") {
        Some((MountMode::Overlay, rest))
    } else {
        None
    }
}

/// Parse CLI mount arguments into Mount structs.
///
/// Format: `[MODE:]PATH` or `[MODE:]SRC:DST`
/// - MODE is optional, defaults to "rw"
/// - Valid modes: "ro", "rw", "o"
///
/// Examples:
/// - `~/data` → mode=rw, spec=~/data
/// - `ro:~/config` → mode=ro, spec=~/config
/// - `rw:~/src:/app` → mode=rw, spec=~/src:/app
pub fn parse_cli_mounts(home_relative: &[String], absolute: &[String]) -> Result<Vec<Mount>> {
    let mut mounts = Vec::new();

    for spec in home_relative {
        mounts.push(parse_single_cli_mount(spec, true)?);
    }

    for spec in absolute {
        mounts.push(parse_single_cli_mount(spec, false)?);
    }

    Ok(mounts)
}

/// Parse a single CLI mount argument.
fn parse_single_cli_mount(arg: &str, home_relative: bool) -> Result<Mount> {
    // Check for mode prefix (ro:, rw:, o:)
    let (mode, spec) = match parse_mode_prefix(arg) {
        Some((mode, rest)) => (mode, rest.to_string()),
        None => (MountMode::Rw, arg.to_string()),
    };

    // Validate the spec is not empty
    if spec.is_empty() {
        return Err(eyre::eyre!("Empty mount path after mode prefix: {}", arg));
    }

    // Validate path format (must start with / or ~)
    let path_to_check = if spec.contains(':') {
        // For src:dst format, check the src part
        spec.split(':').next().unwrap()
    } else {
        &spec
    };

    if !path_to_check.starts_with('/') && !path_to_check.starts_with('~') {
        return Err(eyre::eyre!(
            "Mount path must be absolute (/...) or home-relative (~/...): {}",
            arg
        ));
    }

    Ok(Mount {
        spec,
        home_relative,
        mode,
    })
}

/// Build container configuration from workspace and source paths
/// - workspace_path: the directory to mount as working directory (rw)
/// - source_path: the source repo to mount .git/.jj from
/// - local: if true, workspace and source are the same, so don't double-mount
/// - resolved_profile: resolved mounts and env from profile resolution
/// - cli_mounts: additional mounts from CLI arguments
/// - command: command arguments to pass to the container entrypoint
/// - should_check: if true, perform mount validity checks
/// - should_skip: if true, skip mounts that are already covered by parent mounts
pub fn build_container_config(
    config: &Config,
    workspace_path: &Path,
    source_path: &Path,
    local: bool,
    entrypoint_override: Option<&str>,
    resolved_profile: &ResolvedProfile,
    cli_mounts: &[Mount],
    command: Option<Vec<String>>,
    should_check: bool,
    should_skip: bool,
) -> Result<ContainerConfig> {
    let pb_to_str = |pb: &Path| {
        pb.canonicalize()
            .unwrap_or_else(|_| panic!("couldnt canonicalize: {pb:?}"))
            .to_string_lossy()
            .to_string()
    };

    /// Format a mount as bind string (host:container:mode)
    pub fn format_bind(host_path: &Path, container_path: &Path, mode: MountMode) -> String {
        format!(
            "{}:{}:{}",
            host_path.display(),
            container_path.display(),
            mode.as_str()
        )
    }

    let workspace_path_str = pb_to_str(workspace_path);

    let mut binds = vec![format_bind(workspace_path, workspace_path, MountMode::Rw)];

    // Mount source repo's .git and .jj directories only if not local
    // (in local mode, workspace IS the source, so they're already included)
    if !local {
        let source_git = source_path.join(".git");
        let source_jj = source_path.join(".jj");

        if source_git.exists() {
            binds.push(format_bind(&source_git, &source_git, MountMode::Rw));
        }
        if source_jj.exists() {
            binds.push(format_bind(&source_jj, &source_jj, MountMode::Rw));
        }
    }

    // Combine profile mounts and CLI mounts
    let all_mounts: Vec<&Mount> = resolved_profile
        .mounts
        .iter()
        .chain(cli_mounts.iter())
        .collect();

    // Check for overlay mounts and validate backend
    let has_overlay = all_mounts.iter().any(|m| m.mode == MountMode::Overlay);

    if has_overlay && config.runtime.backend != "podman" {
        return Err(eyre::eyre!(
            "Overlay mounts are only supported with Podman backend, but '{}' is configured",
            config.runtime.backend
        ));
    }

    add_mounts(&all_mounts, &mut binds, should_check, should_skip)?;

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
    // Use env from resolved profile (includes runtime.env + profile envs)
    env.extend(resolved_profile.env.iter().cloned());

    Ok(ContainerConfig {
        image: config.runtime.image.clone(),
        entrypoint,
        command,
        user: format!("{}:{}", uid, gid),
        working_dir: workspace_path_str,
        mounts: binds,
        env,
    })
}

/// Check if a path is covered by any existing mount (exact match or subpath).
/// Returns Some(existing_mode) if covered, None if not covered.
fn find_covering_mount<'a>(
    host_path: &Path,
    existing_mounts: &'a [ResolvedMount],
) -> Option<&'a ResolvedMount> {
    for mount in existing_mounts {
        // Exact match - already mounted
        if host_path == mount.host {
            return Some(mount);
        }

        // Check if new path is under existing mount
        if host_path.starts_with(&mount.host) {
            return Some(mount);
        }
    }
    None
}

/// Check if mode combination is invalid (child under parent).
/// Returns true if the combination should error.
fn is_incompatible_mode_combination(parent: MountMode, child: MountMode) -> bool {
    matches!(
        (parent, child),
        (MountMode::Ro, MountMode::Rw) | (MountMode::Ro, MountMode::Overlay)
    )
}

/// Add mounts to the binds vector.
/// Handles symlinks by mounting the entire symlink chain.
/// Skips paths that are already covered by a parent mount (unless should_skip is false).
/// Returns error if trying to mount rw/overlay under a ro parent (unless should_check is false).
///
/// Mount mode compatibility matrix (existing parent → new child):
///
/// | Parent | Child | Action |
/// |--------|-------|--------|
/// | ro     | ro    | Skip (covered) [unless --no-skip] |
/// | ro     | rw    | **Error** (can't write under ro) [unless --no-check] |
/// | ro     | O     | **Error** (can't overlay under ro) [unless --no-check] |
/// | rw     | ro    | Skip (covered, ro ⊆ rw) [unless --no-skip] |
/// | rw     | rw    | Skip (covered) [unless --no-skip] |
/// | rw     | O     | Skip (covered) [unless --no-skip] |
/// | O      | ro    | Skip (covered) [unless --no-skip] |
/// | O      | rw    | Skip (covered) [unless --no-skip] |
/// | O      | O     | Skip (covered) [unless --no-skip] |
fn add_mounts(
    mounts: &[&Mount],
    binds: &mut Vec<String>,
    should_check: bool,
    should_skip: bool,
) -> Result<()> {
    // Parse existing binds into resolved mounts for coverage checking
    let mut existing_resolved: Vec<ResolvedMount> = binds
        .iter()
        .filter_map(|b| {
            let parts: Vec<&str> = b.split(':').collect();
            if parts.len() >= 3 {
                Some(ResolvedMount {
                    host: PathBuf::from(parts[0]),
                    container: PathBuf::from(parts[1]),
                    mode: parts[2].parse().unwrap_or(MountMode::Rw),
                })
            } else {
                None
            }
        })
        .collect();

    // First, resolve all mounts and collect them
    let mut all_resolved: Vec<ResolvedMount> = Vec::new();
    for mount in mounts {
        // to_resolved_mounts handles existence check and symlink chain
        let mount_resolved = mount.to_resolved_mounts()?;
        all_resolved.extend(mount_resolved);
    }

    // Sort by host path length (shortest first) so parent paths are processed before children.
    // This ensures that when a symlink chain resolves to paths under /nix/store,
    // the /nix/store mount is already in existing_resolved and coverage check works.
    all_resolved.sort_by(|a, b| {
        a.host
            .as_os_str()
            .len()
            .cmp(&b.host.as_os_str().len())
            .then_with(|| a.host.cmp(&b.host))
    });

    for resolved in all_resolved {
        if let Some(existing_mode) = find_covering_mount(&resolved.host, &existing_resolved) {
            // Check for invalid mode combinations (if should_check is true)
            if should_check && is_incompatible_mode_combination(existing_mode.mode, resolved.mode) {
                return Err(eyre::eyre!(
                    "Cannot mount '{}' as {} under read-only parent mount {}",
                    resolved.host.display(),
                    resolved.mode,
                    existing_mode.host.display()
                ));
            }
            // Skip if covered (unless should_skip is false)
            if !should_skip {
                // Add even though it's covered
                binds.push(resolved.to_bind_string());
                existing_resolved.push(resolved);
            }
            // Otherwise skip - already covered
        } else {
            // Not covered - add to existing resolved mounts and binds
            binds.push(resolved.to_bind_string());
            existing_resolved.push(resolved);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolved_mount_to_bind_string() {
        let resolved = ResolvedMount {
            host: PathBuf::from("/host/path"),
            container: PathBuf::from("/container/path"),
            mode: MountMode::Ro,
        };
        assert_eq!(resolved.to_bind_string(), "/host/path:/container/path:ro");
    }

    #[test]
    fn test_resolved_mount_overlay() {
        let resolved = ResolvedMount {
            host: PathBuf::from("/host"),
            container: PathBuf::from("/container"),
            mode: MountMode::Overlay,
        };
        assert_eq!(resolved.to_bind_string(), "/host:/container:O");
    }

    #[test]
    fn test_is_incompatible_mode_combination() {
        // ro parent
        assert!(is_incompatible_mode_combination(
            MountMode::Ro,
            MountMode::Rw
        ));
        assert!(is_incompatible_mode_combination(
            MountMode::Ro,
            MountMode::Overlay
        ));
        assert!(!is_incompatible_mode_combination(
            MountMode::Ro,
            MountMode::Ro
        ));

        // rw parent - all allowed
        assert!(!is_incompatible_mode_combination(
            MountMode::Rw,
            MountMode::Ro
        ));
        assert!(!is_incompatible_mode_combination(
            MountMode::Rw,
            MountMode::Rw
        ));
        assert!(!is_incompatible_mode_combination(
            MountMode::Rw,
            MountMode::Overlay
        ));

        // overlay parent - all allowed
        assert!(!is_incompatible_mode_combination(
            MountMode::Overlay,
            MountMode::Ro
        ));
        assert!(!is_incompatible_mode_combination(
            MountMode::Overlay,
            MountMode::Rw
        ));
        assert!(!is_incompatible_mode_combination(
            MountMode::Overlay,
            MountMode::Overlay
        ));
    }

    #[test]
    fn test_find_covering_mount_exact_match() {
        let mounts = vec![ResolvedMount {
            host: PathBuf::from("/host/path"),
            container: PathBuf::from("/container/path"),
            mode: MountMode::Ro,
        }];
        let result = find_covering_mount(Path::new("/host/path"), &mounts).map(|m| m.mode);
        assert_eq!(result, Some(MountMode::Ro));
    }

    #[test]
    fn test_find_covering_mount_subpath() {
        let mounts = vec![ResolvedMount {
            host: PathBuf::from("/nix/store"),
            container: PathBuf::from("/nix/store"),
            mode: MountMode::Ro,
        }];
        let result =
            find_covering_mount(Path::new("/nix/store/abc123-package"), &mounts).map(|m| m.mode);
        assert_eq!(result, Some(MountMode::Ro));
    }

    #[test]
    fn test_find_covering_mount_not_covered() {
        let mounts = vec![ResolvedMount {
            host: PathBuf::from("/nix/store"),
            container: PathBuf::from("/nix/store"),
            mode: MountMode::Ro,
        }];
        let result = find_covering_mount(Path::new("/home/user"), &mounts);
        assert_eq!(result, None);
    }

    const HOST_HOME: &str = "/home/hostuser";
    const CONTAINER_HOME: &str = "/home/containeruser";

    /// Helper to create a Mount and resolve with test homes (without canonicalization)
    fn resolve_test(spec: &str, home_relative: bool) -> (String, String) {
        let mount = Mount {
            spec: spec.to_string(),
            home_relative,
            mode: MountMode::Rw,
        };
        // Use resolve_paths directly to avoid canonicalization in tests
        mount.resolve_paths(HOST_HOME, CONTAINER_HOME).unwrap()
    }

    #[test]
    fn test_resolve_absolute_single_path() {
        // absolute (home_relative=false): same path on both sides
        let (host, container) = resolve_test("/nix/store", false);
        assert_eq!(host, "/nix/store");
        assert_eq!(container, "/nix/store");
    }

    #[test]
    fn test_resolve_absolute_single_path_with_tilde() {
        // absolute with ~: expands to host home, container gets same absolute path
        let (host, container) = resolve_test("~/.config", false);
        assert_eq!(host, "/home/hostuser/.config");
        assert_eq!(container, "/home/hostuser/.config"); // same path, NOT translated
    }

    #[test]
    fn test_resolve_home_relative_single_path() {
        // home_relative=true: host home prefix replaced with container home
        let (host, container) = resolve_test("~/.config", true);
        assert_eq!(host, "/home/hostuser/.config");
        assert_eq!(container, "/home/containeruser/.config"); // translated!
    }

    #[test]
    fn test_resolve_home_relative_path_not_under_home() {
        // home_relative=true but path not under home: use as-is
        let (host, container) = resolve_test("/nix/store", true);
        assert_eq!(host, "/nix/store");
        assert_eq!(container, "/nix/store");
    }

    #[test]
    fn test_resolve_explicit_mapping_absolute() {
        // Explicit source:dest mapping
        let (host, container) = resolve_test("/host/path:/container/path", false);
        assert_eq!(host, "/host/path");
        assert_eq!(container, "/container/path");
    }

    #[test]
    fn test_resolve_explicit_mapping_with_tilde() {
        // Explicit mapping with ~ on dest side expands to container home
        let (host, container) = resolve_test("/run/user/1000/gnupg:~/.gnupg", true);
        assert_eq!(host, "/run/user/1000/gnupg");
        assert_eq!(container, "/home/containeruser/.gnupg");
    }

    #[test]
    fn test_resolve_explicit_mapping_tilde_both_sides() {
        // ~ on both sides: host ~ -> host home, container ~ -> container home
        let (host, container) = resolve_test("~/.foo:~/.bar", false);
        assert_eq!(host, "/home/hostuser/.foo");
        assert_eq!(container, "/home/containeruser/.bar");
    }

    #[test]
    fn test_add_mounts_skips_covered_paths() {
        // Test that symlink chain paths under already-mounted directories are skipped
        let mut binds = vec!["/nix/store:/nix/store:ro".to_string()];

        // Create a temp symlink that points into /nix/store (simulated)
        let temp_dir = std::env::temp_dir().join(format!("ab_covered_{}", std::process::id()));
        let link_path = temp_dir.join("mylink");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create symlink to /nix/store (which exists)
        std::os::unix::fs::symlink("/nix/store", &link_path).unwrap();

        let mount = Mount {
            spec: link_path.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Ro,
        };

        add_mounts(&[&mount], &mut binds, true, true).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should have 2 mounts: original /nix/store and the symlink itself
        // The symlink target (/nix/store) should NOT be added again
        assert_eq!(binds.len(), 2);
        assert!(binds[0].starts_with("/nix/store:"));
        assert!(binds[1].contains("mylink"));
    }

    #[test]
    fn test_add_mounts_ro_under_rw_allowed() {
        // ro mount under rw parent should be skipped (covered)
        let temp_dir = std::env::temp_dir().join(format!("ab_ro_rw_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:rw", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Ro,
        };

        add_mounts(&[&mount], &mut binds, true, true).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should still have just 1 mount (subdir skipped as covered by parent)
        assert_eq!(binds.len(), 1);
    }

    #[test]
    fn test_add_mounts_rw_under_ro_error() {
        // rw mount under ro parent should error
        let temp_dir = std::env::temp_dir().join(format!("ab_rw_ro_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:ro", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Rw,
        };

        let result = add_mounts(&[&mount], &mut binds, true, true);

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should error
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("read-only"));
    }

    #[test]
    fn test_add_mounts_overlay_under_ro_error() {
        // overlay mount under ro parent should error
        let temp_dir = std::env::temp_dir().join(format!("ab_o_ro_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:ro", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Overlay,
        };

        let result = add_mounts(&[&mount], &mut binds, true, true);

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should error
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("read-only"));
    }

    #[test]
    fn test_add_mounts_ro_under_ro_skipped() {
        // ro mount under ro parent should be skipped
        let temp_dir = std::env::temp_dir().join(format!("ab_ro_ro_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:ro", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Ro,
        };

        add_mounts(&[&mount], &mut binds, true, true).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should still have just 1 mount
        assert_eq!(binds.len(), 1);
    }

    #[test]
    fn test_add_mounts_rw_under_rw_skipped() {
        // rw mount under rw parent should be skipped
        let temp_dir = std::env::temp_dir().join(format!("ab_rw_rw_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:rw", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Rw,
        };

        add_mounts(&[&mount], &mut binds, true, true).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should still have just 1 mount
        assert_eq!(binds.len(), 1);
    }

    #[test]
    fn test_add_mounts_overlay_under_rw_skipped() {
        // overlay mount under rw parent should be skipped
        let temp_dir = std::env::temp_dir().join(format!("ab_o_rw_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:rw", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Overlay,
        };

        add_mounts(&[&mount], &mut binds, true, true).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should still have just 1 mount
        assert_eq!(binds.len(), 1);
    }

    #[test]
    fn test_add_mounts_ro_under_overlay_skipped() {
        // ro mount under overlay parent should be skipped
        let temp_dir = std::env::temp_dir().join(format!("ab_ro_o_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:O", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Ro,
        };

        add_mounts(&[&mount], &mut binds, true, true).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should still have just 1 mount
        assert_eq!(binds.len(), 1);
    }

    #[test]
    fn test_add_mounts_rw_under_overlay_skipped() {
        // rw mount under overlay parent should be skipped
        let temp_dir = std::env::temp_dir().join(format!("ab_rw_o_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:O", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Rw,
        };

        add_mounts(&[&mount], &mut binds, true, true).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should still have just 1 mount
        assert_eq!(binds.len(), 1);
    }

    #[test]
    fn test_add_mounts_overlay_under_overlay_skipped() {
        // overlay mount under overlay parent should be skipped
        let temp_dir = std::env::temp_dir().join(format!("ab_o_o_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:O", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Overlay,
        };

        add_mounts(&[&mount], &mut binds, true, true).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should still have just 1 mount
        assert_eq!(binds.len(), 1);
    }

    #[test]
    fn test_add_mounts_rw_under_ro_with_no_check_allowed() {
        // rw mount under ro parent should succeed when should_check=false
        let temp_dir =
            std::env::temp_dir().join(format!("ab_rw_ro_nocheck_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:ro", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Rw,
        };

        // Should succeed with should_check=false
        add_mounts(&[&mount], &mut binds, false, true).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should still have just 1 mount (subdir skipped as covered)
        assert_eq!(binds.len(), 1);
    }

    #[test]
    fn test_add_mounts_overlay_under_ro_with_no_check_allowed() {
        // overlay mount under ro parent should succeed when should_check=false
        let temp_dir = std::env::temp_dir().join(format!("ab_o_ro_nocheck_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:ro", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Overlay,
        };

        // Should succeed with should_check=false
        add_mounts(&[&mount], &mut binds, false, true).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should still have just 1 mount (subdir skipped as covered)
        assert_eq!(binds.len(), 1);
    }

    #[test]
    fn test_add_mounts_rw_under_rw_with_no_skip() {
        // rw mount under rw parent should NOT be skipped when should_skip=false
        let temp_dir = std::env::temp_dir().join(format!("ab_rw_rw_noskip_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:rw", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Rw,
        };

        // Should add even though it's covered, with should_skip=false
        add_mounts(&[&mount], &mut binds, true, false).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should have 2 mounts (parent + child)
        assert_eq!(binds.len(), 2);
    }

    #[test]
    fn test_add_mounts_ro_under_rw_with_no_skip() {
        // ro mount under rw parent should NOT be skipped when should_skip=false
        let temp_dir = std::env::temp_dir().join(format!("ab_ro_rw_noskip_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:rw", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Ro,
        };

        // Should add even though it's covered, with should_skip=false
        add_mounts(&[&mount], &mut binds, true, false).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should have 2 mounts (parent + child)
        assert_eq!(binds.len(), 2);
    }

    #[test]
    fn test_add_mounts_rw_under_ro_with_no_check_and_no_skip() {
        // rw under ro should add when both should_check=false and should_skip=false
        let temp_dir =
            std::env::temp_dir().join(format!("ab_rw_ro_nocheck_noskip_{}", std::process::id()));
        let subdir = temp_dir.join("subdir");

        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&subdir).unwrap();

        let mut binds = vec![format!("{}:{}:ro", temp_dir.display(), temp_dir.display())];

        let mount = Mount {
            spec: subdir.to_string_lossy().to_string(),
            home_relative: false,
            mode: MountMode::Rw,
        };

        // Should add (no error because should_check=false, no skip because should_skip=false)
        add_mounts(&[&mount], &mut binds, false, false).unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Should have 2 mounts (parent + child)
        assert_eq!(binds.len(), 2);
    }

    #[test]
    fn test_mount_equality_same_spec() {
        let m1 = Mount {
            spec: "~/.config".to_string(),
            home_relative: true,
            mode: MountMode::Ro,
        };
        let m2 = Mount {
            spec: "~/.config".to_string(),
            home_relative: true,
            mode: MountMode::Ro,
        };
        assert_eq!(m1, m2);
    }

    #[test]
    fn test_mount_equality_different_mode() {
        let m1 = Mount {
            spec: "~/.config".to_string(),
            home_relative: true,
            mode: MountMode::Ro,
        };
        let m2 = Mount {
            spec: "~/.config".to_string(),
            home_relative: true,
            mode: MountMode::Rw,
        };
        assert_ne!(m1, m2);
    }

    #[test]
    fn test_mount_equality_equivalent_paths() {
        // These resolve to the same paths, so should be equal
        let m1 = Mount {
            spec: "/nix/store".to_string(),
            home_relative: false,
            mode: MountMode::Ro,
        };
        let m2 = Mount {
            spec: "/nix/store".to_string(),
            home_relative: true, // different flag, but resolves same
            mode: MountMode::Ro,
        };
        assert_eq!(m1, m2);
    }

    // CLI mount parsing tests

    #[test]
    fn test_parse_cli_mount_no_mode_defaults_to_rw() {
        let m = parse_single_cli_mount("~/data", true).unwrap();
        assert_eq!(m.mode, MountMode::Rw);
        assert_eq!(m.spec, "~/data");
        assert!(m.home_relative);
    }

    #[test]
    fn test_parse_cli_mount_absolute_no_mode() {
        let m = parse_single_cli_mount("/nix/store", false).unwrap();
        assert_eq!(m.mode, MountMode::Rw);
        assert_eq!(m.spec, "/nix/store");
        assert!(!m.home_relative);
    }

    #[test]
    fn test_parse_cli_mount_ro_mode() {
        let m = parse_single_cli_mount("ro:~/.config/git", true).unwrap();
        assert_eq!(m.mode, MountMode::Ro);
        assert_eq!(m.spec, "~/.config/git");
        assert!(m.home_relative);
    }

    #[test]
    fn test_parse_cli_mount_rw_mode() {
        let m = parse_single_cli_mount("rw:~/data", true).unwrap();
        assert_eq!(m.mode, MountMode::Rw);
        assert_eq!(m.spec, "~/data");
    }

    #[test]
    fn test_parse_cli_mount_overlay_mode() {
        let m = parse_single_cli_mount("o:~/.gnupg", true).unwrap();
        assert_eq!(m.mode, MountMode::Overlay);
        assert_eq!(m.spec, "~/.gnupg");
    }

    #[test]
    fn test_parse_cli_mount_with_src_dst() {
        let m = parse_single_cli_mount("ro:~/src:/app", true).unwrap();
        assert_eq!(m.mode, MountMode::Ro);
        assert_eq!(m.spec, "~/src:/app");
    }

    #[test]
    fn test_parse_cli_mount_absolute_with_mode() {
        let m = parse_single_cli_mount("ro:/etc/hosts", false).unwrap();
        assert_eq!(m.mode, MountMode::Ro);
        assert_eq!(m.spec, "/etc/hosts");
        assert!(!m.home_relative);
    }

    #[test]
    fn test_parse_cli_mount_empty_after_mode_fails() {
        assert!(parse_single_cli_mount("ro:", true).is_err());
    }

    #[test]
    fn test_parse_cli_mount_relative_path_fails() {
        assert!(parse_single_cli_mount("data/stuff", true).is_err());
        assert!(parse_single_cli_mount("ro:data/stuff", true).is_err());
    }

    #[test]
    fn test_parse_cli_mounts_mixed() {
        let home_rel = vec!["~/data".to_string(), "ro:~/.config".to_string()];
        let absolute = vec!["/nix/store".to_string(), "o:/tmp/overlay".to_string()];

        let mounts = parse_cli_mounts(&home_rel, &absolute).unwrap();

        assert_eq!(mounts.len(), 4);

        assert_eq!(mounts[0].spec, "~/data");
        assert_eq!(mounts[0].mode, MountMode::Rw);
        assert!(mounts[0].home_relative);

        assert_eq!(mounts[1].spec, "~/.config");
        assert_eq!(mounts[1].mode, MountMode::Ro);
        assert!(mounts[1].home_relative);

        assert_eq!(mounts[2].spec, "/nix/store");
        assert_eq!(mounts[2].mode, MountMode::Rw);
        assert!(!mounts[2].home_relative);

        assert_eq!(mounts[3].spec, "/tmp/overlay");
        assert_eq!(mounts[3].mode, MountMode::Overlay);
        assert!(!mounts[3].home_relative);
    }

    #[test]
    fn test_parse_cli_mounts_empty() {
        let mounts = parse_cli_mounts(&[], &[]).unwrap();
        assert!(mounts.is_empty());
    }

    #[test]
    fn test_parse_cli_mount_tilde_src_absolute_dst() {
        // ~/src:/app - tilde on source, absolute on dest
        let m = parse_single_cli_mount("rw:~/src:/app", true).unwrap();
        assert_eq!(m.mode, MountMode::Rw);
        assert_eq!(m.spec, "~/src:/app");
    }

    #[test]
    fn test_parse_cli_mount_absolute_src_tilde_dst() {
        // /host/path:~/data - absolute source, tilde dest
        let m = parse_single_cli_mount("/run/user/1000/socket:~/.gnupg/socket", true).unwrap();
        assert_eq!(m.mode, MountMode::Rw);
        assert_eq!(m.spec, "/run/user/1000/socket:~/.gnupg/socket");
    }

    #[test]
    fn test_mount_mode_as_str() {
        assert_eq!(MountMode::Ro.as_str(), "ro");
        assert_eq!(MountMode::Rw.as_str(), "rw");
        assert_eq!(MountMode::Overlay.as_str(), "O");
    }

    #[test]
    fn test_parse_mode_prefix() {
        assert_eq!(
            parse_mode_prefix("ro:~/data"),
            Some((MountMode::Ro, "~/data"))
        );
        assert_eq!(
            parse_mode_prefix("rw:/path"),
            Some((MountMode::Rw, "/path"))
        );
        assert_eq!(
            parse_mode_prefix("o:~/.gnupg"),
            Some((MountMode::Overlay, "~/.gnupg"))
        );
        assert_eq!(parse_mode_prefix("~/data"), None);
        assert_eq!(parse_mode_prefix("/nix/store"), None);
    }
}

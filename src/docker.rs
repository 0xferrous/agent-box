use bollard::Docker;
use bollard::models::{ContainerCreateBody, HostConfig};
use eyre::{Context, Result};
use futures_util::StreamExt;
use std::path::PathBuf;

use crate::config::Config;
use crate::path::{RepoIdentifier, WorkspaceType, expand_path};

use std::io::{Read, Write, stdout};
use termion::async_stdin;
use termion::raw::IntoRawMode;
use tokio::io::AsyncWriteExt;

pub async fn spawn_container(
    config: &Config,
    repo_id: &RepoIdentifier,
    wtype: WorkspaceType,
    session: &str,
    entrypoint_override: Option<&str>,
) -> Result<()> {
    let workspace_path = repo_id.workspace_path(config, wtype, session);

    let docker = Docker::connect_with_defaults().wrap_err("Failed to connect to Docker daemon")?;

    if !workspace_path.exists() {
        return Err(eyre::eyre!(
            "Workspace path does not exist: {}",
            workspace_path.display()
        ));
    }

    let pb_to_str = |pb: &PathBuf| pb.canonicalize().unwrap().to_string_lossy().to_string();
    let mount_path = |path: &str, mode: &str| format!("{path}:{path}:{mode}");
    let mount_path_custom = |src: &str, dst: &str, mode: &str| format!("{src}:{dst}:{mode}");
    let mount_path_ro = |path: &str| mount_path(path, "ro");
    let mount_path_rw = |path: &str| mount_path(path, "rw");

    let workspace_path_str = pb_to_str(&workspace_path);
    let backing_binds = match wtype {
        WorkspaceType::Git => vec![repo_id.git_path(config)],
        WorkspaceType::Jj => vec![repo_id.git_path(config), repo_id.jj_path(config)],
    };
    let more_binds = backing_binds
        .iter()
        .map(|it| {
            let path = pb_to_str(it);
            mount_path_rw(&path)
        })
        .collect::<Vec<_>>();

    let mut binds = vec![mount_path_rw(&workspace_path_str)];
    binds.extend(more_binds);

    // Process read-only absolute mounts
    for dir in &config.docker.mounts.ro.absolute {
        let expanded = expand_path(&PathBuf::from(dir))
            .wrap_err(format!("Failed to expand ro.absolute path: {}", dir))?;

        if !expanded.exists() {
            return Err(eyre::eyre!("Read-only absolute mount does not exist: {}", dir));
        }

        let expanded_str = pb_to_str(&expanded);
        binds.push(mount_path_ro(&expanded_str));
    }

    // Process read-only home_relative mounts
    for dir in &config.docker.mounts.ro.home_relative {
        let (host_path, container_path) = resolve_home_relative_mount(dir, &config.agent.user)?;

        let host_pathbuf = PathBuf::from(&host_path);
        if !host_pathbuf.exists() {
            return Err(eyre::eyre!("Read-only home_relative mount does not exist: {}", dir));
        }

        binds.push(mount_path_custom(&host_path, &container_path, "ro"));
    }

    // Process read-write absolute mounts
    for dir in &config.docker.mounts.rw.absolute {
        let expanded = expand_path(&PathBuf::from(dir))
            .wrap_err(format!("Failed to expand rw.absolute path: {}", dir))?;

        if !expanded.exists() {
            return Err(eyre::eyre!("Read-write absolute mount does not exist: {}", dir));
        }

        let expanded_str = pb_to_str(&expanded);
        binds.push(mount_path_rw(&expanded_str));
    }

    // Process read-write home_relative mounts
    for dir in &config.docker.mounts.rw.home_relative {
        let (host_path, container_path) = resolve_home_relative_mount(dir, &config.agent.user)?;

        let host_pathbuf = PathBuf::from(&host_path);
        if !host_pathbuf.exists() {
            return Err(eyre::eyre!("Read-write home_relative mount does not exist: {}", dir));
        }

        binds.push(mount_path_custom(&host_path, &container_path, "rw"));
    }

    let uid = get_uid(&config.agent.user)?;
    let gid = get_gid(&config.agent.group)?;

    let entrypoint = entrypoint_override
        .map(|s| vec![s.to_string()])
        .or_else(|| config.docker.entrypoint.clone());

    // Configure git safe.directory for the workspace and backing paths
    // Also configure core.sharedRepository to ensure proper group permissions
    let backing_paths_str = backing_binds
        .iter()
        .map(|pb| pb_to_str(pb))
        .collect::<Vec<_>>();

    let git_configs = std::iter::once(("core.sharedRepository", "group"))
        .chain(std::iter::once(("safe.directory", workspace_path_str.as_str())))
        .chain(backing_paths_str.iter().map(|p| ("safe.directory", p.as_str())));

    let git_env = build_git_config_env(git_configs);

    eprintln!("DEBUG: Creating container with:");
    eprintln!("  Image: {}", config.docker.image);
    eprintln!("  Entrypoint: {:?}", entrypoint);
    eprintln!("  User: {}:{}", uid, gid);
    eprintln!("  Working dir: {}", workspace_path_str);
    eprintln!("  Binds:");
    for bind in &binds {
        eprintln!("    {}", bind);
    }
    eprintln!("  Git config:");
    for env in &git_env {
        eprintln!("    {}", env);
    }

    let container_config = ContainerCreateBody {
        image: Some(config.docker.image.clone()),
        entrypoint: entrypoint.clone(),
        tty: Some(true),
        attach_stdin: Some(true),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        open_stdin: Some(true),
        env: Some(git_env),
        host_config: Some(HostConfig {
            binds: Some(binds),
            auto_remove: Some(true),
            ..Default::default()
        }),
        user: Some(format!("{}:{}", uid, gid)),
        working_dir: Some(workspace_path_str.clone()),
        ..Default::default()
    };

    eprintln!("DEBUG: Attempting to create container...");
    let container = docker
        .create_container(None, container_config)
        .await
        .wrap_err("Failed to create container")?;
    eprintln!("DEBUG: Container created with ID: {}", container.id);

    docker
        .start_container(&container.id, None)
        .await
        .wrap_err("Failed to start container")?;

    let bollard::container::AttachContainerResults {
        mut output,
        mut input,
    } = docker
        .attach_container(
            &container.id,
            Some(
                bollard::query_parameters::AttachContainerOptionsBuilder::default()
                    .stdout(true)
                    .stderr(true)
                    .stdin(true)
                    .stream(true)
                    .build(),
            ),
        )
        .await
        .wrap_err("Failed to attach to container")?;

    // Pipe stdin into the docker attach stream input
    tokio::task::spawn(async move {
        #[allow(clippy::unbuffered_bytes)]
        let mut stdin = async_stdin().bytes();
        loop {
            if let Some(Ok(byte)) = stdin.next() {
                let _ = input.write_all(&[byte]).await;
            } else {
                tokio::time::sleep(std::time::Duration::from_nanos(10)).await;
            }
        }
    });

    // Set stdout in raw mode so we can do tty stuff
    let stdout = stdout();
    let mut stdout = stdout
        .lock()
        .into_raw_mode()
        .wrap_err("Failed to set terminal to raw mode")?;

    // Pipe docker attach output into stdout
    while let Some(Ok(output)) = output.next().await {
        stdout
            .write_all(output.into_bytes().as_ref())
            .wrap_err("Failed to write to stdout")?;
        stdout.flush().wrap_err("Failed to flush stdout")?;
    }

    Ok(())
}

/// Build GIT_CONFIG_* environment variables from key-value pairs
fn build_git_config_env<'a>(configs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<String> {
    let pairs: Vec<(&str, &str)> = configs.into_iter().collect();
    let count = pairs.len();

    let mut env = vec![format!("GIT_CONFIG_COUNT={}", count)];

    for (i, (key, value)) in pairs.iter().enumerate() {
        env.push(format!("GIT_CONFIG_KEY_{}={}", i, key));
        env.push(format!("GIT_CONFIG_VALUE_{}={}", i, value));
    }

    env
}

/// Resolve a home_relative mount path
/// Takes a host path (e.g., "~/dev/patched") and returns (host_path, container_path)
/// where container_path is relative to the container user's home directory
fn resolve_home_relative_mount(
    host_path: &str,
    container_user: &str,
) -> Result<(String, String)> {
    // Expand the host path
    let expanded_host = expand_path(&PathBuf::from(host_path))
        .wrap_err(format!("Failed to expand home_relative path: {}", host_path))?;

    // Get the host's home directory
    let host_home = std::env::var("HOME")
        .wrap_err("Failed to get HOME environment variable")?;
    let host_home_path = PathBuf::from(&host_home);

    // Get the relative path from host's home
    let rel_path = expanded_host
        .strip_prefix(&host_home_path)
        .wrap_err(format!(
            "Path {} is not relative to home directory {}",
            expanded_host.display(),
            host_home_path.display()
        ))?;

    // Construct container path
    let container_path = PathBuf::from("/home")
        .join(container_user)
        .join(rel_path);

    // Canonicalize and convert to strings
    let host_str = expanded_host.canonicalize()
        .wrap_err(format!("Failed to canonicalize path: {}", expanded_host.display()))?
        .to_string_lossy()
        .to_string();
    let container_str = container_path.to_string_lossy().to_string();

    Ok((host_str, container_str))
}

fn get_uid(username: &str) -> Result<u32> {
    let user = nix::unistd::User::from_name(username)
        .wrap_err(format!("Failed to find user: {}", username))?
        .ok_or_else(|| eyre::eyre!("User not found: {}", username))?;
    Ok(user.uid.as_raw())
}

fn get_gid(groupname: &str) -> Result<u32> {
    let group = nix::unistd::Group::from_name(groupname)
        .wrap_err(format!("Failed to find group: {}", groupname))?
        .ok_or_else(|| eyre::eyre!("Group not found: {}", groupname))?;
    Ok(group.gid.as_raw())
}

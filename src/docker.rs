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

    for dir in &config.docker.context_dirs {
        let expanded = expand_path(&PathBuf::from(dir))
            .wrap_err(format!("Failed to expand context dir: {}", dir))?;

        if !expanded.exists() {
            return Err(eyre::eyre!("Context directory does not exist: {}", dir));
        }

        let expanded_str = pb_to_str(&expanded);
        binds.push(mount_path_ro(&expanded_str));
    }

    let uid = get_uid(&config.agent.user)?;
    let gid = get_gid(&config.agent.group)?;

    let entrypoint = entrypoint_override
        .map(|s| vec![s.to_string()])
        .or_else(|| config.docker.entrypoint.clone());

    eprintln!("DEBUG: Creating container with:");
    eprintln!("  Image: {}", config.docker.image);
    eprintln!("  Entrypoint: {:?}", entrypoint);
    eprintln!("  User: {}:{}", uid, gid);
    eprintln!("  Working dir: {}", workspace_path_str);
    eprintln!("  Binds:");
    for bind in &binds {
        eprintln!("    {}", bind);
    }

    let container_config = ContainerCreateBody {
        image: Some(config.docker.image.clone()),
        entrypoint: entrypoint.clone(),
        tty: Some(true),
        attach_stdin: Some(true),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        open_stdin: Some(true),
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

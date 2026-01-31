use eyre::{Context, Result};

use super::docker::ContainerBackend;
use super::{ContainerConfig, print_command};

/// Podman container runtime implementation
pub struct PodmanRuntime;

impl PodmanRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl ContainerBackend for PodmanRuntime {
    fn spawn_container(&self, config: &ContainerConfig) -> Result<()> {
        eprintln!("DEBUG: Creating container with Podman:");
        eprintln!("  Image: {}", config.image);
        eprintln!("  Entrypoint: {:?}", config.entrypoint);
        eprintln!("  Command: {:?}", config.command);
        eprintln!("  User: {}", config.user);
        eprintln!("  Working dir: {}", config.working_dir);
        eprintln!("  Mounts: {} volumes", config.mounts.len());
        eprintln!("  Env vars: {} variables", config.env.len());

        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "-it".to_string(),
            "--userns".to_string(),
            "keep-id".to_string(),
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

        // Add command arguments (passed to entrypoint)
        if let Some(command) = &config.command {
            args.extend(command.clone());
        }

        print_command("podman", &args);

        // Execute podman run with inherited stdio
        let status = std::process::Command::new("podman")
            .args(&args)
            .status()
            .wrap_err("Failed to execute podman command")?;

        if !status.success() {
            return Err(eyre::eyre!(
                "Podman container exited with status: {}",
                status
            ));
        }

        Ok(())
    }
}

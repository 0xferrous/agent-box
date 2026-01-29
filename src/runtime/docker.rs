use eyre::{Context, Result};

use super::ContainerConfig;

/// Docker container runtime implementation
pub struct DockerRuntime;

impl DockerRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl ContainerBackend for DockerRuntime {
    fn spawn_container(&self, config: &ContainerConfig) -> Result<()> {
        eprintln!("DEBUG: Creating container with Docker:");
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
}

/// Internal trait for runtime implementations
pub(super) trait ContainerBackend: Send + Sync {
    fn spawn_container(&self, config: &ContainerConfig) -> Result<()>;
}

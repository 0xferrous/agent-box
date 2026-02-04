use eyre::{Context, Result};

use super::{ContainerConfig, print_command};

/// Docker container runtime implementation
pub struct DockerRuntime;

impl DockerRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl ContainerBackend for DockerRuntime {
    fn path_exists_in_image(&self, image: &str, path: &str) -> Result<bool> {
        use std::process::Stdio;

        // Create container without starting it
        let create_output = std::process::Command::new("docker")
            .args(["create", image])
            .output()
            .wrap_err("Failed to create container")?;

        if !create_output.status.success() {
            let stderr = String::from_utf8_lossy(&create_output.stderr);
            return Err(eyre::eyre!("Failed to create container: {}", stderr));
        }

        let container_id = String::from_utf8_lossy(&create_output.stdout)
            .trim()
            .to_string();

        // Export and search for the specific path
        let export_child = std::process::Command::new("docker")
            .args(["export", &container_id])
            .stdout(Stdio::piped())
            .spawn()
            .wrap_err("Failed to spawn docker export")?;

        // Use tar verbose to check if path exists as a directory
        let tar_child = std::process::Command::new("tar")
            .args(["-tv"])
            .stdin(export_child.stdout.unwrap())
            .stdout(Stdio::piped())
            .spawn()
            .wrap_err("Failed to spawn tar")?;

        let output = tar_child
            .wait_with_output()
            .wrap_err("Failed to read tar output")?;

        // Cleanup
        let _ = std::process::Command::new("docker")
            .args(["rm", &container_id])
            .output();

        if !output.status.success() {
            return Ok(false);
        }

        let normalized_path = path.trim_end_matches('/').trim_start_matches('/');
        let stdout = String::from_utf8_lossy(&output.stdout);

        let exists = stdout
            .lines()
            .filter(|line| line.starts_with('d')) // Only directories
            .any(|line| {
                if let Some(entry_path) = line.split_whitespace().last() {
                    let entry_normalized = entry_path.trim_end_matches('/').trim_start_matches('/');
                    entry_normalized == normalized_path
                } else {
                    false
                }
            });

        Ok(exists)
    }

    fn list_paths_in_image(&self, image: &str, root_path: Option<&str>) -> Result<Vec<String>> {
        use std::process::Stdio;

        // Create a container without starting it
        let create_output = std::process::Command::new("docker")
            .args(["create", image])
            .output()
            .wrap_err("Failed to create container")?;

        if !create_output.status.success() {
            let stderr = String::from_utf8_lossy(&create_output.stderr);
            return Err(eyre::eyre!("Failed to create container: {}", stderr));
        }

        let container_id = String::from_utf8_lossy(&create_output.stdout)
            .trim()
            .to_string();

        // Export the container filesystem and list contents with tar
        let export_child = std::process::Command::new("docker")
            .args(["export", &container_id])
            .stdout(Stdio::piped())
            .spawn()
            .wrap_err("Failed to spawn docker export")?;

        let tar_child = std::process::Command::new("tar")
            .args(["-tv"]) // Verbose mode shows file types
            .stdin(export_child.stdout.unwrap())
            .stdout(Stdio::piped())
            .spawn()
            .wrap_err("Failed to spawn tar")?;

        let output = tar_child
            .wait_with_output()
            .wrap_err("Failed to read tar output")?;

        // Cleanup the container
        let _ = std::process::Command::new("docker")
            .args(["rm", &container_id])
            .output();

        if !output.status.success() {
            return Err(eyre::eyre!("Failed to list tar contents"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let all_paths: Vec<String> = stdout
            .lines()
            // Filter for directories only (line starts with 'd' in permissions)
            .filter(|line| line.starts_with('d'))
            .filter_map(|line| {
                // Parse tar verbose output: "drwxr-xr-x 0/0  0 2024-01-01 00:00 path/to/dir/"
                // The path is the last field
                line.split_whitespace().last().map(|s| {
                    let trimmed = s.trim_end_matches('/');
                    if trimmed.is_empty() || trimmed == "." {
                        "/".to_string()
                    } else if trimmed.starts_with('/') {
                        trimmed.to_string()
                    } else {
                        format!("/{}", trimmed)
                    }
                })
            })
            .collect();

        // Filter by root_path if specified
        let filtered_paths: Vec<String> = if let Some(root) = root_path {
            let root_normalized = root.trim_end_matches('/');
            all_paths
                .into_iter()
                .filter(|p| {
                    if root_normalized.is_empty() || root_normalized == "/" {
                        true
                    } else {
                        p == root_normalized || p.starts_with(&format!("{}/", root_normalized))
                    }
                })
                .collect()
        } else {
            all_paths
        };

        Ok(filtered_paths)
    }

    fn spawn_container(&self, config: &ContainerConfig) -> Result<()> {
        eprintln!("DEBUG: Creating container with Docker:");
        eprintln!("  Image: {}", config.image);
        eprintln!("  Entrypoint: {:?}", config.entrypoint);
        eprintln!("  Command: {:?}", config.command);
        eprintln!("  User: {}", config.user);
        eprintln!("  Working dir: {}", config.working_dir);
        eprintln!("  Mounts: {} volumes", config.mounts.len());
        eprintln!("  Env vars: {} variables", config.env.len());
        eprintln!("  Ports: {} mappings", config.ports.len());
        eprintln!("  Hosts: {} entries", config.hosts.len());
        eprintln!("  Network: {:?}", config.network);

        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "-it".to_string(),
            "--user".to_string(),
            config.user.clone(),
            "--workdir".to_string(),
            config.working_dir.clone(),
        ];

        // Add network mode if specified
        if let Some(ref network) = config.network {
            args.push("--network".to_string());
            args.push(network.clone());
        }

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

        // Add port mappings
        for port in &config.ports {
            args.push("-p".to_string());
            args.push(port.clone());
        }

        // Add custom host entries
        for host in &config.hosts {
            args.push("--add-host".to_string());
            args.push(host.clone());
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

        print_command("docker", &args);

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

    /// Check if a path exists in the container image
    fn path_exists_in_image(&self, image: &str, path: &str) -> Result<bool>;

    /// List all paths in the container image
    fn list_paths_in_image(&self, image: &str, root_path: Option<&str>) -> Result<Vec<String>>;
}

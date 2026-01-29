use eyre::Result;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

use crate::path::expand_path;

#[derive(Debug, Deserialize, Default)]
pub struct MountPaths {
    #[serde(default)]
    pub absolute: Vec<String>,
    #[serde(default)]
    pub home_relative: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct MountsConfig {
    #[serde(default)]
    pub ro: MountPaths,
    #[serde(default)]
    pub rw: MountPaths,
    #[serde(default)]
    pub o: MountPaths,
}

#[derive(Debug, Deserialize, Default)]
pub struct RuntimeConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    pub image: String,
    #[serde(default)]
    pub entrypoint: Option<Vec<String>>,
    #[serde(default)]
    pub mounts: MountsConfig,
    #[serde(default)]
    pub env: Vec<String>,
}

fn default_backend() -> String {
    "docker".to_string()
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub workspace_dir: PathBuf,
    pub base_repo_dir: PathBuf,
    pub runtime: RuntimeConfig,
}

/// Load configuration from ~/.agent-box.toml
pub fn load_config() -> Result<Config> {
    use eyre::Context;

    let home = std::env::var("HOME").wrap_err("Failed to get HOME environment variable")?;
    let config_path = PathBuf::from(home).join(".agent-box.toml");

    let content = fs::read_to_string(&config_path)
        .wrap_err_with(|| format!("Failed to read config file at {}", config_path.display()))?;

    let mut config: Config =
        toml::from_str(&content).wrap_err("Failed to parse TOML configuration")?;

    // Expand all paths
    config.workspace_dir =
        expand_path(&config.workspace_dir).wrap_err("Failed to expand workspace_dir path")?;
    config.base_repo_dir =
        expand_path(&config.base_repo_dir).wrap_err("Failed to expand base_repo_dir path")?;

    Ok(config)
}

/// Load configuration or exit on error
pub fn load_config_or_exit() -> Config {
    match load_config() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            std::process::exit(1);
        }
    }
}

use eyre::{Result, WrapErr};
use figment::{
    Figment,
    providers::{Format, Toml},
};
use serde::{Deserialize, Deserializer};
use std::path::PathBuf;

use crate::path::expand_path;
use crate::repo::find_git_root;

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct MountPaths {
    #[serde(default)]
    pub absolute: Vec<String>,
    #[serde(default)]
    pub home_relative: Vec<String>,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct MountsConfig {
    #[serde(default)]
    pub ro: MountPaths,
    #[serde(default)]
    pub rw: MountPaths,
    #[serde(default)]
    pub o: MountPaths,
}

/// Deserialize entrypoint from a shell-style string into Vec<String>
fn deserialize_entrypoint<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    opt.map(|s| shell_words::split(&s).map_err(serde::de::Error::custom))
        .transpose()
}

fn default_backend() -> String {
    "docker".to_string()
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct RuntimeConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default)]
    pub image: String,
    #[serde(default, deserialize_with = "deserialize_entrypoint")]
    pub entrypoint: Option<Vec<String>>,
    #[serde(default)]
    pub mounts: MountsConfig,
    #[serde(default)]
    pub env: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Config {
    pub workspace_dir: PathBuf,
    pub base_repo_dir: PathBuf,
    #[serde(default)]
    pub runtime: RuntimeConfig,
}

/// Build a Figment from global and optional repo-local config paths.
/// Uses admerge: arrays concatenate, scalars override, dicts union recursively.
fn build_figment(global_config_path: &PathBuf, repo_config_path: Option<&PathBuf>) -> Figment {
    let mut figment = Figment::from(Toml::file(global_config_path));

    if let Some(repo_path) = repo_config_path {
        figment = figment.admerge(Toml::file(repo_path));
    }

    figment
}

/// Load configuration with layered merging:
/// 1. Load ~/.agent-box.toml (global config, required)
/// 2. Load <git_root>/.agent-box.toml (repo config, optional)
/// 3. Merge using admerge: arrays are concatenated, scalars are overridden
pub fn load_config() -> Result<Config> {
    let home = std::env::var("HOME").wrap_err("Failed to get HOME environment variable")?;
    let global_config_path = PathBuf::from(&home).join(".agent-box.toml");

    // Find repo-local config if present (silently ignore if not in a git repo)
    let repo_config_path = find_git_root()
        .ok()
        .map(|root| root.join(".agent-box.toml"));

    let figment = build_figment(&global_config_path, repo_config_path.as_ref());

    let mut config: Config = figment.extract().map_err(|e| {
        // Convert figment::Error to eyre::Report with nice formatting
        eyre::eyre!("{}", e)
    })?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use figment::Jail;

    #[test]
    fn test_global_config_only() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [runtime]
                backend = "docker"
                image = "test:latest"
                env = ["FOO=bar"]

                [runtime.mounts.ro]
                absolute = ["/nix/store"]
                home_relative = ["~/.config/git"]
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let figment = build_figment(&global_path, None);
            let config: Config = figment.extract()?;

            assert_eq!(config.workspace_dir, PathBuf::from("/workspaces"));
            assert_eq!(config.base_repo_dir, PathBuf::from("/repos"));
            assert_eq!(config.runtime.backend, "docker");
            assert_eq!(config.runtime.image, "test:latest");
            assert_eq!(config.runtime.env, vec!["FOO=bar"]);
            assert_eq!(config.runtime.mounts.ro.absolute, vec!["/nix/store"]);
            assert_eq!(
                config.runtime.mounts.ro.home_relative,
                vec!["~/.config/git"]
            );

            Ok(())
        });
    }

    #[test]
    fn test_repo_config_overrides_scalars() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [runtime]
                backend = "docker"
                image = "global:latest"
                "#,
            )?;

            jail.create_file(
                "repo.toml",
                r#"
                [runtime]
                image = "repo:latest"
                backend = "podman"
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let repo_path = jail.directory().join("repo.toml");
            let figment = build_figment(&global_path, Some(&repo_path));
            let config: Config = figment.extract()?;

            // Scalars should be overridden by repo config
            assert_eq!(config.runtime.image, "repo:latest");
            assert_eq!(config.runtime.backend, "podman");

            // Top-level values should remain from global
            assert_eq!(config.workspace_dir, PathBuf::from("/workspaces"));
            assert_eq!(config.base_repo_dir, PathBuf::from("/repos"));

            Ok(())
        });
    }

    #[test]
    fn test_repo_config_concatenates_arrays() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [runtime]
                image = "test:latest"
                env = ["GLOBAL=1", "SHARED=global"]

                [runtime.mounts.ro]
                absolute = ["/nix/store"]
                home_relative = ["~/.config/git"]

                [runtime.mounts.rw]
                absolute = ["/tmp"]
                "#,
            )?;

            jail.create_file(
                "repo.toml",
                r#"
                [runtime]
                env = ["REPO=2", "EXTRA=value"]

                [runtime.mounts.ro]
                absolute = ["/opt/tools"]
                home_relative = ["~/.ssh"]

                [runtime.mounts.rw]
                home_relative = ["~/.local/share"]
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let repo_path = jail.directory().join("repo.toml");
            let figment = build_figment(&global_path, Some(&repo_path));
            let config: Config = figment.extract()?;

            // Arrays should be concatenated (global first, then repo)
            assert_eq!(
                config.runtime.env,
                vec!["GLOBAL=1", "SHARED=global", "REPO=2", "EXTRA=value"]
            );

            // Nested arrays should also be concatenated
            assert_eq!(
                config.runtime.mounts.ro.absolute,
                vec!["/nix/store", "/opt/tools"]
            );
            assert_eq!(
                config.runtime.mounts.ro.home_relative,
                vec!["~/.config/git", "~/.ssh"]
            );

            // rw mounts should union the dicts and concatenate arrays
            assert_eq!(config.runtime.mounts.rw.absolute, vec!["/tmp"]);
            assert_eq!(
                config.runtime.mounts.rw.home_relative,
                vec!["~/.local/share"]
            );

            Ok(())
        });
    }

    #[test]
    fn test_repo_config_can_override_top_level() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/global/workspaces"
                base_repo_dir = "/global/repos"

                [runtime]
                image = "test:latest"
                "#,
            )?;

            jail.create_file(
                "repo.toml",
                r#"
                workspace_dir = "/repo/workspaces"
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let repo_path = jail.directory().join("repo.toml");
            let figment = build_figment(&global_path, Some(&repo_path));
            let config: Config = figment.extract()?;

            // workspace_dir should be overridden
            assert_eq!(config.workspace_dir, PathBuf::from("/repo/workspaces"));
            // base_repo_dir should remain from global
            assert_eq!(config.base_repo_dir, PathBuf::from("/global/repos"));

            Ok(())
        });
    }

    #[test]
    fn test_entrypoint_replaces_not_concatenates() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [runtime]
                image = "test:latest"
                entrypoint = "/bin/bash -c"
                "#,
            )?;

            jail.create_file(
                "repo.toml",
                r#"
                [runtime]
                entrypoint = "/bin/zsh"
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let repo_path = jail.directory().join("repo.toml");
            let figment = build_figment(&global_path, Some(&repo_path));
            let config: Config = figment.extract()?;

            // entrypoint is a string, so repo overrides global (no concatenation)
            assert_eq!(
                config.runtime.entrypoint,
                Some(vec!["/bin/zsh".to_string()])
            );

            Ok(())
        });
    }

    #[test]
    fn test_entrypoint_global_only() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [runtime]
                image = "test:latest"
                entrypoint = "/bin/bash -c"
                "#,
            )?;

            jail.create_file(
                "repo.toml",
                r#"
                [runtime]
                image = "repo:latest"
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let repo_path = jail.directory().join("repo.toml");
            let figment = build_figment(&global_path, Some(&repo_path));
            let config: Config = figment.extract()?;

            // If repo doesn't set entrypoint, global's value is used
            assert_eq!(
                config.runtime.entrypoint,
                Some(vec!["/bin/bash".to_string(), "-c".to_string()])
            );

            Ok(())
        });
    }

    #[test]
    fn test_entrypoint_repo_only() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [runtime]
                image = "test:latest"
                "#,
            )?;

            jail.create_file(
                "repo.toml",
                r#"
                [runtime]
                entrypoint = "/bin/zsh -l"
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let repo_path = jail.directory().join("repo.toml");
            let figment = build_figment(&global_path, Some(&repo_path));
            let config: Config = figment.extract()?;

            // If global doesn't set entrypoint, repo config's value is used directly
            assert_eq!(
                config.runtime.entrypoint,
                Some(vec!["/bin/zsh".to_string(), "-l".to_string()])
            );

            Ok(())
        });
    }

    #[test]
    fn test_entrypoint_with_quoted_args() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [runtime]
                image = "test:latest"
                entrypoint = "git commit -m 'some message with spaces'"
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let figment = build_figment(&global_path, None);
            let config: Config = figment.extract()?;

            // Shell-words parsing should handle quoted arguments
            assert_eq!(
                config.runtime.entrypoint,
                Some(vec![
                    "git".to_string(),
                    "commit".to_string(),
                    "-m".to_string(),
                    "some message with spaces".to_string()
                ])
            );

            Ok(())
        });
    }

    #[test]
    fn test_entrypoint_with_double_quotes() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [runtime]
                image = "test:latest"
                entrypoint = 'echo "hello world"'
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let figment = build_figment(&global_path, None);
            let config: Config = figment.extract()?;

            assert_eq!(
                config.runtime.entrypoint,
                Some(vec!["echo".to_string(), "hello world".to_string()])
            );

            Ok(())
        });
    }

    #[test]
    fn test_missing_repo_config_is_ok() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [runtime]
                image = "test:latest"
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let repo_path = jail.directory().join("nonexistent.toml");
            let figment = build_figment(&global_path, Some(&repo_path));
            let config: Config = figment.extract()?;

            // Should work fine with just global config
            assert_eq!(config.workspace_dir, PathBuf::from("/workspaces"));
            assert_eq!(config.runtime.image, "test:latest");

            Ok(())
        });
    }

    #[test]
    fn test_default_backend() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [runtime]
                image = "test:latest"
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let figment = build_figment(&global_path, None);
            let config: Config = figment.extract()?;

            // Backend should default to "docker"
            assert_eq!(config.runtime.backend, "docker");

            Ok(())
        });
    }

    #[test]
    fn test_empty_arrays_by_default() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [runtime]
                image = "test:latest"
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let figment = build_figment(&global_path, None);
            let config: Config = figment.extract()?;

            // Arrays should default to empty
            assert!(config.runtime.env.is_empty());
            assert!(config.runtime.mounts.ro.absolute.is_empty());
            assert!(config.runtime.mounts.ro.home_relative.is_empty());
            assert!(config.runtime.mounts.rw.absolute.is_empty());
            assert!(config.runtime.mounts.rw.home_relative.is_empty());
            assert!(config.runtime.mounts.o.absolute.is_empty());
            assert!(config.runtime.mounts.o.home_relative.is_empty());

            Ok(())
        });
    }

    #[test]
    fn test_deeply_nested_merge() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [runtime]
                image = "test:latest"

                [runtime.mounts.ro]
                absolute = ["/a"]

                [runtime.mounts.rw]
                absolute = ["/b"]

                [runtime.mounts.o]
                absolute = ["/c"]
                "#,
            )?;

            jail.create_file(
                "repo.toml",
                r#"
                [runtime.mounts.ro]
                absolute = ["/d"]

                [runtime.mounts.rw]
                home_relative = ["~/e"]

                [runtime.mounts.o]
                absolute = ["/f"]
                home_relative = ["~/g"]
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let repo_path = jail.directory().join("repo.toml");
            let figment = build_figment(&global_path, Some(&repo_path));
            let config: Config = figment.extract()?;

            // All nested arrays should be properly merged
            assert_eq!(config.runtime.mounts.ro.absolute, vec!["/a", "/d"]);
            assert!(config.runtime.mounts.ro.home_relative.is_empty());

            assert_eq!(config.runtime.mounts.rw.absolute, vec!["/b"]);
            assert_eq!(config.runtime.mounts.rw.home_relative, vec!["~/e"]);

            assert_eq!(config.runtime.mounts.o.absolute, vec!["/c", "/f"]);
            assert_eq!(config.runtime.mounts.o.home_relative, vec!["~/g"]);

            Ok(())
        });
    }
}

use eyre::{Result, WrapErr};
use figment::{
    Figment,
    providers::{Format, Toml},
};
use serde::{Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};
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

impl MountPaths {
    /// Merge another MountPaths into this one (concatenate arrays)
    pub fn merge(&mut self, other: &MountPaths) {
        self.absolute.extend(other.absolute.iter().cloned());
        self.home_relative
            .extend(other.home_relative.iter().cloned());
    }
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

impl MountsConfig {
    /// Merge another MountsConfig into this one (concatenate arrays)
    pub fn merge(&mut self, other: &MountsConfig) {
        self.ro.merge(&other.ro);
        self.rw.merge(&other.rw);
        self.o.merge(&other.o);
    }
}

/// A profile defines a named set of mounts and environment variables.
/// Profiles can extend other profiles via the `extends` field.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct ProfileConfig {
    /// List of profile names this profile extends (inherits from)
    #[serde(default)]
    pub extends: Vec<String>,
    /// Mounts defined by this profile
    #[serde(default)]
    pub mounts: MountsConfig,
    /// Environment variables defined by this profile
    #[serde(default)]
    pub env: Vec<String>,
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
    /// Default profile name to always apply (if set)
    #[serde(default)]
    pub default_profile: Option<String>,
    /// Named profiles that can be selected via CLI
    #[serde(default)]
    pub profiles: HashMap<String, ProfileConfig>,
    #[serde(default)]
    pub runtime: RuntimeConfig,
}

/// Resolved mounts and env from profile resolution
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ResolvedProfile {
    pub mounts: MountsConfig,
    pub env: Vec<String>,
}

impl ResolvedProfile {
    /// Merge another resolved profile into this one
    pub fn merge(&mut self, other: &ResolvedProfile) {
        self.mounts.merge(&other.mounts);
        self.env.extend(other.env.iter().cloned());
    }
}

/// Resolve profiles with inheritance, returning merged mounts and env.
///
/// Resolution order:
/// 1. Start with runtime.mounts and runtime.env as base
/// 2. Apply default_profile if set
/// 3. Apply each profile from `profile_names` in order
///
/// Each profile's `extends` chain is resolved depth-first before the profile itself.
pub fn resolve_profiles(config: &Config, profile_names: &[String]) -> Result<ResolvedProfile> {
    let mut resolved = ResolvedProfile {
        mounts: config.runtime.mounts.clone(),
        env: config.runtime.env.clone(),
    };

    // Collect all profiles to apply: default + CLI-specified
    let mut profiles_to_apply: Vec<&str> = Vec::new();

    if let Some(ref default) = config.default_profile {
        profiles_to_apply.push(default);
    }

    for name in profile_names {
        profiles_to_apply.push(name);
    }

    // Resolve each profile
    for profile_name in profiles_to_apply {
        let profile_resolved = resolve_single_profile(config, profile_name, &mut HashSet::new())?;
        resolved.merge(&profile_resolved);
    }

    Ok(resolved)
}

/// Resolve a single profile with its extends chain.
/// Uses `visited` to detect cycles.
fn resolve_single_profile(
    config: &Config,
    profile_name: &str,
    visited: &mut HashSet<String>,
) -> Result<ResolvedProfile> {
    // Check for cycles
    if visited.contains(profile_name) {
        return Err(eyre::eyre!(
            "Circular profile dependency detected: '{}' was already visited in chain: {:?}",
            profile_name,
            visited
        ));
    }

    // Get the profile
    let profile = config.profiles.get(profile_name).ok_or_else(|| {
        let available: Vec<_> = config.profiles.keys().collect();
        eyre::eyre!(
            "Unknown profile '{}'. Available profiles: {:?}",
            profile_name,
            available
        )
    })?;

    visited.insert(profile_name.to_string());

    let mut resolved = ResolvedProfile::default();

    // First resolve all extended profiles (depth-first)
    for parent_name in &profile.extends {
        let parent_resolved = resolve_single_profile(config, parent_name, visited)?;
        resolved.merge(&parent_resolved);
    }

    // Then apply this profile's own mounts and env
    resolved.mounts.merge(&profile.mounts);
    resolved.env.extend(profile.env.iter().cloned());

    // Remove from visited after processing (allow same profile in different branches)
    visited.remove(profile_name);

    Ok(resolved)
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

/// Validation error for profile configuration
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileValidationError {
    pub profile_name: Option<String>,
    pub message: String,
}

impl std::fmt::Display for ProfileValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.profile_name {
            Some(name) => write!(f, "Profile '{}': {}", name, self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

/// Result of config validation
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationResult {
    pub errors: Vec<ProfileValidationError>,
    pub warnings: Vec<ProfileValidationError>,
}

impl ValidationResult {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}

/// Validate the configuration, checking for:
/// - `default_profile` references a defined profile
/// - All `extends` references point to defined profiles
/// - No circular dependencies in `extends` chains
/// - No self-references in `extends`
///
/// Returns a ValidationResult with errors and warnings.
pub fn validate_config(config: &Config) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check default_profile exists if set
    if let Some(ref default) = config.default_profile
        && !config.profiles.contains_key(default)
    {
        let available: Vec<_> = config.profiles.keys().cloned().collect();
        errors.push(ProfileValidationError {
            profile_name: None,
            message: format!(
                "default_profile '{}' is not defined. Available profiles: {:?}",
                default, available
            ),
        });
    }

    // Check each profile
    for (profile_name, profile) in &config.profiles {
        // Check for self-reference
        if profile.extends.contains(profile_name) {
            errors.push(ProfileValidationError {
                profile_name: Some(profile_name.clone()),
                message: "extends itself (self-reference)".to_string(),
            });
        }

        // Check all extends references exist
        for parent_name in &profile.extends {
            if !config.profiles.contains_key(parent_name) {
                let available: Vec<_> = config.profiles.keys().cloned().collect();
                errors.push(ProfileValidationError {
                    profile_name: Some(profile_name.clone()),
                    message: format!(
                        "extends unknown profile '{}'. Available profiles: {:?}",
                        parent_name, available
                    ),
                });
            }
        }

        // Check for circular dependencies (only if no self-reference already detected)
        if !profile.extends.contains(profile_name)
            && let Some(cycle) = detect_cycle(config, profile_name)
        {
            errors.push(ProfileValidationError {
                profile_name: Some(profile_name.clone()),
                message: format!("circular dependency detected: {}", cycle.join(" -> ")),
            });
        }

        // Warn about empty profiles (no mounts, no env, no extends)
        if profile.extends.is_empty()
            && profile.env.is_empty()
            && profile.mounts.ro.absolute.is_empty()
            && profile.mounts.ro.home_relative.is_empty()
            && profile.mounts.rw.absolute.is_empty()
            && profile.mounts.rw.home_relative.is_empty()
            && profile.mounts.o.absolute.is_empty()
            && profile.mounts.o.home_relative.is_empty()
        {
            warnings.push(ProfileValidationError {
                profile_name: Some(profile_name.clone()),
                message: "profile is empty (no mounts, env, or extends)".to_string(),
            });
        }
    }

    ValidationResult { errors, warnings }
}

/// Detect circular dependencies starting from a profile.
/// Returns Some(cycle_path) if a cycle is found, None otherwise.
fn detect_cycle(config: &Config, start: &str) -> Option<Vec<String>> {
    let mut visited = HashSet::new();
    let mut path = Vec::new();
    detect_cycle_recursive(config, start, &mut visited, &mut path)
}

fn detect_cycle_recursive(
    config: &Config,
    current: &str,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> Option<Vec<String>> {
    if visited.contains(current) {
        // Found a cycle - return the path from the cycle start
        path.push(current.to_string());
        return Some(path.clone());
    }

    let profile = config.profiles.get(current)?;

    visited.insert(current.to_string());
    path.push(current.to_string());

    for parent in &profile.extends {
        if let Some(cycle) = detect_cycle_recursive(config, parent, visited, path) {
            return Some(cycle);
        }
    }

    path.pop();
    visited.remove(current);
    None
}

/// Validate config and return errors as a formatted Result
pub fn validate_config_or_err(config: &Config) -> Result<()> {
    let result = validate_config(config);

    if !result.is_ok() {
        let error_messages: Vec<String> = result.errors.iter().map(|e| e.to_string()).collect();
        return Err(eyre::eyre!(
            "Configuration validation failed:\n  - {}",
            error_messages.join("\n  - ")
        ));
    }

    Ok(())
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

    // Profile resolution tests

    fn make_test_config() -> Config {
        Config {
            workspace_dir: PathBuf::from("/workspaces"),
            base_repo_dir: PathBuf::from("/repos"),
            default_profile: None,
            profiles: HashMap::new(),
            runtime: RuntimeConfig {
                backend: "docker".to_string(),
                image: "test:latest".to_string(),
                entrypoint: None,
                mounts: MountsConfig::default(),
                env: vec!["BASE=1".to_string()],
            },
        }
    }

    #[test]
    fn test_resolve_profiles_no_profiles() {
        let config = make_test_config();
        let resolved = resolve_profiles(&config, &[]).unwrap();

        // Should just have runtime.env
        assert_eq!(resolved.env, vec!["BASE=1"]);
        assert!(resolved.mounts.ro.absolute.is_empty());
    }

    #[test]
    fn test_resolve_profiles_single_profile() {
        let mut config = make_test_config();
        config.profiles.insert(
            "git".to_string(),
            ProfileConfig {
                extends: vec![],
                mounts: MountsConfig {
                    ro: MountPaths {
                        absolute: vec![],
                        home_relative: vec!["~/.gitconfig".to_string()],
                    },
                    ..Default::default()
                },
                env: vec!["GIT=1".to_string()],
            },
        );

        let resolved = resolve_profiles(&config, &["git".to_string()]).unwrap();

        assert_eq!(resolved.env, vec!["BASE=1", "GIT=1"]);
        assert_eq!(
            resolved.mounts.ro.home_relative,
            vec!["~/.gitconfig".to_string()]
        );
    }

    #[test]
    fn test_resolve_profiles_with_extends() {
        let mut config = make_test_config();

        // base profile
        config.profiles.insert(
            "base".to_string(),
            ProfileConfig {
                extends: vec![],
                mounts: MountsConfig {
                    ro: MountPaths {
                        absolute: vec!["/nix/store".to_string()],
                        home_relative: vec![],
                    },
                    ..Default::default()
                },
                env: vec!["PROFILE_BASE=1".to_string()],
            },
        );

        // git extends base
        config.profiles.insert(
            "git".to_string(),
            ProfileConfig {
                extends: vec!["base".to_string()],
                mounts: MountsConfig {
                    ro: MountPaths {
                        absolute: vec![],
                        home_relative: vec!["~/.gitconfig".to_string()],
                    },
                    ..Default::default()
                },
                env: vec!["GIT=1".to_string()],
            },
        );

        let resolved = resolve_profiles(&config, &["git".to_string()]).unwrap();

        // Should have: runtime.env + base env + git env
        assert_eq!(resolved.env, vec!["BASE=1", "PROFILE_BASE=1", "GIT=1"]);
        // Mounts from base and git
        assert_eq!(resolved.mounts.ro.absolute, vec!["/nix/store"]);
        assert_eq!(resolved.mounts.ro.home_relative, vec!["~/.gitconfig"]);
    }

    #[test]
    fn test_resolve_profiles_with_default_profile() {
        let mut config = make_test_config();
        config.default_profile = Some("base".to_string());

        config.profiles.insert(
            "base".to_string(),
            ProfileConfig {
                extends: vec![],
                mounts: MountsConfig::default(),
                env: vec!["DEFAULT=1".to_string()],
            },
        );

        config.profiles.insert(
            "extra".to_string(),
            ProfileConfig {
                extends: vec![],
                mounts: MountsConfig::default(),
                env: vec!["EXTRA=1".to_string()],
            },
        );

        // Request extra, but default should also be applied first
        let resolved = resolve_profiles(&config, &["extra".to_string()]).unwrap();

        assert_eq!(resolved.env, vec!["BASE=1", "DEFAULT=1", "EXTRA=1"]);
    }

    #[test]
    fn test_resolve_profiles_multiple_cli_profiles() {
        let mut config = make_test_config();

        config.profiles.insert(
            "git".to_string(),
            ProfileConfig {
                extends: vec![],
                mounts: MountsConfig::default(),
                env: vec!["GIT=1".to_string()],
            },
        );

        config.profiles.insert(
            "rust".to_string(),
            ProfileConfig {
                extends: vec![],
                mounts: MountsConfig::default(),
                env: vec!["RUST=1".to_string()],
            },
        );

        let resolved = resolve_profiles(&config, &["git".to_string(), "rust".to_string()]).unwrap();

        assert_eq!(resolved.env, vec!["BASE=1", "GIT=1", "RUST=1"]);
    }

    #[test]
    fn test_resolve_profiles_diamond_inheritance() {
        let mut config = make_test_config();

        // Diamond: git and jj both extend base, dev extends both
        config.profiles.insert(
            "base".to_string(),
            ProfileConfig {
                extends: vec![],
                mounts: MountsConfig::default(),
                env: vec!["BASE_PROFILE=1".to_string()],
            },
        );

        config.profiles.insert(
            "git".to_string(),
            ProfileConfig {
                extends: vec!["base".to_string()],
                mounts: MountsConfig::default(),
                env: vec!["GIT=1".to_string()],
            },
        );

        config.profiles.insert(
            "jj".to_string(),
            ProfileConfig {
                extends: vec!["base".to_string()],
                mounts: MountsConfig::default(),
                env: vec!["JJ=1".to_string()],
            },
        );

        config.profiles.insert(
            "dev".to_string(),
            ProfileConfig {
                extends: vec!["git".to_string(), "jj".to_string()],
                mounts: MountsConfig::default(),
                env: vec!["DEV=1".to_string()],
            },
        );

        let resolved = resolve_profiles(&config, &["dev".to_string()]).unwrap();

        // base is resolved twice (once via git, once via jj) - this is expected
        // Order: runtime.env, then git chain (base, git), then jj chain (base, jj), then dev
        assert_eq!(
            resolved.env,
            vec![
                "BASE=1",
                "BASE_PROFILE=1",
                "GIT=1",
                "BASE_PROFILE=1",
                "JJ=1",
                "DEV=1"
            ]
        );
    }

    #[test]
    fn test_resolve_profiles_circular_dependency_detected() {
        let mut config = make_test_config();

        // a extends b, b extends a
        config.profiles.insert(
            "a".to_string(),
            ProfileConfig {
                extends: vec!["b".to_string()],
                mounts: MountsConfig::default(),
                env: vec![],
            },
        );

        config.profiles.insert(
            "b".to_string(),
            ProfileConfig {
                extends: vec!["a".to_string()],
                mounts: MountsConfig::default(),
                env: vec![],
            },
        );

        let result = resolve_profiles(&config, &["a".to_string()]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Circular"));
    }

    #[test]
    fn test_resolve_profiles_self_reference_detected() {
        let mut config = make_test_config();

        config.profiles.insert(
            "self".to_string(),
            ProfileConfig {
                extends: vec!["self".to_string()],
                mounts: MountsConfig::default(),
                env: vec![],
            },
        );

        let result = resolve_profiles(&config, &["self".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Circular"));
    }

    #[test]
    fn test_resolve_profiles_unknown_profile_error() {
        let config = make_test_config();

        let result = resolve_profiles(&config, &["nonexistent".to_string()]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown profile"));
        assert!(err.contains("nonexistent"));
    }

    #[test]
    fn test_resolve_profiles_unknown_extends_error() {
        let mut config = make_test_config();

        config.profiles.insert(
            "broken".to_string(),
            ProfileConfig {
                extends: vec!["nonexistent".to_string()],
                mounts: MountsConfig::default(),
                env: vec![],
            },
        );

        let result = resolve_profiles(&config, &["broken".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown profile"));
    }

    #[test]
    fn test_resolve_profiles_mounts_merge_correctly() {
        let mut config = make_test_config();
        config.runtime.mounts.ro.absolute = vec!["/runtime".to_string()];

        config.profiles.insert(
            "base".to_string(),
            ProfileConfig {
                extends: vec![],
                mounts: MountsConfig {
                    ro: MountPaths {
                        absolute: vec!["/base".to_string()],
                        home_relative: vec!["~/.base".to_string()],
                    },
                    rw: MountPaths {
                        absolute: vec![],
                        home_relative: vec!["~/.base-rw".to_string()],
                    },
                    o: MountPaths::default(),
                },
                env: vec![],
            },
        );

        config.profiles.insert(
            "extra".to_string(),
            ProfileConfig {
                extends: vec!["base".to_string()],
                mounts: MountsConfig {
                    ro: MountPaths {
                        absolute: vec!["/extra".to_string()],
                        home_relative: vec![],
                    },
                    rw: MountPaths::default(),
                    o: MountPaths {
                        absolute: vec![],
                        home_relative: vec!["~/.extra-o".to_string()],
                    },
                },
                env: vec![],
            },
        );

        let resolved = resolve_profiles(&config, &["extra".to_string()]).unwrap();

        // ro: runtime + base + extra
        assert_eq!(
            resolved.mounts.ro.absolute,
            vec!["/runtime", "/base", "/extra"]
        );
        assert_eq!(resolved.mounts.ro.home_relative, vec!["~/.base"]);

        // rw: base only
        assert_eq!(resolved.mounts.rw.home_relative, vec!["~/.base-rw"]);

        // o: extra only
        assert_eq!(resolved.mounts.o.home_relative, vec!["~/.extra-o"]);
    }

    #[test]
    fn test_profile_parsing_from_toml() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"
                default_profile = "base"

                [profiles.base]
                env = ["BASE=1"]

                [profiles.base.mounts.ro]
                absolute = ["/nix/store"]

                [profiles.git]
                extends = ["base"]
                env = ["GIT=1"]

                [profiles.git.mounts.ro]
                home_relative = ["~/.gitconfig"]

                [runtime]
                image = "test:latest"
                "#,
            )?;

            let config_path = jail.directory().join("config.toml");
            let figment = build_figment(&config_path, None);
            let config: Config = figment.extract()?;

            assert_eq!(config.default_profile, Some("base".to_string()));
            assert_eq!(config.profiles.len(), 2);

            let base = config.profiles.get("base").unwrap();
            assert!(base.extends.is_empty());
            assert_eq!(base.env, vec!["BASE=1"]);
            assert_eq!(base.mounts.ro.absolute, vec!["/nix/store"]);

            let git = config.profiles.get("git").unwrap();
            assert_eq!(git.extends, vec!["base"]);
            assert_eq!(git.env, vec!["GIT=1"]);
            assert_eq!(git.mounts.ro.home_relative, vec!["~/.gitconfig"]);

            Ok(())
        });
    }

    #[test]
    fn test_layered_profiles_repo_extends_global() {
        // Test that repo-local config can define a profile that extends a global profile
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [profiles.base]
                env = ["BASE=1"]

                [profiles.base.mounts.ro]
                absolute = ["/nix/store"]

                [profiles.git]
                extends = ["base"]
                env = ["GIT=1"]

                [runtime]
                image = "test:latest"
                "#,
            )?;

            jail.create_file(
                "repo.toml",
                r#"
                # Repo-local profile that extends global "git" profile
                [profiles.repo-dev]
                extends = ["git"]
                env = ["REPO_DEV=1"]

                [profiles.repo-dev.mounts.rw]
                home_relative = ["~/.local/share/myproject"]
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let repo_path = jail.directory().join("repo.toml");
            let figment = build_figment(&global_path, Some(&repo_path));
            let config: Config = figment.extract()?;

            // Should have all 3 profiles merged
            assert_eq!(config.profiles.len(), 3);
            assert!(config.profiles.contains_key("base"));
            assert!(config.profiles.contains_key("git"));
            assert!(config.profiles.contains_key("repo-dev"));

            // repo-dev should extend git (which extends base)
            let repo_dev = config.profiles.get("repo-dev").unwrap();
            assert_eq!(repo_dev.extends, vec!["git"]);
            assert_eq!(repo_dev.env, vec!["REPO_DEV=1"]);

            // Now resolve the profile chain
            let resolved = resolve_profiles(&config, &["repo-dev".to_string()]).unwrap();

            // Should have: runtime.env (empty) + base + git + repo-dev
            assert_eq!(resolved.env, vec!["BASE=1", "GIT=1", "REPO_DEV=1"]);
            // Mounts from base
            assert_eq!(resolved.mounts.ro.absolute, vec!["/nix/store"]);
            // Mounts from repo-dev
            assert_eq!(
                resolved.mounts.rw.home_relative,
                vec!["~/.local/share/myproject"]
            );

            Ok(())
        });
    }

    #[test]
    fn test_layered_profiles_repo_overrides_default_profile() {
        // Test that repo config can override the default_profile
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"
                default_profile = "base"

                [profiles.base]
                env = ["BASE=1"]

                [profiles.dev]
                extends = ["base"]
                env = ["DEV=1"]

                [runtime]
                image = "test:latest"
                "#,
            )?;

            jail.create_file(
                "repo.toml",
                r#"
                # Override default_profile for this repo
                default_profile = "dev"
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let repo_path = jail.directory().join("repo.toml");
            let figment = build_figment(&global_path, Some(&repo_path));
            let config: Config = figment.extract()?;

            // default_profile should be overridden to "dev"
            assert_eq!(config.default_profile, Some("dev".to_string()));

            // Resolve with no extra profiles - should use default "dev"
            let resolved = resolve_profiles(&config, &[]).unwrap();
            assert_eq!(resolved.env, vec!["BASE=1", "DEV=1"]);

            Ok(())
        });
    }

    #[test]
    fn test_layered_profiles_repo_adds_env_to_global_profile() {
        // Test that repo config can add env vars to a global profile
        Jail::expect_with(|jail| {
            jail.create_file(
                "global.toml",
                r#"
                workspace_dir = "/workspaces"
                base_repo_dir = "/repos"

                [profiles.rust]
                env = ["CARGO_HOME=~/.cargo"]

                [profiles.rust.mounts.ro]
                home_relative = ["~/.cargo/config.toml"]

                [runtime]
                image = "test:latest"
                "#,
            )?;

            jail.create_file(
                "repo.toml",
                r#"
                # Add more env vars and mounts to the global rust profile
                [profiles.rust]
                env = ["RUST_BACKTRACE=1"]

                [profiles.rust.mounts.rw]
                home_relative = ["~/.cargo/registry"]
                "#,
            )?;

            let global_path = jail.directory().join("global.toml");
            let repo_path = jail.directory().join("repo.toml");
            let figment = build_figment(&global_path, Some(&repo_path));
            let config: Config = figment.extract()?;

            // Profile should have merged env and mounts
            let rust = config.profiles.get("rust").unwrap();
            assert_eq!(rust.env, vec!["CARGO_HOME=~/.cargo", "RUST_BACKTRACE=1"]);
            assert_eq!(rust.mounts.ro.home_relative, vec!["~/.cargo/config.toml"]);
            assert_eq!(rust.mounts.rw.home_relative, vec!["~/.cargo/registry"]);

            Ok(())
        });
    }

    // Validation tests

    #[test]
    fn test_validate_config_valid() {
        let mut config = make_test_config();
        config.default_profile = Some("base".to_string());
        config.profiles.insert(
            "base".to_string(),
            ProfileConfig {
                extends: vec![],
                mounts: MountsConfig::default(),
                env: vec!["A=1".to_string()],
            },
        );

        let result = validate_config(&config);
        assert!(result.is_ok());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validate_config_invalid_default_profile() {
        let mut config = make_test_config();
        config.default_profile = Some("nonexistent".to_string());

        let result = validate_config(&config);
        assert!(!result.is_ok());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].message.contains("default_profile"));
        assert!(result.errors[0].message.contains("nonexistent"));
    }

    #[test]
    fn test_validate_config_invalid_extends() {
        let mut config = make_test_config();
        config.profiles.insert(
            "broken".to_string(),
            ProfileConfig {
                extends: vec!["nonexistent".to_string()],
                mounts: MountsConfig::default(),
                env: vec![],
            },
        );

        let result = validate_config(&config);
        assert!(!result.is_ok());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].message.contains("nonexistent"));
        assert_eq!(result.errors[0].profile_name, Some("broken".to_string()));
    }

    #[test]
    fn test_validate_config_self_reference() {
        let mut config = make_test_config();
        config.profiles.insert(
            "self_ref".to_string(),
            ProfileConfig {
                extends: vec!["self_ref".to_string()],
                mounts: MountsConfig::default(),
                env: vec![],
            },
        );

        let result = validate_config(&config);
        assert!(!result.is_ok());
        // Should have self-reference error
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.message.contains("self-reference"))
        );
    }

    #[test]
    fn test_validate_config_circular_dependency() {
        let mut config = make_test_config();
        config.profiles.insert(
            "a".to_string(),
            ProfileConfig {
                extends: vec!["b".to_string()],
                mounts: MountsConfig::default(),
                env: vec![],
            },
        );
        config.profiles.insert(
            "b".to_string(),
            ProfileConfig {
                extends: vec!["c".to_string()],
                mounts: MountsConfig::default(),
                env: vec![],
            },
        );
        config.profiles.insert(
            "c".to_string(),
            ProfileConfig {
                extends: vec!["a".to_string()],
                mounts: MountsConfig::default(),
                env: vec![],
            },
        );

        let result = validate_config(&config);
        assert!(!result.is_ok());
        // Should detect cycle
        assert!(result.errors.iter().any(|e| e.message.contains("circular")));
    }

    #[test]
    fn test_validate_config_empty_profile_warning() {
        let mut config = make_test_config();
        config.profiles.insert(
            "empty".to_string(),
            ProfileConfig {
                extends: vec![],
                mounts: MountsConfig::default(),
                env: vec![],
            },
        );

        let result = validate_config(&config);
        assert!(result.is_ok()); // warnings don't make it invalid
        assert!(result.has_warnings());
        assert!(result.warnings[0].message.contains("empty"));
    }

    #[test]
    fn test_validate_config_multiple_errors() {
        let mut config = make_test_config();
        config.default_profile = Some("nonexistent".to_string());
        config.profiles.insert(
            "broken1".to_string(),
            ProfileConfig {
                extends: vec!["also_nonexistent".to_string()],
                mounts: MountsConfig::default(),
                env: vec![],
            },
        );
        config.profiles.insert(
            "broken2".to_string(),
            ProfileConfig {
                extends: vec!["broken2".to_string()], // self-reference
                mounts: MountsConfig::default(),
                env: vec![],
            },
        );

        let result = validate_config(&config);
        assert!(!result.is_ok());
        // Should have multiple errors: default_profile, extends unknown, self-reference
        assert!(result.errors.len() >= 3);
    }

    #[test]
    fn test_validate_config_or_err_success() {
        let mut config = make_test_config();
        config.profiles.insert(
            "valid".to_string(),
            ProfileConfig {
                extends: vec![],
                mounts: MountsConfig::default(),
                env: vec!["A=1".to_string()],
            },
        );

        let result = validate_config_or_err(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_config_or_err_failure() {
        let mut config = make_test_config();
        config.default_profile = Some("nonexistent".to_string());

        let result = validate_config_or_err(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("validation failed"));
        assert!(err.contains("nonexistent"));
    }

    #[test]
    fn test_validate_config_no_profiles_is_valid() {
        let config = make_test_config();

        let result = validate_config(&config);
        assert!(result.is_ok());
        assert!(!result.has_warnings());
    }

    #[test]
    fn test_validate_config_deep_valid_chain() {
        let mut config = make_test_config();
        config.profiles.insert(
            "a".to_string(),
            ProfileConfig {
                extends: vec![],
                mounts: MountsConfig::default(),
                env: vec!["A=1".to_string()],
            },
        );
        config.profiles.insert(
            "b".to_string(),
            ProfileConfig {
                extends: vec!["a".to_string()],
                mounts: MountsConfig::default(),
                env: vec!["B=1".to_string()],
            },
        );
        config.profiles.insert(
            "c".to_string(),
            ProfileConfig {
                extends: vec!["b".to_string()],
                mounts: MountsConfig::default(),
                env: vec!["C=1".to_string()],
            },
        );
        config.profiles.insert(
            "d".to_string(),
            ProfileConfig {
                extends: vec!["c".to_string()],
                mounts: MountsConfig::default(),
                env: vec!["D=1".to_string()],
            },
        );
        config.default_profile = Some("d".to_string());

        let result = validate_config(&config);
        assert!(result.is_ok());
    }
}

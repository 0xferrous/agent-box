use eyre::{Result, eyre};
use std::path::{Path, PathBuf};

/// Expand path with ~ support and canonicalize
pub fn expand_path(path: &Path) -> Result<PathBuf> {
    use eyre::Context;

    let expanded = if path.starts_with("~") {
        let home = std::env::var("HOME")
            .wrap_err("Failed to get HOME environment variable when expanding ~")?;
        PathBuf::from(home).join(path.strip_prefix("~")?)
    } else {
        path.to_owned()
    };

    // Canonicalize to get absolute path and resolve symlinks
    expanded.canonicalize()
        .wrap_err_with(|| format!("Failed to canonicalize path: {}", expanded.display()))
}

/// Calculate bare repo path from base_repo_dir and git_dir
pub fn calculate_bare_repo_path(
    base_repo_dir: &Path,
    current_repo_path: &Path,
    git_dir: &Path,
) -> Result<PathBuf> {
    let relative_path = current_repo_path.strip_prefix(base_repo_dir).map_err(|_| {
        eyre!(
            "Repository {} is not under base_repo_dir {}",
            current_repo_path.display(),
            base_repo_dir.display()
        )
    })?;

    let target_path = git_dir.join(relative_path);
    Ok(target_path)
}

/// Convert Path to str with a descriptive error message
pub fn path_to_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| eyre!("Path contains invalid UTF-8: {}", path.display()))
}

/// Calculate relative path from base directory to full path
pub fn calculate_relative_path(base_dir: &Path, full_path: &Path) -> Result<PathBuf> {
    full_path.strip_prefix(base_dir)
        .map(|p| p.to_path_buf())
        .map_err(|_| eyre!(
            "Path {} is not under base directory {}",
            full_path.display(),
            base_dir.display()
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_bare_repo_path_simple() {
        let base = PathBuf::from("/home/user/repos");
        let current = PathBuf::from("/home/user/repos/myproject");
        let git_dir = PathBuf::from("/mnt/git-storage");

        let result = calculate_bare_repo_path(&base, &current, &git_dir).unwrap();
        assert_eq!(result, PathBuf::from("/mnt/git-storage/myproject"));
    }

    #[test]
    fn test_calculate_bare_repo_path_nested() {
        let base = PathBuf::from("/home/user/repos");
        let current = PathBuf::from("/home/user/repos/work/project/subdir");
        let git_dir = PathBuf::from("/mnt/git-storage");

        let result = calculate_bare_repo_path(&base, &current, &git_dir).unwrap();
        assert_eq!(
            result,
            PathBuf::from("/mnt/git-storage/work/project/subdir")
        );
    }

    #[test]
    fn test_calculate_bare_repo_path_not_under_base() {
        let base = PathBuf::from("/home/user/repos");
        let current = PathBuf::from("/somewhere/else/project");
        let git_dir = PathBuf::from("/mnt/git-storage");

        let result = calculate_bare_repo_path(&base, &current, &git_dir);
        assert!(result.is_err());
    }
}

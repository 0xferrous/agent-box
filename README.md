# Agent Box

A Git repository management tool for organizing repositories with bare repos and worktrees, with optional Jujutsu integration.

## Installation

```bash
cargo install --path .
```

## Configuration

Create `~/.agent-box.toml`:

```toml
git_dir = "~/git-repos"           # Where bare repos are stored
jj_dir = "~/jj-workspaces"        # Where jj workspaces are stored
workspace_dir = "~/workspaces"    # (currently unused)
base_repo_dir = "~/repos"         # Base directory for your repos

[agent]
user = "your-user"
group = "your-group"
```

All paths support `~` expansion and will be canonicalized.

## Usage

### Export Repository (All-in-One)

Export the current repository to a bare repo, convert it to a worktree, and initialize a jj workspace:

```bash
ab export
```

This is the main command that sets up everything in one step.

Skip the conversion and jj initialization:

```bash
ab export --no-convert
```

### Initialize Jujutsu Workspace

Create a Jujutsu workspace backed by the bare repository:

```bash
ab init-jj
```

### Convert to Worktree

Convert an existing repository to a worktree of its bare repo:

```bash
ab convert-to-worktree
```

### Show Repository Information

Display configuration, current repository status, and list all workspaces:

```bash
ab info
```

This shows:
- Configuration paths (git_dir, jj_dir, etc.)
- Current repository's bare repo location
- All git worktrees for the current repository
- Current repository's jj workspace status
- All jj workspaces found
- All bare repositories

## How It Works

- **Export**:
  1. Clones your repository to a bare repo at `git_dir`, preserving the relative path structure from `base_repo_dir`
  2. Transforms the current repo into a worktree of the bare repo
  3. Creates a Jujutsu workspace at `jj_dir` that uses the bare Git repo as its backing store
- **Convert**: Standalone command to transform a repo into a worktree of the bare repo
- **Init JJ**: Standalone command to create a Jujutsu workspace using the bare Git repo

All operations set `umask 0002` and configure `setgid` bits for proper group permissions.

## Requirements

- Rust (2024 edition)
- Git
- Jujutsu (for `init-jj` command)

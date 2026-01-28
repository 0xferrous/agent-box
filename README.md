# Agent Box

A Git/Jujutsu workspace management tool with Docker container integration.

## Installation

```bash
cargo install --path .
```

## Configuration

Create `~/.agent-box.toml`:

```toml
workspace_dir = "~/workspaces"    # Where git worktrees and jj workspaces are created
base_repo_dir = "~/repos"         # Base directory for your repos (colocated jj/git)

[docker]
image = "agent-box:latest"
entrypoint = ["/bin/bash"]

[docker.mounts.ro]
absolute = ["/nix/store"]
home_relative = ["~/.config/git"]

[docker.mounts.rw]
absolute = []
home_relative = ["~/.local/share"]
```

All paths support `~` expansion and will be canonicalized.

### Sharing Nix Store

To share your host's Nix store with containers:

```toml
[docker]
env = ["NIX_REMOTE=daemon"]

[docker.mounts.ro]
absolute = ["/nix/store"]

[docker.mounts.rw]
absolute = ["/nix/var/nix/daemon-socket/"]
```

This allows containers to use binaries from your host's Nix store via the daemon socket.

## Usage

### Show Repository Information

Display configuration, current repository status, and list all workspaces:

```bash
ab info
```

This shows:
- Configuration paths (workspace_dir, base_repo_dir)
- Current repository status
- All git worktrees for the current repository
- JJ workspace status
- All jj workspaces found

### Create a New Workspace

Create a new jj workspace (default) or git worktree:

```bash
# Create jj workspace (default), prompts for session name
ab new myrepo

# Create jj workspace with session name
ab new myrepo -s feature-x
ab new myrepo --session feature-x

# Create git worktree
ab new myrepo --session feature-x --git

# Create jj workspace explicitly
ab new myrepo --session feature-x --jj

# Use current directory's repo
ab new --session feature-x
ab new -s feature-x
```

### Spawn a Docker Container

Spawn a Docker container for a workspace:

```bash
# Spawn container for session (positional argument)
ab spawn my-session

# Specify repository with -r/--repo
ab spawn my-session -r myrepo
ab spawn my-session --repo myrepo

# Create workspace and spawn container (--new flag)
ab spawn my-session --repo myrepo --new
ab spawn my-session -r myrepo -n

# Use git worktree instead of jj workspace
ab spawn my-session --repo myrepo --git

# Override entrypoint
ab spawn my-session --repo myrepo --entrypoint /bin/zsh
```

### Remove Repository

Remove all workspaces and repositories for a given repo ID:

```bash
# Show what would be deleted (dry run)
ab remove myrepo --dry-run

# Remove with confirmation prompt
ab remove myrepo

# Remove without confirmation
ab remove myrepo -f
ab remove myrepo --force
```

### Interactive Clean

Interactively select and clean repositories and their artifacts:

```bash
ab clean
```

### One-off Container

Spawn a one-off container with the current directory mounted:

```bash
# Mount as read-only (default)
ab oneoff

# Mount as read-write
ab oneoff -w
ab oneoff --write

# Override entrypoint
ab oneoff --entrypoint /bin/zsh
```

## How It Works

- **Directory Structure**:
  - `base_repo_dir`: Your source repositories (colocated jj/git repos)
  - `workspace_dir/git/{repo_path}/{session}`: Git worktrees
  - `workspace_dir/jj/{repo_path}/{session}`: JJ workspaces

- **New Workspace**:
  - For JJ: Creates a workspace from a colocated jj repo in `base_repo_dir` using `jj workspace add`
  - For Git: Creates a worktree from a git repo in `base_repo_dir` using `git worktree add`

- **Spawn Container**:
  - Mounts the workspace path as read-write
  - Mounts source repo's `.git` and `.jj` directories
  - Adds configured mounts (ro/rw, absolute/home_relative)
  - Runs as current user (uid:gid)
  - Sets working directory to the workspace

- **Repository Identification**:
  - Repos are identified by their relative path from `base_repo_dir`
  - Can search by full path (`fr/agent-box`) or partial name (`agent-box`)

## Requirements

- Rust (2024 edition)
- Git
- Jujutsu (for jj workspaces)
- Docker (for container spawning)

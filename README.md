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
base_repo_dir = "~/repos"         # Base directory for your repos (colocated jj/git repos)

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

## Usage

### Show Repository Information

```bash
ab info
```

Displays git worktrees and jj workspaces for the current repository.

### Create a New Workspace

```bash
# Create jj workspace (default), prompts for session name
ab new myrepo

# Create jj workspace with session name
ab new myrepo -s feature-x

# Create git worktree instead
ab new myrepo -s feature-x --git

# Use current directory's repo
ab new -s feature-x
```

### Spawn a Docker Container

```bash
# Spawn container for a session workspace
ab spawn -s my-session

# Specify repository
ab spawn -s my-session -r myrepo

# Create workspace and spawn container
ab spawn -s my-session -r myrepo -n

# Local mode: use current directory as workspace
ab spawn -l

# Override entrypoint
ab spawn -s my-session --entrypoint /bin/zsh
```

**Session vs Local mode:**
- `-s/--session`: Creates/uses a separate workspace directory, mounts source repo's `.git`/`.jj` separately
- `-l/--local`: Uses current directory as both source and workspace (mutually exclusive with `-s`)

## How It Works

- **Directory Structure**:
  - `base_repo_dir`: Your source repositories (colocated jj/git repos)
  - `workspace_dir/git/{repo_path}/{session}`: Git worktrees
  - `workspace_dir/jj/{repo_path}/{session}`: JJ workspaces

- **New Workspace**:
  - For JJ: Creates a workspace from a colocated jj repo using `jj workspace add`
  - For Git: Creates a worktree from a git repo using `git worktree add`

- **Spawn Container**:
  - Mounts the workspace path as read-write
  - In session mode: also mounts source repo's `.git` and `.jj` directories
  - In local mode: workspace and source are the same directory
  - Adds configured mounts (ro/rw, absolute/home_relative)
  - Runs as current user (uid:gid)
  - Sets working directory to the workspace

- **Repository Identification**:
  - Repos are identified by their relative path from `base_repo_dir`
  - Can search by full path (`fr/agent-box`) or partial name (`agent-box`)
  - If multiple repos match, prompts user to select

## How-To

### Share Host's Nix Store with Containers

To use binaries from your host's Nix store inside containers via the daemon socket:

```toml
[docker]
env = ["NIX_REMOTE=daemon"]

[docker.mounts.ro]
absolute = ["/nix/store"]

[docker.mounts.rw]
absolute = ["/nix/var/nix/daemon-socket/"]
```

This mounts the Nix store read-only and the daemon socket read-write, allowing the container to request builds/fetches from the host's Nix daemon.

## Requirements

- Rust (2024 edition)
- Git
- Jujutsu (for jj workspaces)
- Docker (for container spawning)

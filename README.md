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

Display git worktrees and jj workspaces for the current repository:

```bash
ab info
```

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
# Spawn container for a session workspace
ab spawn -s my-session
ab spawn --session my-session

# Specify repository with -r/--repo
ab spawn -s my-session -r myrepo
ab spawn --session my-session --repo myrepo

# Create workspace and spawn container (--new flag)
ab spawn -s my-session -r myrepo -n
ab spawn --session my-session --repo myrepo --new

# Use git worktree instead of jj workspace
ab spawn -s my-session -r myrepo --git

# Local mode: use current directory as workspace (no separate workspace)
ab spawn -l
ab spawn --local

# Override entrypoint
ab spawn -s my-session --entrypoint /bin/zsh
ab spawn -l --entrypoint /bin/zsh
```

**Session vs Local mode:**
- `-s/--session`: Creates/uses a separate workspace directory, mounts source repo's `.git`/`.jj` separately
- `-l/--local`: Uses current directory as both source and workspace (mutually exclusive with `-s`)

### Remove Repository

Remove all workspaces for a given repo ID:

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

## Requirements

- Rust (2024 edition)
- Git
- Jujutsu (for jj workspaces)
- Docker (for container spawning)

# Agent Box

Run AI coding agents in sandboxed Docker containers with full permissions (`--dangerously-skip-permissions` or equivalent) without risking your host system.

Agent Box manages Git/Jujutsu workspaces and spawns isolated containers where agents can freely execute commands, modify files, and install packages - all contained within a disposable environment.

![Demo](https://github.com/user-attachments/assets/c7aaaf16-fbcc-4669-97f3-33c423f2ff90)

## Why

AI coding agents like Claude Code, Cursor, and others work best when given full autonomy - but running `--dangerously-skip-permissions` on your host machine is risky. Agents can execute arbitrary commands, install packages, modify system files, or accidentally `rm -rf` something important.

Agent Box solves this by:
- **Sandboxing**: Agents run in Docker containers with no access to your host system
- **Disposable workspaces**: Each session gets a fresh Git worktree or JJ workspace that can be thrown away
- **Shared Nix store**: Optionally share your host's Nix store for fast, reproducible tooling without rebuilding inside containers
- **Easy iteration**: Spawn containers, let agents go wild, review changes, discard or keep - repeat

## Table of Contents

- [Why](#why)
- [Installation](#installation)
- [Configuration](#configuration)
- [Usage](#usage)
  - [Show Repository Information](#show-repository-information)
  - [Create a New Workspace](#create-a-new-workspace)
  - [Spawn a Docker Container](#spawn-a-docker-container)
- [How It Works](#how-it-works)
- [How-To](#how-to)
  - [Share Host's Nix Store with Containers](#share-hosts-nix-store-with-containers)
- [Requirements](#requirements)

## Installation

```bash
cargo install --path .
```

## Configuration

Create `~/.agent-box.toml`:

```toml
workspace_dir = "~/workspaces"    # Where git worktrees and jj workspaces are created
base_repo_dir = "~/repos"         # Base directory for your repos (colocated jj/git repos)

[runtime]
backend = "docker"                # Container runtime: "docker" or "podman" (default: docker)
image = "agent-box:latest"
entrypoint = ["/bin/bash"]

[runtime.mounts.ro]
absolute = ["/nix/store"]
home_relative = ["~/.config/git"]

[runtime.mounts.rw]
absolute = []
home_relative = ["~/.local/share"]

[runtime.mounts.o]  # Overlay mounts (Podman only)
absolute = []
home_relative = []
```

All paths support `~` expansion and will be canonicalized.

### Mount Path Syntax

Paths must be absolute (`/...`) or home-relative (`~/...`).

**`absolute` vs `home_relative`:**

The key difference is how single-path mounts (without explicit `:` mapping) handle the container path:

- **`absolute`**: Same path on both sides  
  `~/.config/git` → `/home/hostuser/.config/git:/home/hostuser/.config/git`

- **`home_relative`**: Host's home prefix is replaced with container's home  
  `~/.config/git` → `/home/hostuser/.config/git:/home/containeruser/.config/git`

**Explicit `source:dest` mapping:**

Both support explicit mappings where `~` expands to host home for source, container home for dest:

```toml
[runtime.mounts.rw]
# Map host socket to container's ~/.gnupg/S.gpg-agent
home_relative = ["/run/user/1000/gnupg/S.gpg-agent:~/.gnupg/S.gpg-agent"]
```

**Examples:**
```toml
[runtime.mounts.ro]
# Same path on both sides (stays /nix/store:/nix/store)
absolute = ["/nix/store"]

# Host ~/.config/git -> container ~/.config/git (home translated)
home_relative = ["~/.config/git"]

[runtime.mounts.rw]
# Explicit mapping with different paths
absolute = ["/host/path:/container/path"]
```

### Runtime Backends

Agent Box supports two container runtimes:

- **Docker** (default): Set `backend = "docker"` or omit the `backend` key
- **Podman**: Set `backend = "podman"`

**Differences:**
- Podman uses `--userns keep-id` for better user namespace mapping
- Podman supports overlay mounts (`mounts.o`) with the `:O` flag
- Docker uses direct `--user` mapping and does not support overlay mounts

**Overlay mounts** allow containers to write to a mounted directory without affecting the host. Changes are stored in a temporary overlay layer that is discarded when the container exits.

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

### Spawn a Container

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
  - Uses the configured runtime backend (docker or podman)

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
- Docker or Podman (for container spawning)

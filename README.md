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
  - [Layered Configuration](#layered-configuration)
  - [Mount Path Syntax](#mount-path-syntax)
  - [Port Mappings](#port-mappings)
  - [Runtime Backends](#runtime-backends)
  - [Profiles](#profiles)
  - [Validating Configuration](#validating-configuration)
  - [Previewing Resolved Configuration](#previewing-resolved-configuration)
- [Usage](#usage)
  - [Show Repository Information](#show-repository-information)
  - [Create a New Workspace](#create-a-new-workspace)
  - [Spawn a Container](#spawn-a-container)
- [How It Works](#how-it-works)
- [How-To](#how-to)
  - [Forward GPG Agent to Containers](#forward-gpg-agent-to-containers)
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
entrypoint = "/bin/bash"          # Shell-style command string (supports quotes for args with spaces)
skip_mounts = ["/nix/store/*", "/nix/var/nix"]  # Glob patterns for paths to always skip
ports = ["8080:8080"]                # Port mappings (Docker -p syntax)

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

### Layered Configuration

Agent Box supports layered configuration. It loads config files in the following order:

1. `~/.agent-box.toml` (global config, **required**)
2. `<git_root>/.agent-box.toml` (repo-local config, optional)

**Merge behavior:**
- **Scalar values** (strings, numbers, booleans, including `entrypoint`): repo-local overrides global
- **Arrays** (`env`, mount paths): repo-local values are appended to global values
- **Nested objects**: merged recursively

This allows you to define global defaults in `~/.agent-box.toml` and override or extend them per-repository.

**Example:**

Global config (`~/.agent-box.toml`):
```toml
workspace_dir = "~/workspaces"
base_repo_dir = "~/repos"

[runtime]
image = "default-agent:latest"
env = ["EDITOR=nvim"]

[runtime.mounts.ro]
home_relative = ["~/.config/git"]
```

Repo-local config (`~/repos/myproject/.agent-box.toml`):
```toml
[runtime]
image = "myproject-agent:latest"  # overrides global
entrypoint = '/bin/bash -c "nix develop"'  # overrides global (shell-style parsing)
env = ["PROJECT=myproject"]       # appended: ["EDITOR=nvim", "PROJECT=myproject"]

[runtime.mounts.ro]
home_relative = ["~/.ssh"]        # appended to global mounts
```

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

**Glob expansion:**

Mount paths support glob patterns (`*`, `?`, `[...]`). Each matching path is mounted individually with the same mode and path derivation rules:

```toml
[runtime.mounts.rw]
# Mounts every /tmp/kitty-* directory that exists at spawn time
absolute = ["/tmp/kitty-*"]

# Each match under ~/.config/ is home-translated independently
home_relative = ["~/.config/sock-*"]
```

- `home_relative` translation applies per expanded match
- Globs are **not** supported with explicit `src:dst` mappings (ambiguous: N sources, 1 dest)
- Zero matches are silently skipped, same as a non-existent literal path
- When using `-m`/`-M` on the CLI, **quote the glob** so the shell doesn't expand it first: `-M '/tmp/kitty-*'`

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

### Mount Behavior

**Symlink Handling:**

When a mount path contains symlinks, Agent Box automatically mounts the entire symlink chain so that paths resolve identically inside the container:

```bash
# If you have: ~/mylink -> /tmp/intermediate -> /data/actual
# Agent Box mounts all three:
#   ~/mylink:/home/user/mylink:rw
#   /tmp/intermediate:/tmp/intermediate:rw
#   /data/actual:/data/actual:rw
```

This ensures symlinks work correctly in the container.

**Path Coverage & Deduplication:**

Agent Box automatically deduplicates mounts and skips redundant paths:

- Paths are deduplicated by their canonical (resolved) path
- Subpaths under already-mounted directories are skipped
- Example: If `/nix/store` is mounted, `/nix/store/package` is redundant

**Non-Existent Path Filtering:**

Agent Box automatically filters out mount paths that don't exist on the host:

- Paths are checked for existence before being added to the container command
- Non-existent paths are silently filtered out with a debug message
- This prevents container spawn failures due to missing host paths
- Example: If `~/.config/nvim` is in your profile but doesn't exist, it's skipped

To see filtered paths, look for `DEBUG: Filtering out non-existent mount:` messages when spawning containers.

**Mount Coverage:**

When a mount path is under an already-mounted parent directory, it is automatically skipped (unless `--no-skip` is used):

- Mounts are checked against existing mounts to avoid redundancy
- If a path is already covered by a parent mount, the child mount is skipped
- This applies to all mode combinations (ro/rw/overlay)
- Use `--no-skip` flag to disable this behavior and mount all paths explicitly

Example:
```toml
[runtime.mounts.ro]
absolute = ["/nix/store"]

[profiles.dev.mounts.rw]
absolute = ["/nix/store/mydata"]  # Skipped - already covered by parent /nix/store
```

**Skipping Special Paths:**

Agent Box can be configured to always skip certain "special" paths that should never be mounted into containers:

- Patterns in `runtime.skip_mounts` are always skipped, even if explicitly configured
- Supports glob patterns with `*` wildcards (e.g., `/nix/store/*` to skip all subdirectories)
- This is useful for large system directories like `/nix/store` on NixOS
- Skip patterns are checked before mount coverage and deduplication
- The `--no-skip` flag does NOT affect skip_mounts - special paths are always skipped

**Glob Pattern Support:**

The `skip_mounts` option supports standard glob patterns with `*` wildcards (using the `glob` crate):
- `/nix/store/*` - Skip `/nix/store/` followed by exactly one path segment (e.g., `/nix/store/package`)
- `/nix/store/**` - Skip everything under `/nix/store` including nested paths
- `/tmp/test-*` - Skip paths starting with `test-` in `/tmp`
- `/*/*/temp` - Skip paths two levels deep ending in `temp`
- Exact paths like `/nix/var/nix` - Match that exact path only (not subdirectories unless using `**`)

**Note:** Glob patterns are matched against the full path string. For recursive matching (including all subdirectories), use `**` or ensure your pattern covers all cases.

Default skip patterns (on NixOS systems):
```toml
[runtime]
skip_mounts = ["/nix/store/**", "/nix/var/nix"]
```

To override the defaults, set an empty array or specify your own patterns:
```toml
[runtime]
skip_mounts = []  # Don't skip any special paths

# Or specify your own patterns:
skip_mounts = ["/nix/store/**", "/var/lib/**", "/usr/lib/**"]

# Skip specific subdirectories:
skip_mounts = ["/nix/store/glibc-*", "/nix/store/rustc-*"]
```

This is particularly useful when:
- You have large read-only system directories that shouldn't be mounted
- Symlink chains resolve to system paths you want to avoid mounting
- You want to reduce container startup time by avoiding large directory mounts
- You want to skip specific subdirectory patterns (like all glibc packages) while still allowing others

### Port Mappings

Port mappings expose container ports to the host, using the same layered configuration system as mounts and environment variables.

**Configuration:**

```toml
[runtime]
ports = ["8080:8080", "3000:3000"]

[profiles.dev]
ports = ["9000:9000"]
```

**Format:**

Port specs follow Docker's `-p` syntax:

- `CONTAINER_PORT` — Expose a container port on a random host port
- `HOST_PORT:CONTAINER_PORT` — Map a specific host port to a container port
- `HOST_IP:HOST_PORT:CONTAINER_PORT` — Bind to a specific host IP
- `HOST_PORT-END:CONTAINER_PORT-END` — Port ranges

**CLI:**

```bash
# Expose port 8080 on host and container
ab spawn -s my-session -P 8080:8080

# Multiple ports
ab spawn -s my-session -P 8080:8080 -P 3000:3000

# Combine with profiles (profile ports + CLI ports are merged)
ab spawn -s my-session -p dev -P 9000:9000
```

**Resolution order:**

1. `runtime.ports` (always applied first)
2. `default_profile` ports (if set)
3. CLI profiles (`-p`) ports in the order specified
4. CLI ports (`-P`) applied last

Duplicate port specs (exact string match) are automatically deduplicated — if the same spec appears in multiple profiles or on the CLI, only the first occurrence is kept.

### Runtime Backends

Agent Box supports two container runtimes:

- **Docker** (default): Set `backend = "docker"` or omit the `backend` key
- **Podman**: Set `backend = "podman"`

**Differences:**
- Podman uses `--userns keep-id` for better user namespace mapping
- Podman supports overlay mounts (`mounts.o`) with the `:O` flag
- Docker uses direct `--user` mapping and does not support overlay mounts

**Overlay mounts** allow containers to write to a mounted directory without affecting the host. Changes are stored in a temporary overlay layer that is discarded when the container exits.

### Profiles

Profiles let you define named sets of mounts and environment variables that can be selectively applied when spawning containers. This enables modular, reusable configurations for different toolchains.

**Basic profile definition:**

```toml
# Default profile applied to all spawn commands (optional)
default_profile = "base"

[profiles.base]
env = ["EDITOR=nvim"]

[profiles.base.mounts.ro]
absolute = ["/nix/store"]
home_relative = ["~/.config/git"]

[profiles.git]
extends = ["base"]  # Inherits mounts and env from base
env = ["GIT_AUTHOR_NAME=You"]

[profiles.git.mounts.ro]
home_relative = ["~/.gitconfig"]

[profiles.rust]
env = ["CARGO_HOME=/home/user/.cargo"]

[profiles.rust.mounts.ro]
home_relative = ["~/.cargo/config.toml"]

[profiles.rust.mounts.rw]
home_relative = ["~/.cargo/registry"]

[profiles.dev]
extends = ["rust"]
ports = ["8080:8080", "3000:3000"]  # Port mappings (inherited by children)

[profiles.gpg]
[profiles.gpg.mounts.o]  # Overlay (Podman only)
home_relative = ["~/.gnupg"]

[profiles.gpg.mounts.rw]
home_relative = [
  "/run/user/1000/gnupg/S.gpg-agent:~/.gnupg/S.gpg-agent",
]
```

**Using profiles:**

```bash
# Use only default_profile (if set)
ab spawn -s my-session

# Add specific profiles on top of default
ab spawn -s my-session -p git

# Combine multiple profiles
ab spawn -s my-session -p git -p rust -p gpg

# Profiles + additional CLI mounts and ports
ab spawn -s my-session -p rust -m ~/my-data -P 8080:8080
```

**Profile inheritance with `extends`:**

Profiles can inherit from other profiles using the `extends` field. Parent profiles are resolved depth-first, in order:

```toml
[profiles.base]
env = ["A=1"]

[profiles.git]
extends = ["base"]
env = ["B=2"]

[profiles.dev]
extends = ["git"]  # Inherits from git, which inherits from base
env = ["C=3"]
```

Using `-p dev` results in env: `["A=1", "B=2", "C=3"]`

**Resolution order:**

1. `runtime.mounts` and `runtime.env` (always applied first)
2. `default_profile` (if set)
3. CLI profiles (`-p`) in the order specified
4. CLI mounts (`-m`, `-M`) applied last

Arrays (mounts, env, ports) are concatenated. Duplicate mount paths and port specs (exact string match) are automatically deduplicated - if the same spec appears in multiple profiles, only the first occurrence is kept. Circular dependencies are detected and reported as errors.

**Profiles with layered configuration:**

Profiles work with [layered configuration](#layered-configuration). Repo-local profiles can extend profiles defined in the global config:

Global config (`~/.agent-box.toml`):
```toml
[profiles.base]
env = ["EDITOR=nvim"]

[profiles.base.mounts.ro]
absolute = ["/nix/store"]

[profiles.rust]
extends = ["base"]
env = ["CARGO_HOME=~/.cargo"]
```

Repo-local config (`~/repos/myproject/.agent-box.toml`):
```toml
# Override default profile for this repo
default_profile = "myproject-dev"

# Define a repo-specific profile that extends global "rust"
[profiles.myproject-dev]
extends = ["rust"]
env = ["PROJECT=myproject"]

[profiles.myproject-dev.mounts.rw]
home_relative = ["~/.local/share/myproject"]

# Add to an existing global profile
[profiles.rust]
env = ["RUST_BACKTRACE=1"]  # Appended to global rust.env
```

This allows:
- Repo-local profiles extending global profiles
- Overriding `default_profile` per-repo
- Adding mounts/env to existing global profiles (arrays are concatenated)

### Validating Configuration

Use `ab dbg validate` to check your profile configuration for errors:

```bash
ab dbg validate
```

This validates:
- `default_profile` references a defined profile
- All `extends` references point to defined profiles
- No circular dependencies in `extends` chains
- No self-references in `extends`

Example output for a valid configuration:
```
Configuration valid. No errors or warnings.

Profiles defined: 3
  - rust (extends: base)
  - base
  - git (extends: base)

Default profile: base
```

Example output with errors:
```
Errors:
  ✗ default_profile 'nonexistent' is not defined. Available profiles: ["base", "git"]
  ✗ Profile 'broken': extends unknown profile 'also_nonexistent'. Available profiles: ["base", "git"]

Warnings:
  ⚠ Profile 'empty': profile is empty (no mounts, env, ports, or extends)

Configuration invalid: 2 error(s), 1 warning(s).
```

### Previewing Resolved Configuration

Use `ab dbg resolve` to see the merged configuration after applying profiles:

```bash
# Show resolved config with just default_profile (if set)
ab dbg resolve

# Show resolved config with specific profiles
ab dbg resolve -p rust -p git
```

This shows the final merged mounts and environment variables after:
1. Starting with base `runtime.mounts` and `runtime.env`
2. Applying `default_profile` (if set)
3. Applying CLI-specified profiles in order

Example output:
```
Profiles applied (in order): base → rust

Resolved config:

  Mounts:
    ro: /nix/store -> /nix/store:/nix/store:ro
    rw: /nix/var/nix/daemon-socket/ -> /nix/var/nix/daemon-socket:/nix/var/nix/daemon-socket:rw
    ro: ~/.config/git (home-relative) -> /home/user/.config/git:/home/user/.config/git:ro
    rw: ~/.cargo (home-relative) -> /home/user/.cargo:/home/user/.cargo:rw
    O: ~/.gnupg (home-relative) -> /home/user/.gnupg:/home/user/.gnupg:O

  Environment:
    NIX_REMOTE=daemon

  Ports:
    8080:8080
    3000:3000
```

The output shows both the mount spec and the resolved bind string (`host:container:mode`).
Mounts marked `(home-relative)` will have their host home directory prefix mapped to the container's home directory (e.g., `/home/alice/.cargo` → `/home/bob/.cargo`).

If a mount path contains symlinks, all intermediate symlinks and the final target are mounted so that path resolution works identically in the container:
```
    rw: ~/mylink (home-relative) ->
      /home/user/mylink:/home/user/mylink:rw
      /home/user/intermediate:/home/user/intermediate:rw  
      /data/actual:/data/actual:rw
```

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

# Run a command in the container (passed to entrypoint)
ab spawn -s my-session -c pi
ab spawn -s my-session -c cargo build

# Override entrypoint (bypass nix develop wrapper)
ab spawn -s my-session -e /bin/bash

# Add additional profiles
ab spawn -s my-session -p git -p rust

# Add additional mounts (home-relative with -m, absolute with -M)
ab spawn -s my-session -m ~/data -m ro:~/.config/git
ab spawn -s my-session -M /nix/store -M ro:/etc/hosts

# Mount with explicit source:dest mapping
ab spawn -s my-session -m rw:~/src:/app/src
ab spawn -s my-session -m /run/user/1000/gnupg/S.gpg-agent:~/.gnupg/S.gpg-agent

# Expose ports
ab spawn -s my-session -P 8080:8080 -P 3000

# Combine profiles with additional mounts and ports
ab spawn -s my-session -p rust -m ~/project-data -P 8080:8080
```

**Session vs Local mode:**
- `-s/--session`: Creates/uses a separate workspace directory, mounts source repo's `.git`/`.jj` separately
- `-l/--local`: Uses current directory as both source and workspace (mutually exclusive with `-s`)

**Additional mounts (`-m` and `-M`):**

Add extra mounts beyond what's configured in `~/.agent-box.toml`:

- `-m` / `--mount`: Home-relative mount (container path translates `~` to container user's home)
- `-M` / `--Mount`: Absolute mount (same path on host and container)

Format: `[MODE:]PATH` or `[MODE:]SRC:DST`
- `MODE` is optional: `ro` (read-only), `rw` (read-write, default), or `o` (overlay, Podman only)
- `PATH` must be absolute (`/...`) or home-relative (`~/...`)

Examples:
```bash
-m ~/data           # rw mount, ~/data on host → ~/data in container
-m ro:~/.config     # ro mount
-M /nix/store       # rw mount, same absolute path on both sides
-M o:/tmp/cache     # overlay mount (Podman only)
-m ~/src:/app       # explicit mapping: ~/src on host → /app in container
-M '/tmp/kitty-*'   # glob: mounts every matching path (quote to prevent shell expansion)
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
  - Uses the configured runtime backend (docker or podman)

- **Repository Identification**:
  - Repos are identified by their relative path from `base_repo_dir`
  - Can search by full path (`fr/agent-box`) or partial name (`agent-box`)
  - If multiple repos match, prompts user to select

## How-To

### Forward GPG Agent to Containers

To use your host's GPG keys for signing inside containers, you need to:

1. Mount `~/.gnupg` as an overlay (so container writes don't affect host)
2. Mount the GPG socket files from the host's runtime directory

**Find your socket paths:**

On your host, run:
```bash
gpgconf --list-dirs
```

Look for these paths:
- `socketdir` - where GPG expects sockets (usually `~/.gnupg`)
- `agent-socket` - the gpg-agent socket
- `keyboxd-socket` - the keybox daemon socket (GPG 2.4+)

On most Linux systems with systemd, the actual sockets live in `/run/user/<UID>/gnupg/`.

**Configuration:**

```toml
[runtime.mounts.o]  # Overlay mount (Podman only)
home_relative = ["~/.gnupg"]

[runtime.mounts.rw]
# Mount sockets from host's runtime dir to container's ~/.gnupg
# Replace 1000 with your UID
home_relative = [
  "/run/user/1000/gnupg/S.gpg-agent:~/.gnupg/S.gpg-agent",
  "/run/user/1000/gnupg/S.keyboxd:~/.gnupg/S.keyboxd",
]
```

**Why overlay mount for `~/.gnupg`?**

GPG creates lock files and other temporary files in `~/.gnupg`. Without an overlay:
- Lock files from the host (with host PIDs) confuse the container
- Container writes would affect your host's GPG directory

The overlay mount lets the container see your keys and config but writes go to a temporary layer.

**For Docker users:**

Docker doesn't support overlay mounts. You can either:
1. Use Podman instead (`backend = "podman"`)
2. Mount `~/.gnupg` as read-write and accept that lock files may conflict

**Smartcard/YubiKey users:**

If your signing key is on a smartcard, also mount the scdaemon socket:
```toml
home_relative = [
  "/run/user/1000/gnupg/S.gpg-agent:~/.gnupg/S.gpg-agent",
  "/run/user/1000/gnupg/S.keyboxd:~/.gnupg/S.keyboxd",
  "/run/user/1000/gnupg/S.scdaemon:~/.gnupg/S.scdaemon",
]
```

**Troubleshooting:**

- **"Connection timed out" / "waiting for lock"**: Stale lock files in `~/.gnupg`. Use overlay mount or clean up `.#lk*` files.
- **"IPC call has been cancelled"**: Usually means your default key is on a smartcard that isn't connected. Specify a different key with `gpg -u <keyid>`.
- **Verify sockets are working**: Run `gpg-connect-agent 'getinfo socket_name' /bye` - should show the socket path and return `OK`.
- **List keys**: `gpg --list-secret-keys` - keys with `>` after `sec` are on smartcards.

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

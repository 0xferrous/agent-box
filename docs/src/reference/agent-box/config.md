# Agent-box configuration reference

Main configuration file: `~/.agent-box.toml`

## Layered configuration

Load order:

1. `~/.agent-box.toml` (required)
2. `{git-root}/.agent-box.toml` (optional)

Merge behavior:

- Scalars: repo-local overrides global
- Arrays: values are appended
- Objects: merged recursively

## Root keys

- `workspace_dir` (path): base directory for generated workspaces
- `base_repo_dir` (path, default `/`): base directory for source repositories. When defaulting to `/`, the full filesystem path is mirrored inside the workspace directory (e.g., `/home/user/repos/myproject` becomes `{workspace_dir}/git/home/user/repos/myproject/{session}`). Set this to a common ancestor of your repos (e.g., `~/repos`) for shorter workspace paths.
- `default_profile` (string|null): profile automatically applied to `ab spawn`
- `profiles` (table): named profile definitions
- `runtime` (table): runtime/backend settings
- `context` (string): root context content
- `context_path` (string, default `/tmp/context`): in-container path for context file
- `portal` (table): portal host integration settings

All paths support `~` expansion.

## `[runtime]`

- `backend` (string, default `podman`): `podman` or `docker`
- `image` (string): container image
- `entrypoint` (shell-style string): parsed to argv
- `env` (array of `KEY=VALUE`)
- `env_passthrough` (array of variable names)
- `ports` (array of `-p` compatible port mappings)
- `hosts` (array of `HOST:IP` entries)
- `skip_mounts` (array of glob patterns)
- `mounts` (table): `ro`, `rw`, and `o` mount categories

## Mount table shape

Each of `runtime.mounts.ro`, `runtime.mounts.rw`, and `runtime.mounts.o` has:

- `absolute` (array of strings)
- `home_relative` (array of strings)

Mount modes:

- `ro`: read-only
- `rw`: read-write
- `o`: overlay (Podman only)

## CLI additional mount syntax (`ab spawn`)

- `[MODE:]PATH`
- `[MODE:]SRC:DST`

`MODE` values: `ro`, `rw`, `o` (default: `rw`).

Examples:

- `-m ~/data`
- `-m ro:~/.config/git`
- `-m rw:~/src:/app/src`
- `-M /nix/store`
- `-M o:/tmp/cache`

## Environment passthrough

`env_passthrough` copies host env values into the container at spawn time.

Example:

```toml
[runtime]
env_passthrough = ["PATH", "SSH_AUTH_SOCK", "TERM"]
```

## Context composition

Context is built in this order:

1. Root `context`
2. Each resolved profile `context` in profile resolution order

Values are joined with newlines and written to a temp file mounted at `context_path`.

## Port mappings

Port values follow container runtime `-p` syntax:

- `HOST_PORT:CONTAINER_PORT`
- `HOST_IP:HOST_PORT:CONTAINER_PORT`
- `CONTAINER_PORT`

Example:

```toml
[runtime]
ports = ["8080:8080", "127.0.0.1:9090:9090", "3000"]
```

## Host entries

`hosts` entries are passed as runtime `--add-host` values.

Example:

```toml
[runtime]
hosts = ["host.docker.internal:host-gateway", "myhost:10.0.0.1"]
```

## Network mode

CLI flag: `ab spawn --network=MODE`

Typical values:

- `host`
- `bridge`
- `none`
- runtime-specific named network

On Docker, `--network=host` conflicts with published ports and add-host options.

## Runtime backend differences

- Podman: supports overlay mount mode (`o`) and keep-id user namespace behavior
- Docker: no overlay mounts; uses direct user mapping

## Profiles

Profiles are reusable config fragments you can layer on top of runtime defaults.

Profile table: `[profiles.NAME]`

Supported keys:

- `extends` (array of profile names)
- `mounts` (same shape as runtime mounts)
- `env` (array of `KEY=VALUE`)
- `env_passthrough` (array of variable names)
- `ports` (array of port mapping strings)
- `hosts` (array of `HOST:IP` entries)
- `context` (string)

### Profile inheritance (`extends`)

A profile can inherit from one or more profiles using `extends`.
Inherited values are merged using the same layered rules described above:

- Scalars override
- Arrays append
- Objects merge recursively

Example:

```toml
[profiles.base]
env = ["RUST_BACKTRACE=1"]
mounts.rw.home_relative = ["~/.cargo"]

[profiles.gpg]
mounts.rw.absolute = ["/run/user/1000/gnupg/S.gpg-agent:~/.gnupg/S.gpg-agent"]

[profiles.dev]
extends = ["base", "gpg"]
ports = ["8080:8080"]
```

### Activation order

Final runtime config is resolved in this order:

1. root runtime config (`[runtime]`)
2. `default_profile` (if configured)
3. each CLI profile flag in order (`ab spawn -p one -p two`)

That means later profiles can override scalar values from earlier layers, while arrays continue to append.

### Typical usage

Set a baseline profile for daily use:

```toml
default_profile = "base"
```

Then add task-specific profiles per spawn:

```bash
ab spawn -r myrepo -s mysession -p rust -p gpg
```

## Validation and inspection

Validate config:

```bash
ab dbg validate
```

Preview merged config/profile resolution:

```bash
ab dbg resolve
ab dbg resolve -p rust -p gpg
```

## Portal integration

Portal config is defined under `[portal]` in the same file.

See [Portal config](../portal/config.md) and [Portal wrapper contract](../portal/wrapper-contract.md).

## Migrating away from `base_repo_dir`

If you remove `base_repo_dir` from your configuration (or change its value), existing workspaces become **unresolved** because their paths were computed relative to the old `base_repo_dir` value.

To inspect and clean up:

1. Run `ab dbg list --unresolved` to see which workspaces no longer resolve to a source repository.
2. Run `ab dbg remove --unresolved` to delete the orphaned workspace directories.

Workspaces created via symlinked paths before canonicalization was introduced may also appear unresolved, since the canonical path differs from the original symlink path.

If you prefer to avoid any disruption, you can keep your existing `base_repo_dir` setting. It remains fully supported and provides backward compatibility with all previously created workspaces.

## JSON Schema

A machine-readable JSON Schema for the configuration is available for validation, IDE autocompletion, and tool integration.

- [Raw schema file](https://raw.githubusercontent.com/0xferrous/agent-box/main/common/config.schema.json)
- [GitHub UI view](https://github.com/0xferrous/agent-box/blob/main/common/config.schema.json)

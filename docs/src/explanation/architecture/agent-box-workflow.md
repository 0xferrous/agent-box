# Agent-box workflow internals

This page explains the runtime flow behind `ab new` and `ab spawn`.

## Repository/workspace model

- Source repositories can be located anywhere on the filesystem. When `base_repo_dir` is at the default (`/`), the full repo path is mirrored inside the workspace directory. Setting `base_repo_dir` to a common ancestor produces shorter workspace paths.
- Workspaces are created under `workspace_dir`.
- Workspace mode is either JJ workspace or Git worktree.
- When running from inside a linked git worktree (e.g., a session workspace), `ab new` and `ab spawn` in session mode resolve to the main repository root, not the worktree itself. `ab spawn --local` preserves the current worktree path.

## `ab new` flow

1. Resolve repository ID (explicit or from current directory).
2. Choose workspace type (JJ default, or Git).
3. Create workspace for selected session name.

## `ab spawn` flow

1. Resolve workspace path (`--session` mode) or current dir (`--local`).
2. Load and validate layered configuration.
3. Resolve profile graph (`default_profile` + CLI profiles).
4. Build runtime-specific container configuration.
5. Apply mounts/env/ports/hosts/network options.
6. If portal is enabled:
   - `portal.global = true`: mount configured portal socket and set `AGENT_PORTAL_SOCKET`.
   - `portal.global = false`: start a per-container in-process portal host, mount its socket, and set `AGENT_PORTAL_SOCKET`.
7. Execute selected runtime backend (Podman or Docker).

## Path resolution notes

- Home-relative paths are translated for host/container user homes.
- Relative mount source paths are resolved from current working directory.
- Symlinked paths are expanded to preserve resolution behavior inside container.

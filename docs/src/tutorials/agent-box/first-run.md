# Tutorial: Agent-box first run

## Outcome

You create a workspace and start a containerized session with `ab`.

## Prerequisites

- `ab` installed
- Docker or Podman available
- `~/.agent-box.toml` created with `workspace_dir` and `runtime.image` (optionally `base_repo_dir` for shorter workspace paths)

## Steps

1. Check basic CLI access:

    ```bash
    ab info
    ```

2. Create a workspace (JJ by default):

    ```bash
    ab new myrepo -s first-session
    ```

3. Spawn the container:

    ```bash
    ab spawn -r myrepo -s first-session
    ```

4. Inside the container, verify where you are:

    ```bash
    pwd
    ```

    You should be in the workspace path managed by Agent-box.

## What you learned

- How to create a named session workspace
- How to spawn a container bound to that workspace

Next: see [How-to guides](../../how-to/index.md) for custom runtime and CI usage.

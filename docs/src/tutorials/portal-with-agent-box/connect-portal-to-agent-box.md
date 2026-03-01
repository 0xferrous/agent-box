# Tutorial: Connect Portal to Agent-box

## Outcome

You run an Agent-box container where tools can use Portal through wrapper binaries.

## Prerequisites

- Agent-box setup working (`ab spawn` succeeds)
- Portal host running (`agent-portal-host`)
- Wrappers installed in container image or mounted into container PATH

## Steps

1. Enable portal in config:

    ```toml
    [portal]
    enabled = true
    socket_path = "/run/user/1000/agent-portal/portal.sock"

    [portal.policy.defaults]
    clipboard_read_image = "allow"
    gh_exec = "ask_for_writes"
    ```

2. Start portal host on the machine running containers:

    ```bash
    agent-portal-host
    ```

3. Spawn an Agent-box session:

    ```bash
    ab spawn -r myrepo -s portal-session
    ```

    Agent-box mounts the configured socket and sets `AGENT_PORTAL_SOCKET` in the container.

4. In the container, validate wrapper-backed flow:

    ```bash
    wl-paste --list-types
    ```

    If wrappers are in PATH and policy allows, this returns an image MIME type when present.

## What you learned

- How Agent-box and Portal integrate
- How wrappers keep calling conventions tool-compatible

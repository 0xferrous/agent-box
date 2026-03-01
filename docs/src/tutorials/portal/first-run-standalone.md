# Tutorial: Portal standalone first run

## Outcome

You run `agent-portal-host` and send successful requests with `agent-portal-cli`.

## Prerequisites

- `agent-portal-host` and `agent-portal-cli` installed
- `~/.agent-box.toml` with `[portal]` enabled
- `wl-paste` available on host if testing clipboard method

## Minimal config

```toml
[portal]
enabled = true
socket_path = "/run/user/1000/agent-portal/portal.sock"

[portal.policy.defaults]
clipboard_read_image = "allow"
gh_exec = "ask_for_writes"
```

## Steps

1. Start the host service in a terminal:

    ```bash
    RUST_LOG=info agent-portal-host
    ```

2. In another terminal, verify connectivity:

    ```bash
    agent-portal-cli ping
    agent-portal-cli whoami
    ```

3. Optionally test image clipboard request:

    ```bash
    agent-portal-cli clipboard-read-image --out /tmp/clip.bin
    ```

## What you learned

- Portal works independently of Agent-box
- Portal methods are reachable over a Unix socket via official CLI

Next: [Portal wrapper contract](../../reference/portal/wrapper-contract.md).

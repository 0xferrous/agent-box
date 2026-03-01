# Portal wrapper contract reference

This page describes behavior expected from compatibility wrappers in `wrappers/`.

## General contract

- Wrappers expose familiar command names/flags.
- Wrappers do **not** call host capability directly inside container.
- Wrappers call Portal methods via Unix socket.
- Wrappers forward output as tool-compatible stdout/stderr.
- Wrapper process exit code matches operation result semantics.

## Socket resolution order

1. `AGENT_PORTAL_SOCKET` environment variable
2. `~/.agent-box.toml` -> `[portal].socket_path`
3. Built-in default `/run/user/<uid>/agent-portal/portal.sock`

## `wl-paste` wrapper contract

- `--list-types` returns a single available image MIME type selected by portal policy.
- `--type <mime> --no-newline` writes raw image bytes.
- If requested MIME does not match available MIME, wrapper errors.

## `gh` wrapper contract

- Forwards argv as `gh.exec` request payload.
- Includes a human-readable `reason` string for prompt/audit context.
- Does not prompt in-container.
- Prints portal-returned stdout/stderr and exits with portal-returned exit code.

## Host-side execution model

- Policy decisions and prompts are enforced by `agent-portal-host`.
- Host service resolves host-native binaries (`gh`, `wl-paste`) to avoid wrapper recursion.

## Versioning

Current request/response protocol version field is `1`.

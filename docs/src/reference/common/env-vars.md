# Environment variables reference

## Portal-related

- `AGENT_PORTAL_SOCKET`
  - Used by portal clients/wrappers to select socket path.
  - Resolution priority is env var first, then config/default.

- `AGENT_PORTAL_HOST_GH`
  - Used by `agent-portal-host` to override host `gh` binary path.

## Logging

- `RUST_LOG`
  - Controls tracing filter for `agent-portal-host` and other Rust binaries using tracing subscriber.

- `XDG_STATE_HOME`
  - Used to resolve the default Portal log directory.
  - Default Portal log directory: `$XDG_STATE_HOME/agent-box/logs/`
  - Fallback when unset: `~/.local/state/agent-box/logs/`
  - Each Portal log filename is derived from the socket filename, replacing `.sock` with `.log`

## Runtime passthrough

Variables listed in `[runtime].env_passthrough` are copied from host into container at spawn time.

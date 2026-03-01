# Environment variables reference

## Portal-related

- `AGENT_PORTAL_SOCKET`
  - Used by portal clients/wrappers to select socket path.
  - Resolution priority is env var first, then config/default.

- `AGENT_PORTAL_HOST_WL_PASTE`
  - Used by `agent-portal-host` to override host `wl-paste` binary path.

- `AGENT_PORTAL_HOST_GH`
  - Used by `agent-portal-host` to override host `gh` binary path.

## Logging

- `RUST_LOG`
  - Controls tracing filter for `agent-portal-host` and other Rust binaries using tracing subscriber.

## Runtime passthrough

Variables listed in `[runtime].env_passthrough` are copied from host into container at spawn time.

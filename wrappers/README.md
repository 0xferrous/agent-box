# agent-wrappers

Compatibility wrapper binaries that make portal-backed host access transparent to tools/agents.

## Binaries

- `wl-paste`: wrapper compatible with image clipboard flow
- `agent-portal-client`: small helper CLI for scripts/wrappers
- `gh`: transparent guest wrapper that forwards command execution to portal method `gh.exec`
- `portal/scripts/gh-policy-gen.py`: optional report generator for `gh` leaf command read/write classification

## Goal

Tools keep calling familiar commands while wrappers proxy requests to the portal socket.

## Development

From repo root:

```bash
cargo run -p agent-wrappers --bin wl-paste -- --help
cargo run -p agent-wrappers --bin agent-portal-client -- --help
nix-shell -p gh python3 --run 'python3 portal/scripts/gh-policy-gen.py'
cargo run -p agent-wrappers --bin gh -- --version
cargo test -p agent-wrappers
```

## gh wrapper flow

```bash
cargo run -p agent-wrappers --bin gh -- pr view 123
```

The wrapper does **not** execute host `gh` directly and does not prompt in-container.
It sends `gh.exec` requests to `agent-portal-host`, where policy and prompt behavior are enforced on host side.
Host policy mode is configured via `portal.policy.defaults.gh_exec` (`ask_for_writes|ask_for_all|ask_for_none|deny_all`).

Optional env vars:

- `AGENT_PORTAL_SOCKET` (override portal socket path)

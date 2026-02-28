# agent-portal

Portal crate containing host service and official CLI for container-to-host mediated access.

## Binaries

- `agent-portal-host`: host daemon listening on Unix socket
- `agent-portal-cli`: official API/operations CLI client

## Current Methods

- `ping`
- `whoami`
- `clipboard.read_image`
- `gh.exec`

`gh.exec` classification uses an embedded-at-compile-time command policy generated at repo root
via `portal/scripts/gh-policy-gen.py`:
`portal/gh-leaf-command-read-write-report.json`.
Policy mode is configured in `~/.agent-box.toml` via `portal.policy.defaults.gh_exec`.

## Logging

`agent-portal-host` uses `tracing` + `RUST_LOG` filtering.

Example:

```bash
RUST_LOG=debug agent-portal-host
```

## Development

From repo root:

```bash
cargo run -p agent-portal --bin agent-portal-host -- --help
cargo run -p agent-portal --bin agent-portal-cli -- --help
cargo test -p agent-portal
```

## Integration tests

- `portal/tests/host_integration.rs`

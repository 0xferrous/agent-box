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

`clipboard.read_image` is implemented directly against the Wayland clipboard via
[`wl-clipboard-rs`](https://github.com/YaLTeR/wl-clipboard-rs), rather than shelling out to
`wl-paste`.

`gh.exec` classification uses an embedded-at-compile-time command policy generated at repo root
via `portal/scripts/gh-policy-gen.py`:
`portal/gh-leaf-command-read-write-report.json`.
Policy mode is configured in `~/.agent-box.toml` via `portal.policy.defaults.gh_exec`.

## Logging

`agent-portal-host` uses `tracing` and writes logs to stderr and a log file when run directly. Managed per-container Portal instances started by `ab spawn` write logs only to files.

Default log file location:

```text
${XDG_STATE_HOME:-~/.local/state}/agent-box/logs/<socket-name>.log
```

The log filename is derived from the socket filename by replacing `.sock` with `.log`.

Examples:

```text
portal.sock -> portal.log
portal-12345-abc.sock -> portal-12345-abc.log
```

Use `RUST_LOG` for tracing filter control.

Example:

```bash
RUST_LOG=agent_portal=debug,agent_portal_host=trace agent-portal-host
```

Managed per-container Portal instances started by `ab spawn` also initialize logging this way, so each managed socket gets a matching per-instance log file.

## Development

From repo root:

```bash
cargo run -p agent-portal --bin agent-portal-host -- --help
cargo run -p agent-portal --bin agent-portal-cli -- --help
cargo test -p agent-portal
```

## Integration tests

- `portal/tests/host_integration.rs`

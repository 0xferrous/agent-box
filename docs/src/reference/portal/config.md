# Portal configuration reference

Portal config lives under `[portal]` in `~/.agent-box.toml`.

## Top-level keys

- `enabled` (bool, default: `true`)
- `global` (bool, default: `false`)
  - `true`: use a long-lived user-managed `agent-portal-host`
  - `false`: `ab spawn` starts a per-container `agent-portal-host` and tears it down when the container exits
- `socket_path` (string, default: `/run/user/<uid>/agent-portal/portal.sock`)
  - used directly when `global = true`
  - ignored by `ab spawn` when `global = false`, because `ab` allocates a unique per-container socket path
- `prompt_command` (string|null, default: unset)
- Logging is controlled at process startup, not in config:
  - `RUST_LOG=...` provides tracing filter control
  - logs are written under `${XDG_STATE_HOME:-~/.local/state}/agent-box/logs/`
  - each log filename is derived from the socket filename, replacing `.sock` with `.log`
- `timeouts.request_ms` (u64, default: `0` = no timeout)
- `timeouts.prompt_ms` (u64, default: `0` = no timeout)
- `limits.max_inflight` (usize, default: `32`)
- `limits.prompt_queue` (usize, default: `64`)
- `limits.rate_per_minute` (u32, default: `60`)
- `limits.rate_burst` (u32, default: `10`)
- `limits.max_clipboard_bytes` (usize, default: `20971520`)
- `clipboard.allowed_mime` (array of strings, default: `image/png`, `image/jpeg`, `image/webp`)

## Operation modes

Managed per-container mode is the default.

### Global mode

Use this when you want to run Portal yourself, for example via a user service.

```toml
[portal]
enabled = true
global = true
socket_path = "/run/user/1000/agent-portal/portal.sock"
```

You only need to set `global = true` when you want to override the default managed mode.

### Managed per-container mode

Use this when you want `ab` to start a dedicated Portal instance for each `ab spawn`.

```toml
[portal]
enabled = true
global = false
```

In this mode, `ab`:

1. starts an in-process Portal host before launching the container
2. chooses a unique socket path for that spawn
3. mounts that socket into the container
4. sets `AGENT_PORTAL_SOCKET` for the container
5. shuts the Portal host down after the container exits

## Policy defaults

`[portal.policy.defaults]`

- `clipboard_read_image`: `allow | ask | deny` (default: `allow`)
- `gh_exec`: `ask_for_writes | ask_for_all | ask_for_none | deny_all` (default: `ask_for_writes`)
  - aliases accepted by config parser:
    - `allow` -> `ask_for_none`
    - `ask` -> `ask_for_writes`
    - `deny` -> `deny_all`

## Per-container policy override

`[portal.policy.containers."<container-id>"]`

Container ID is resolved from peer process cgroup metadata.

Example:

```toml
[portal.policy.containers."3f7a1d5c2b8e"]
clipboard_read_image = "deny"
gh_exec = "ask_for_all"
```

## Example

```toml
[portal]
enabled = true
global = true
socket_path = "/run/user/1000/agent-portal/portal.sock"
prompt_command = "rofi -dmenu -p 'agent-portal'"

[portal.timeouts]
request_ms = 5000
prompt_ms = 15000

[portal.limits]
max_inflight = 32
prompt_queue = 64
rate_per_minute = 60
rate_burst = 10
max_clipboard_bytes = 20971520

[portal.clipboard]
allowed_mime = ["image/png", "image/jpeg", "image/webp"]

[portal.policy.defaults]
clipboard_read_image = "allow"
gh_exec = "ask_for_writes"
```

## JSON Schema

Portal configuration is part of the overall `~/.agent-box.toml` schema. The full JSON Schema can be used for validation and IDE autocompletion.

- [Raw schema file](https://raw.githubusercontent.com/0xferrous/agent-box/main/common/config.schema.json)
- [GitHub UI view](https://github.com/0xferrous/agent-box/blob/main/common/config.schema.json)

# Portal configuration reference

Portal config lives under `[portal]` in `~/.agent-box.toml`.

## Top-level keys

- `enabled` (bool, default: `true`)
- `socket_path` (string, default: `/run/user/<uid>/agent-portal/portal.sock`)
- `prompt_command` (string|null, default: unset)
- `timeouts.request_ms` (u64, default: `0` = no timeout)
- `timeouts.prompt_ms` (u64, default: `0` = no timeout)
- `limits.max_inflight` (usize, default: `32`)
- `limits.prompt_queue` (usize, default: `64`)
- `limits.rate_per_minute` (u32, default: `60`)
- `limits.rate_burst` (u32, default: `10`)
- `limits.max_clipboard_bytes` (usize, default: `20971520`)
- `clipboard.allowed_mime` (array of strings, default: `image/png`, `image/jpeg`, `image/webp`)

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

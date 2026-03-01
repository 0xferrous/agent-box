# How to enable pi image paste via portal wrapper

## Goal

Make image paste work transparently in `pi` sessions using portal-backed `wl-paste` wrapper.

## Requirements

1. `agent-portal-host` running on host
2. Portal enabled in `~/.agent-box.toml`
3. Wrapper binary (`wl-paste`) available in container PATH before system `wl-paste`

## Steps

1. Start portal host:

```bash
agent-portal-host
```

2. Spawn with Agent-box (portal enabled):

```bash
ab spawn -s my-session
```

3. In container/session env:

```bash
export WAYLAND_DISPLAY=wayland-1
# AGENT_PORTAL_SOCKET is injected by ab when portal is enabled
```

4. Validate wrapper flow:

```bash
wl-paste --list-types
wl-paste --type image/png --no-newline
```

## Notes

- Policy is enforced host-side by `agent-portal-host`.
- Wrapper should not prompt in-container.
